# Test plan

Goal: a **lean** but meaningful test suite that (1) keeps the existing DDS
behaviour green and (2) proves each Zenoh feature interoperates with real ROS 2
over `rmw_zenoh`. Total CI budget target: **< 30 min**. Sources of use cases:
[`research/workshop_use_cases.md`](research/workshop_use_cases.md) and the basic
ROS 2 CLI the user called out.

## 1. What exists today

**Tests** (pure Rust, `ros2-client`↔`ros2-client`, RustDDS multicast in-process):
- `tests/pub_and_sub.rs` — two nodes, `std_msgs/String` on a topic, 5 s timeout.
- `tests/late_joiner.rs` — transient-local + reliable, late subscriber gets
  buffered history.
- Unit tests inside `src/` (`names.rs`, `context.rs` node-create).

**CI** (`.github/workflows/`):
- `static-checks.yml` — `fmt` (nightly), `doc`, `clippy` (nightly, all
  lib/bins/examples), `msrv` (`cargo check` on 1.85.1).
- `tests.yml` — `cargo test --test-threads=1` and `--features=security`, Ubuntu.
- `tests-macos.yml` — `cargo test` on macOS.
- `audit.yml` — `cargo audit` on `Cargo.*` changes.

**Gaps:** no Zenoh build coverage; no interop against real ROS 2; no unit tests
for wire formats (keyexpr/QoS/attachment/gid/type-hash); no service/action/param
integration tests even for DDS.

## 2. Design: three test tiers

### Tier A — Unit tests (no network, run everywhere, milliseconds)
Deterministic wire-format tests that pin our bytes/strings to `rmw_zenoh`'s
(from [`research/rmw_zenoh.md`](research/rmw_zenoh.md)). These catch interop
regressions without needing ROS or a router.

| ID | Asserts | Feature |
|----|---------|---------|
| A1 | data keyexpr `0/chatter/std_msgs::msg::dds_::String_/RIHS01_…` | E2 |
| A2 | liveliness keyexpr for NN/MP/MS/SS/SC incl. `%` mangling | E2 |
| A3 | compact QoS encode/decode round-trip; `::,7:,:,:,,` for depth 7; `depth==0`→42 | E1/E2 |
| A4 | attachment byte layout `(i64 seq, i64 ts, 1-byte len 16, 16-byte gid)` via `z_serialize` | E2 |
| A5 | GID = XXH3-128(liveliness key), low64‖high64 LE, vs a known vector | E2 |
| A6 | type-hash table lookups; wildcard-hash key for receivers | E2 |
| A7 | CDR round-trip of `std_msgs/String`, a struct, `AddTwoInts` req/resp; 4-byte encapsulation header present exactly once | E2/E4 |
| A8 | `QosProfile` ↔ `rustdds::QosPolicies` conversions (dds build) | E1 |
| A9 | existing `names.rs` tests keep passing | — |

### Tier B — In-process integration (`ros2-client`↔`ros2-client` over Zenoh, no ROS)
Two contexts in one test process, **connected directly** (peer A listens on
`tcp/localhost:<port>`, peer B connects) so **no `zenohd` router is required** —
keeps these hermetic and fast. Mirrors the existing DDS tests.

| ID | Scenario | Feature |
|----|----------|---------|
| B1 | pub/sub `std_msgs/String`, volatile (port of `pub_and_sub.rs`) | E4 |
| B2 | transient-local late joiner gets history (port of `late_joiner.rs`) | E4 |
| B3 | graph: subscriber node sees publisher via graph cache; `wait_for_subscription` resolves | E5 |
| B4 | service `AddTwoInts` request/response, assert `sum` | E6 |
| B5 | action `Fibonacci`: goal → feedback → result sequence | E7 |
| B6 | parameters: set/get/list round-trip via the param services | E8 |

These also run under the **`dds`** feature (same source, cfg-gated harness) so we
don't lose DDS coverage — B1/B2 already exist as DDS tests.

### Tier C — Interop with real ROS 2 (`rmw_zenoh`), the definition of done
One CI job on a `ros:jazzy` container with `rmw_zenoh_cpp`, `demo_nodes_cpp`,
`example_interfaces`, `action_tutorials_*` installed, a `rmw_zenohd` router
running, and Rust to build our examples. `RMW_IMPLEMENTATION=rmw_zenoh_cpp` on
the ROS side. Each case is a small script asserting on output; a
[`ros2-client`] node is one side, a ROS 2 CLI/demo node the other.

| ID | Scenario | Direction | Feature |
|----|----------|-----------|---------|
| C1 | `ros2-client` talker → `ros2 topic echo /chatter` sees the string | pub→ROS | E4 |
| C2 | `ros2 topic pub /chatter std_msgs/String` → `ros2-client` listener prints it | ROS→sub | E4 |
| C3 | `ros2 topic list` / `ros2 node list` show the `ros2-client` entity | graph | E5 |
| C4 | `ros2 service call /add_two_ints …` → `ros2-client` server replies `sum` | ROS→srv | E6 |
| C5 | `ros2-client` client → C++ `add_two_ints_server`, assert `sum` | client→ROS | E6 |
| C6 | `ros2 action send_goal /fibonacci … --feedback` → `ros2-client` server | ROS→action | E7 |
| C7 | `ros2-client` action client → C++ `fibonacci_action_server` | client→ROS | E7 |
| C8 | `ros2 param set/get` on a `ros2-client` node | params | E8 |
| C9 | `ros2 topic echo /rosout` shows a `ros2-client` log line | rosout | E9 |

C1–C4/C8/C9 are the lean core; C5/C6/C7 add the reverse direction. If CI time is
tight, C5/C7 (reverse service/action) may be marked `slow` and run on a nightly
schedule rather than per-PR — but the aim is to keep all of C in the < 30 min
per-PR budget.

## 3. CI changes

Add one workflow `tests-zenoh.yml` (per-PR):

1. **Job `build-zenoh`** (fast, always): `cargo check/clippy/doc/test
   --no-default-features --features zenoh` — compiles the Zenoh backend and runs
   **Tier A** unit tests. Also `cargo check --no-default-features --features dds`
   to guard the exclusivity. (~5–8 min)
2. **Job `integration-zenoh`** (fast, always): run **Tier B** in-process tests
   (`cargo test --no-default-features --features zenoh -- --test-threads=1`).
   No router needed (direct peer connection). (~3–5 min)
3. **Job `interop-ros2`** (`container: ros:jazzy`): install
   `ros-jazzy-rmw-zenoh-cpp ros-jazzy-demo-nodes-cpp
   ros-jazzy-example-interfaces ros-jazzy-action-tutorials-cpp` + rustup; start
   `rmw_zenohd`; run the **Tier C** scripts under `RMW_IMPLEMENTATION=
   rmw_zenoh_cpp`. Cache cargo + apt. (~12–18 min)

Keep existing `tests.yml`/`static-checks.yml` for the **`dds`** default so DDS
never regresses. `static-checks` gains a `--features zenoh` clippy/doc pass.

Budget: A+B jobs ≈ 10 min, C job ≈ 15 min, run in parallel → wall-clock well
under 30 min. `msrv`/`audit`/`macos` unaffected.

## 4. Harness notes

- Tier C scripts live in `interop/zenoh/` (shell + tiny Rust example binaries),
  invoked by the workflow; each has a hard timeout and asserts on captured
  stdout, following the pattern of the workshop ex-1 commands.
- In-process Tier B uses explicit connect/listen endpoints to avoid multicast
  flakiness and the router dependency.
- Every new feature PR (E4–E9) must land with its Tier A/B tests and wire its
  Tier C case, so CI coverage grows with the implementation rather than at the
  end.
- Local dev (this repo's environment currently has **no** ROS/zenohd): install a
  real ROS 2 + `rmw_zenoh` via `pixi`/RoboStack or the `ros:jazzy` Docker image
  when running Tier C locally; Tiers A/B need only the Rust toolchain and (for B)
  no external services.

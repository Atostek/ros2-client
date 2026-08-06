# 10. Converge DDS and Zenoh public API surfaces

- Status: accepted (directional plan; implementation is phased)
- Date: 2026-08-05
- Updated: 2026-08-05
- Relates to: ADR-0002, ADR-0003, ADR-0004, ADR-0005, PR #73 / branch `zenoh`,
  issue #71
- Partially supersedes: the “minimal churn / leave DDS create_* on
  `QosPolicies`” interim of ADR-0004 — that remains valid as a short-term
  landing strategy for Zenoh, but the **end state** is one ROS-shaped public
  surface for both backends.

## Context

PR #73 added an opt-in Zenoh backend via compile-time features (ADR-0002) and
introduced owned types such as `qos::QosProfile` (ADR-0004). To minimise
immediate churn, the DDS path was left largely unchanged: `create_topic` /
`create_publisher` / services / actions still take `rustdds::QosPolicies`, and
`ros2::` still re-exports RustDDS QoS, timestamps, and error types.

The Zenoh path already uses `QosProfile` and cannot depend on `rustdds` for
public names. The result is **two parallel public APIs** that share crate-root
names (`Context`, `Node`, `Publisher`, …) but differ in argument types, error
types, service generics, `NodeOptions`, and `MessageInfo`.

Using RustDDS types in `ros2-client`’s public API is an accident of history.
ROS-specific abstractions for both backends are an improvement, but moving the
DDS path onto them is a **breaking change**. Leaking RustDDS (or Zenoh) types
into the stable public surface should be avoided long-term.

ADR-0002’s **direction** is that a single build may eventually enable **both**
`dds` and `zenoh`. That makes shared owned types mandatory, and requires
entity types to be disambiguated (modules or similar) rather than assuming
mutual exclusion forever.

This record captures the agreed convergence plan.

## Decision

**End state:** one ROS-shaped vocabulary for QoS, time, errors, message
metadata, and **discovery info/events** (ADR-0005). Backend entity stacks
(`Context` / `Node` / endpoints) are adapters behind features; when both
features are enabled they remain reachable without type collisions (e.g.
`ros2_client::dds::…` / `ros2_client::zenoh::…`).

Compile-time feature gating (ADR-0002) remains for optional deps and code.
Convergence is about **type identity** of the shared surface, not requiring a
runtime `dyn Middleware` for MVP. Encoding helpers should allow future **XCDR2**
when `cdr-encoding` supports it (ADR-0003); MVP stays XCDR1.

Backend-only escape hatches belong in explicit modules, not on the main path.
Crate-root `pub use rustdds` should eventually move or drop.

### Leak inventory (current DDS public surface)

In priority order for unblocking a shared API:

1. **QoS** — Phase 1 moved create APIs to `QosProfile`; residual
   `ros2::QosPolicies` re-exports until Phase 5.
2. **Errors** — addressed in Phase 3 (owned `Create`/`Read`/`Write`/`Wait`/
   `Service` errors); residual: private helpers may still use middleware
   Results internally.
3. **Time / identity** — largely addressed in Phase 2 (`Log`/`ParameterEvent`
   stamps, owned `MessageInfo` / `RmwRequestId` / `Gid`). Residual: any remaining
   `Timestamp` / `GUID` in DDS-only helpers.
4. **Discovery / events / info** — addressed in Phase 2 (`GraphEvent`,
   `DiscoveredTopic`); residual raw escape `discovered_topics_raw()`.
5. **Escape hatches** — `domain_participant()`, `from_domain_participant()`,
   crate-root `pub use rustdds`.
6. **Service plumbing** — `Service` / `AService` / `ServiceMapping` (DDS RPC
   mapping); Zenoh prefers plain `Req`/`Resp`.

`Gid` and `ROSTime` are already closer to the desired shape; keep extending that
pattern.

### Phased implementation

**Phase 0 — Policy**

- Public API = ROS abstractions; middleware types are private adapters.
- Semver: treat signature changes as a **breaking** major (or clearly announced
  0.x break).
- Interim feature exclusivity (ADR-0002) is OK for MVP; design owned types and
  module layout so `{dds,zenoh}` builds can be enabled later without another
  rewrite.
- Keep Zenoh experimental on branch `zenoh` until shared surface progress
  justifies promotion—not merely Zenoh feature completeness.

**Phase 1 — QoS (highest leverage)** — **done on branch `zenoh` (2026-08-05)**

- DDS `create_topic` / `create_publisher` / `create_subscription` / client /
  server / action QoS take `&QosProfile` / `Option<QosProfile>` (same as Zenoh).
- Convert at the boundary: `QosPolicies::from(&profile)` inside the DDS backend.
- `DEFAULT_SUBSCRIPTION_QOS` / `DEFAULT_PUBLISHER_QOS` are `QosProfile` consts
  (aliases of `subscription_default()` / `publisher_default()`).
- In-tree examples and tests build `QosProfile` instead of `QosPolicyBuilder`.
- Escape hatch: `QosProfile::from(&QosPolicies)` remains; `ros2::QosPolicies`
  re-exports stay until Phase 5.
- DDS-only knobs (`max_blocking_time`, `Ownership`, …): fixed defaults in the
  adapter — **not** fields on `QosProfile`.

**Phase 2 — Metadata, IDs, and discovery** — **done on branch `zenoh` (2026-08-05)**

- Shared owned `MessageInfo` (`source_timestamp` / `received_timestamp` as
  `Option<ROSTime>`, `publisher_gid: Gid`, `sequence_number`, optional
  `related_request_id`). DDS maps from `SampleInfo` / cache changes; Zenoh from
  attachments.
- Backend-neutral `Gid` (16-byte Zenoh GIDs via `From<[u8; 16]>` /
  `to_bytes16()`; distro-gated 16 vs 24 length preserved).
- One `RmwRequestId { writer_gid: Gid, sequence_number: i64 }` for both backends.
- Shared `Log` / `raw::ParameterEvent` use `stamp: builtin_interfaces::Time`
  (ROS IDL field name); DDS `rosout!` no longer stamps with `rustdds::Timestamp`.
- Shared `graph::{GraphEvent, GraphEntity, EntityKind, DiscoveredTopic}`;
  `NodeEvent::DDS(...)` removed — status stream uses `NodeEvent::Graph(...)`
  (plus `ParticipantEntities` on DDS). `Context::discovered_topics()` returns
  owned summaries; raw DDS data via `discovered_topics_raw()`.

**Phase 3 — Errors** — **done on branch `zenoh` (2026-08-05)**

- Owned `CreateError` / `ReadError` / `WriteError<D>` / `WaitError` /
  `ServiceError` in [`src/error.rs`](../../src/error.rs), with
  `CreateResult` / `ReadResult` / `WriteResult` / `WaitResult` /
  `ServiceResult` aliases.
- Semantic variants for portable matching; unclassified backend failures as
  `Middleware { reason }`. DDS/Zenoh map via `From` at the adapter boundary.
- Public APIs return these owned Results — not `zenoh::Result` or
  `rustdds::dds::*` error types. `ros2::{Create,Read,Write,Wait}Error` now
  re-export the owned types.

**Phase 4 — Entity API parity** — **done for the scoped items below on branch
`zenoh`**

| Topic | Convergence target | Status |
| ----- | ------------------ | ------ |
| `NodeOptions` | One options vocabulary; Zenoh no-ops or documents unsupported fields | **Done** — moved to always-compiled [`src/node_options.rs`](../../src/node_options.rs); both backends' `Node::new` consume the same builder. Zenoh now honors `enable_rosout` / `read_rosout` / `start_parameter_services` (default on) / `declare_parameter`, wiring up an optional `Logger` / rosout `Subscription` / `ParameterServer` at construction; unsupported fields (`cli_args`, `use_global_arguments`, `parameter_validator`, `parameter_set_action`) are no-ops logged once at `debug`. `Context::new_node` / `Node::new` are now fallible (`CreateResult<Node>`) on Zenoh, since this wiring can fail. |
| Parameters / rosout | Same methods on the node API for both | **Done** — Zenoh `Node::logger()` / `rosout_subscription()` / `parameter_server()` getters expose the options-created instances (avoiding a double-create); `create_logger` / `read_rosout` / `create_parameter_server` remain for creating additional, independent instances. DDS gets a `Node::create_logger()` alias for `logging_handle()` (naming parity). |
| Discovery waits | `wait_for_*`, counts, graph stream — same owned types on both backends | **Done, DDS best-effort** — DDS `Node` gained `wait_for_publisher`/`wait_for_subscription`/`publisher_count`/`subscription_count`/`graph_event_stream`, matching the Zenoh `Node`/`Context` API shape. DDS counts/waits match by (mangled) topic name via `DomainParticipant::discovered_writers`/`discovered_readers` rather than the exact GUID-keyed maps used internally by `Publisher`/`Subscription`; only absolute topic names are recognized (see doc comments on those methods for the caveats). Zenoh `Node` gained `graph_event_stream`/`publisher_count`/`subscription_count` forwarding to `Context`. |
| Async | Same stream / async method names; internals differ | **Done** — Zenoh `Subscription::async_stream()` added (built from `async_take` in a loop), matching the DDS `Subscription::async_stream()` signature/semantics (a `FusedStream`). |
| Pub/sub helpers | Same count/wait helpers and portable GID on `Publisher`/`Subscription` | **Done** — Zenoh `Publisher::gid()` now returns the portable [`Gid`](../../src/gid.rs) (was a raw `[u8; 16]`); `Publisher`/`Subscription` gained `get_subscription_count`/`wait_for_subscription` and `get_publisher_count`/`wait_for_publisher` taking `&Node`, mirroring [`src/pubsub.rs`](../../src/pubsub.rs). |
| Services | Prefer generic `Req`/`Resp` + serde; keep `Service`/`AService` as thin aliases or DDS-only helpers | **Deferred to Phase 5** — explicit non-goal for this slice (see plan below); DDS keeps `create_client<S: Service>` / `Server<S>`, Zenoh keeps `create_client<Req, Resp>` / `Server<Req, Resp>`. This is the main remaining Phase 4 divergence. |
| Dual-backend builds | Entity types namespaced (or equivalent) so both stacks coexist | **Deferred to Phase 5** — no change to the `compile_error!` / feature-exclusivity in this slice. |

**Phase 5 — Escape hatches and re-exports**

- Move `pub use rustdds` → `ros2_client::dds::rustdds` (or drop).
- `domain_participant()` / `from_domain_participant()` → `dds`-only module.
- Symmetric Zenoh: `session()` behind a `zenoh`-only module.
- Lift interim “exactly one backend” `compile_error!` once namespacing and CI
  cover `{dds,zenoh}` (ADR-0002).

### Migration posture

1. Prefer doing **Phase 1 on the DDS path** (even before treating Zenoh as
   non-experimental) so both backends share QoS shapes.
2. Ship a short migration guide: `QosPolicyBuilder` → `QosProfile`; `Timestamp` →
   `ROSTime`; `GUID` → `Gid`; discovery events → owned graph types.
3. Bump major (or a clear 0.x minor) with release notes; do not pretend default
   features keep every dependent compiling if signatures change.
4. Do **not** rely on docs alone (“almost the same API”) while types diverge.
5. Do **not** put Zenoh `unstable` APIs or RustDDS discovery types in the stable
   public surface.
6. Do **not** treat feature mutual exclusion as permanent product policy.

### Recommended sequence (summary)

`QosProfile` on DDS `create_*` → owned `MessageInfo` / time / `Gid` → owned
discovery events/info → owned errors → demote `pub use rustdds` → allow
`{dds,zenoh}` builds → declare APIs converged → promote Zenoh out of
experimental (e.g. `zenoh` → `master` when ready).

## Consequences

- **Pro:** one portable programming model; Zenoh stops being a second dialect;
  dual-backend apps become possible; dependents are insulated from middleware
  crate upgrades; matches ROS mental model (`rmw_qos_profile_t`-like QoS).
- **Con:** deliberate breaking change for DDS users who construct
  `QosPolicies` / use `ros2::Timestamp` / depend on `pub use rustdds`; more
  adapter code; temporary dual surface and possible interim feature exclusivity
  until phases complete.
- Work continues on branch `zenoh` until promotion to `master` is justified by
  API convergence progress, not merely by feature completeness of the Zenoh
  backend.

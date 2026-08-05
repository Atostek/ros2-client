# Refactoring & implementation plan

Derived from [`feature_map.md`](feature_map.md). Goal: add a Zenoh backend to
`ros2-client` behind a `zenoh` cargo feature, **mutually exclusive** with the
default `dds` feature, with **minimal** disruption to the existing DDS code and
public API (per issue [#71](https://github.com/Atostek/ros2-client/issues/71)
and the user's explicit "keep it minimal" constraint).

## Guiding principles

1. **Minimal churn.** Prefer additive changes and `#[cfg(feature=...)]` seams
   over sweeping renames. Where a public type must stop being a RustDDS type,
   introduce an owned type and keep the old `ros2::` name as an alias/re-export
   so downstream code keeps compiling under `dds`.
2. **Interop with `rmw_zenoh` is the definition of done** for each feature —
   verified against real ROS 2 (see [`test_plan.md`](test_plan.md)), not just
   `ros2-client`↔`ros2-client`.
3. **CDR and naming are shared**; do not fork them.
4. **Async-first for Zenoh.** `mio`/sync polling stays a `dds`-only capability.
5. **One session per context**, mirroring `rmw_zenoh`.

## Two candidate architectures, and the choice

**A. Trait-object backend abstraction** — define `trait Middleware` with
associated `Publisher`/`Subscription`/`Client`/... and make `Context`/`Node`
generic. *Rejected for MVP:* it forces generics through the entire public API
(huge churn), fighting the "minimal" constraint, and the two backends are never
used together (mutually exclusive features), so runtime polymorphism buys
nothing.

**B. Compile-time backend selection (`#[cfg(feature)]`)** — one public API;
backend-specific *internals* selected by feature; a small set of owned public
types (QoS, timestamp, errors, events, Gid provenance) shared by both. **Chosen**
(ADR-0002). It matches the mutually-exclusive-feature model, keeps the DDS path
byte-for-byte unchanged, and confines Zenoh code to new modules + `#[cfg]`
branches in `context.rs`/`node.rs`/`pubsub.rs`/`service`.

Concretely, the internals become:

```
Context ── #[cfg(dds)]  ─→ ContextInner { DomainParticipant, … }   (unchanged)
        └─ #[cfg(zenoh)]─→ ContextInner { zenoh::Session, GraphCache, … }

Publisher<M>    ── #[cfg(dds)] DataWriterCdr<M>
                └─ #[cfg(zenoh)] { session, key_expr, seq, gid }

Subscription<M> ── #[cfg(dds)] SimpleDataReaderCdr<M>
                └─ #[cfg(zenoh)] { subscriber stream, PhantomData<M> }

Client/Server   ── #[cfg(dds)] DataWriter/Reader + wrappers
                └─ #[cfg(zenoh)] querier / queryable + pending map
```

Shared owned modules (compile in both, behaviour differs by cfg):
`qos`, `time`, `error`, `graph` (NodeEvent), `gid`, plus new zenoh-only modules
`zenoh_backend/{session,keyexpr,liveliness,attachment,type_hash,graph_cache}`.

---

## Dependency graph

```
          ┌─────────────────────────────────────────────┐
          │ E0  Feature scaffolding (dds default,        │
          │     zenoh opt-in, mutually exclusive, CI)    │
          └───────────────┬─────────────────────────────┘
                          │
     ┌────────────────────┼───────────────────────────────┐
     ▼                    ▼                                 ▼
┌─────────┐        ┌────────────┐                    ┌────────────┐
│ E1 Owned│        │ E2 CDR +   │                    │ E3 Zenoh   │
│  public │        │  keyexpr   │                    │  session/  │
│  types  │        │  + attach  │                    │  context + │
│ (qos,   │        │  + type-   │                    │  config    │
│  time,  │        │  hash +    │                    │            │
│  error, │        │  gid(xxh3) │                    │            │
│  events)│        └─────┬──────┘                    └─────┬──────┘
└────┬────┘              │                                 │
     │        ┌──────────┴───────────┐                     │
     └────────┤                      │                     │
              ▼                      ▼                     ▼
        ┌───────────┐        ┌───────────────┐      (E3 also feeds
        │ E4 Pub/Sub│        │ E5 Discovery  │       E4/E5/E6)
        │ over Zenoh│◀───────│  liveliness + │
        │ (+xtl)    │  graph │  graph cache  │
        └─────┬─────┘  cache │  (wait_for_*, │
              │              │   counts)     │
              │              └───────┬───────┘
              ▼                      │
        ┌───────────┐               │
        │ E6 Services│◀─────────────┘  (SC/SS liveliness)
        │ queryable/ │
        │ get        │
        └─────┬──────┘
              ├───────────────┬──────────────────┐
              ▼               ▼                  ▼
        ┌──────────┐   ┌────────────┐     ┌────────────┐
        │ E7 Actions│  │ E8 Params  │     │ E9 rosout  │
        └──────────┘   └────────────┘     └────────────┘

  Cross-cutting, runs alongside: T* test/CI tasks (see test_plan.md),
  D* decision records (docs/decisions).
```

Edges = "blocked by". E1/E2/E3 depend only on E0 and can proceed in parallel.
E4 needs E1+E2+E3. E5 needs E2 (keyexpr/liveliness/gid)+E3. E6 needs E4-ish
infra + E5 (client/server liveliness tokens). E7/E8/E9 need E6 (and E4).

---

## Work items (become GitHub issues)

Each item lists: scope, key files, interop acceptance (the `rmw_zenoh` smoke
test that defines done), and blocked-by.

### E0 — Feature scaffolding: `dds` (default) / `zenoh` (opt-in, exclusive)
- **Scope:** add `dds` and `zenoh` features; `default=["dds"]`; a
  `compile_error!` if both or neither is enabled; gate all `rustdds` deps/uses
  behind `dds`; add `zenoh`/`zenoh-ext` as `zenoh`-only optional deps; ensure
  `cargo build`/`clippy`/`doc` pass in **both** configurations; add a CI matrix
  entry building/checking the `zenoh` feature (no interop yet).
- **Files:** `Cargo.toml`, `src/lib.rs`, `.github/workflows/*`.
- **Done when:** `cargo check --no-default-features --features dds` and
  `--features zenoh` both compile (zenoh path may be stubs/`todo!()` behind a
  clear "unimplemented" boundary that still builds).
- **Blocked by:** —

### E1 — Owned public types (decouple from RustDDS)
- **Scope:** introduce owned `qos::QosProfile`, `time` timestamp,
  `error` enums, and a backend-neutral graph `NodeEvent`. Under `dds`, provide
  `From`/`Into` to RustDDS equivalents and keep `ros2::` aliases so existing
  examples/tests compile unchanged. Deprecate `NodeEvent::DDS` (keep under
  `dds`). Provide `DEFAULT_PUBLISHER_QOS`/`DEFAULT_SUBSCRIPTION_QOS` as
  `QosProfile`.
- **Files:** new `src/qos.rs`, `src/time.rs` (or extend `ros_time`),
  `src/error.rs`, `src/graph.rs`; touch `lib.rs`, `pubsub.rs`, `node.rs`,
  `message_info.rs`, `builtin_topics.rs`.
- **Done when:** DDS build + all existing tests pass using the owned types
  (behaviourally identical); public API no longer *requires* naming a
  `rustdds::` type for common pub/sub/service use.
- **Blocked by:** E0.

### E2 — Zenoh wire primitives: keyexpr, attachment, type hash, gid, CDR helper
- **Scope (zenoh-only module, unit-testable without a network):**
  - `keyexpr`: data key `<domain>/<name>/<type>/<hash>` + liveliness key builder/
    parser with `%` mangling + compact `<qos>` encode/decode (delta-vs-default).
  - `attachment`: `(i64 seq, i64 ts_ns, [u8;16] gid)` via `zenoh_ext::z_serialize`/
    `z_deserialize`; round-trip + byte-layout test.
  - `type_hash`: known-types table (RIHS01 for interop types) + `**` wildcard
    strategy for receivers; explicit-hash override API.
  - `gid`: XXH3-128 of the liveliness key (low64‖high64, LE).
  - CDR encapsulation-header helper shared with E4.
- **Files:** `src/zenoh_backend/{keyexpr,attachment,type_hash,gid}.rs`;
  `Cargo.toml` (`xxhash-rust`).
- **Done when:** unit tests reproduce the exact strings/bytes from
  [`research/rmw_zenoh.md`](research/rmw_zenoh.md) §2–§7 (e.g. the `0/chatter/...`
  key, the `::,7:,:,:,,` QoS string, the attachment byte layout, a known GID).
- **Blocked by:** E0 (uses E1 `QosProfile` for QoS encoding — soft dep).

### E3 — Zenoh session / Context + config
- **Scope:** `ContextInner` (zenoh) opens a `zenoh::Session` from JSON5 config
  (honour `ZENOH_SESSION_CONFIG_URI`, `ZENOH_CONFIG_OVERRIDE`, sensible peer
  defaults matching `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5`), holds the async
  runtime, and exposes creation hooks. `ContextOptions` gains zenoh config knobs;
  `domain_id` retained (key prefix). Document the `zenohd` router requirement.
- **Files:** `src/context.rs` (`#[cfg]`), new `src/zenoh_backend/session.rs`,
  bundled default config JSON5.
- **Done when:** a context opens a session against a running `zenohd` in the test
  env; graceful behaviour if no router (log + retry, like rmw_zenoh).
- **Blocked by:** E0.

### E4 — Pub/Sub over Zenoh
- **Scope:** `Publisher<M>`/`Subscription<M>` zenoh internals; volatile via
  `declare_publisher`+`put`(attachment) / `declare_subscriber` stream; transient-
  local via `zenoh-ext` advanced pub/sub cache+history; `MessageInfo` from
  attachment; per-publisher seq; congestion control for keep_all+reliable.
  Publishers/subscribers declare their liveliness tokens (from E5) and the entity
  gid.
- **Files:** `src/pubsub.rs` (`#[cfg]`), `src/zenoh_backend/pubsub.rs`.
- **Interop acceptance:** `ros2 topic echo /chatter` receives a `ros2-client`
  `std_msgs/String` publish; a `ros2-client` subscription receives `ros2 topic
  pub`; `ros2 topic list` shows the topic.
- **Blocked by:** E1, E2, E3.

### E5 — Discovery: liveliness tokens + graph cache
- **Scope:** declare a liveliness token per entity (node/pub/sub/service/client);
  a graph-cache liveliness subscriber on `@ros2_lv/<domain>/**` + initial
  `liveliness().get()`; maintain a cache of remote entities; reimplement
  `wait_for_reader/writer` and `get_publisher/subscription_count` over the cache
  (match by name+type, hash liberal); backend-neutral `NodeEvent` emission.
- **Files:** `src/node.rs` (`#[cfg]` spinner/graph), new
  `src/zenoh_backend/{liveliness,graph_cache}.rs`.
- **Interop acceptance:** `ros2 node list` shows a `ros2-client` node; `ros2-client`
  sees a C++ talker's node/topic via its graph API; `wait_for_subscription`
  resolves when `ros2 topic echo` starts.
- **Blocked by:** E2, E3.

### E6 — Services over queryable / get
- **Scope:** `Server` = queryable(`complete=true`) + pending-query map keyed by
  `hash(client_gid)→seq`, reply echoes seq+gid; `Client` = querier/`get`
  (`ALL_COMPLETE`, `consolidation=None`) with per-client seq and reply channels;
  `RmwRequestId` provenance change; `ServiceMapping` becomes a no-op under zenoh;
  SC/SS liveliness tokens.
- **Files:** `src/service/{client,server,mod}.rs` (`#[cfg]`), new
  `src/zenoh_backend/service.rs`.
- **Interop acceptance:** `ros2 service call /add_two_ints
  example_interfaces/srv/AddTwoInts` answered by a `ros2-client` server; a
  `ros2-client` client calls a C++ `add_two_ints` server and gets the sum;
  `ros2 service list` shows it.
- **Blocked by:** E4, E5.

### E7 — Actions over Zenoh
- **Scope:** verify actions work purely via E6 services + E4 topics; add the
  `**/_action/get_result/**` long-timeout heuristic on the client querier.
- **Interop acceptance:** `ros2 action send_goal /fibonacci
  action_tutorials_interfaces/action/Fibonacci "{order: 5}" --feedback`
  against a `ros2-client` action server (and the reverse).
- **Blocked by:** E6.

### E8 — Parameters over Zenoh
- **Scope:** parameter services (E6) + `parameter_events` topic (E4) with owned
  timestamp; verify against `ros2 param list/get/set`.
- **Interop acceptance:** `ros2 param set/get` on a `ros2-client` node.
- **Blocked by:** E6.

### E9 — rosout logging over Zenoh
- **Scope:** `rt/rosout`→Zenoh key publisher with owned timestamp; optional
  inbound subscription.
- **Interop acceptance:** `ros2 topic echo /rosout` shows a `ros2-client` log.
- **Blocked by:** E4.

### E10 — Documentation, examples, README, feature-status update
- **Scope:** update README (Zenoh feature, router requirement, interop matrix),
  make examples build under `zenoh`, finalise decision records, migration notes.
- **Blocked by:** E4–E9 (incremental).

---

## Sequencing for delivery

1. **E0** (unblocks everything; small, mergeable immediately).
2. **E1**, **E2**, **E3** in parallel (independent).
3. **E4** then **E5** (E5 can start on E2/E3; E4 consumes E5's liveliness/gid — in
   practice land E5's token/gid plumbing first, then E4's data plane, then E5's
   graph-cache consumers).
4. **E6**, then **E7/E8/E9** in parallel.
5. **E10** trails each feature.

## Known risks / open questions (tracked in decision records)

- **Type hash (E2/ADR-0007):** correct RIHS01 needed for send-direction interop
  with C++ peers; MVP uses wildcard-receive + known-types table. Full IDL hashing
  is future work.
- **Attachment/GID byte parity (E2/ADR-0008):** `zenoh_ext::z_serialize` framing
  and XXH3-128 byte order must match `rmw_zenoh` exactly; verified by unit tests +
  live interop.
- **Router requirement (E3/ADR-0009):** tests/CI must launch `zenohd` (or enable
  multicast scouting) — impacts CI runtime and hermeticity.
- **QoS default profile parity (E1/E2):** the delta-vs-default encoding only
  interoperates if our canonical default equals `rmw_zenoh`'s.
- **`cdr-encoding` encapsulation header:** verify header presence/duplication in
  the first pub/sub round-trip.

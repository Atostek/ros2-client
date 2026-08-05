# Feature map: `ros2-client` ↔ ROS 2 ↔ Zenoh

This is the technical heart of the study. For every feature of `ros2-client`
it records: the current DDS-based implementation, the corresponding ROS 2
concept, the `rmw_zenoh` (ground-truth) realisation over Zenoh, the **mismatch**
that a Zenoh backend must resolve, and the **refactor** it implies.

Sources: [`research/ros2_client_internals.md`](research/ros2_client_internals.md),
[`research/rmw_zenoh.md`](research/rmw_zenoh.md),
[`research/zenoh_api.md`](research/zenoh_api.md).

Legend for effort/risk: 🟢 mechanical · 🟡 moderate · 🔴 hard / interop-critical.

---

## 0. The shape of the problem

`ros2-client` is a thin, generic layer: the user brings a serde-serialisable
Rust type `M`, and the crate handles CDR encoding, DDS entity creation, naming,
discovery, services, actions, parameters, and logging. RustDDS types leak into
the public API in a handful of well-defined places (QoS, `Timestamp`, `GUID`/
`Gid`, `RmwRequestId`, `MessageInfo`, `NodeEvent`, error types). The data wire
format (CDR) and the ROS naming convention are **shared** between DDS and Zenoh
— those port directly. What does *not* port is everything RTPS-specific:
GUID-based identity, `SampleIdentity`/inline-QoS RPC correlation, DDS discovery
(both SEDP matching and the `ros_discovery_info` topic), and the DDS QoS model.

`rmw_zenoh` shows the target: one Zenoh **session** per context; topics are
Zenoh **key expressions** `<domain>/<name>/<type>/<hash>`; discovery is
**liveliness tokens** under `@ros2_lv/**`; every message carries an
**attachment** `(seq, timestamp, gid)`; services are **queryable + get**; GIDs
are XXH3-128 hashes of the liveliness key. CDR payloads are byte-identical.

---

## 1. Serialization (CDR) — 🟢 reuse

| | |
|---|---|
| **ros2-client today** | RustDDS `no_key::DataWriterCdr`/`SimpleDataReaderCdr` + serde; `RepresentationIdentifier::CDR_LE`; a 4-byte CDR encapsulation header prepended by RustDDS. |
| **ROS 2** | OMG CDR (XCDR1) with 4-byte encapsulation header (`00 01 00 00` = CDR_LE). |
| **rmw_zenoh** | Identical CDR bytes (incl. the 4-byte encapsulation header) are the Zenoh payload. |
| **Mismatch** | The CDR machinery is currently reached *through* RustDDS writer/reader adapters. A Zenoh backend has no writer/reader. |
| **Refactor** | Serialise `M` to `Vec<u8>` with the standalone [`cdr-encoding`](https://lib.rs/crates/cdr-encoding) crate (`to_vec::<M, LittleEndian>()` / `from_bytes`), prepend the 4-byte encapsulation header, and hand the bytes to `session.put(...)`. `cdr-encoding` is already a transitive dependency and is authored by the RustDDS author, so byte-for-byte parity is expected. Encapsulation-header handling becomes a tiny shared helper. |

> **Important detail:** confirm whether `cdr-encoding::to_vec` emits the 4-byte
> encapsulation header or only the body. RustDDS adds it separately
> (`to_writer_with_rep_id`). The Zenoh backend must ensure the header is present
> exactly once. This is verified in the first pub/sub implementation issue.

---

## 2. Naming — 🟢 reuse, adapt to key expressions

| | |
|---|---|
| **ros2-client today** | `src/names.rs`: `to_dds_name("rt"/"rq"/"rr", node, suffix)` → `rt/ns/topic`; `dds_msg_type()` → `pkg::msg::dds_::Type_`; `_Request_`/`_Response_`/`_Reply` suffixes. Pure string ops. |
| **ROS 2** | [Topic and Service name mapping to DDS](https://design.ros2.org/articles/topic_and_service_names.html). |
| **rmw_zenoh** | Data key: `<domain>/<fqn_no_slashes>/<type_name>/<type_hash>` (real slashes kept). Liveliness key: name **mangled** `/`→`%`, empty→`%`. Service uses same scheme with `srv` type. |
| **Mismatch** | (a) Zenoh keys prepend `<domain_id>` and append `<type_name>/<type_hash>` — the `rt/`/`rq/`/`rr/` prefix is **not** used in the data key (queryable vs subscriber distinguishes req/resp; the *name* is the bare `/add_two_ints`). (b) Liveliness keys use `%` mangling. (c) A **type hash** is required (see §11). |
| **Refactor** | Add Zenoh key-expression builders alongside `to_dds_name`. Keep `names.rs` string algorithms; add: `zenoh_topic_keyexpr(domain, name, type, hash)`, `zenoh_liveliness_keyexpr(...)` with `%` mangling, and the DDS type-name reuse (`dds_msg_type()` is exactly `<type_name>`). |

---

## 3. Context / Session — 🟡

| | |
|---|---|
| **ros2-client today** | `Context` wraps one RustDDS `DomainParticipant` (+ default pub/sub, discovery/rosout/param topics). `Context::new()` / `with_options(domain_id, security)`. |
| **ROS 2** | One context ↔ one middleware participant. |
| **rmw_zenoh** | One context ↔ one `zenoh::Session` (peer mode), plus a graph-cache liveliness subscriber and an initial liveliness get. Config from JSON5 (`ZENOH_SESSION_CONFIG_URI`, `ZENOH_CONFIG_OVERRIDE`); **needs a `zenohd` router** by default. |
| **Mismatch** | `DomainParticipant` ↔ `Session` are different lifecycles; Zenoh needs an async runtime and a live router; `ContextOptions` (domain_id, DDS security) differ from Zenoh options (config path/overrides, connect/listen endpoints, router-check attempts). |
| **Refactor** | `ContextInner` becomes backend-specific: `#[cfg(feature="dds")]` keeps the participant; `#[cfg(feature="zenoh")]` owns a `Session` + graph cache. `ContextOptions` gains a zenoh-only `zenoh_config`/overrides path; `domain_id` stays (Zenoh uses it as key prefix, not transport). See [refactoring_plan.md](refactoring_plan.md). |

---

## 4. Publish / Subscribe — 🟡

| | |
|---|---|
| **ros2-client today** | `Publisher<M>` wraps `DataWriterCdr<M>`; `Subscription<M>` wraps `SimpleDataReaderCdr<M>`; async stream via RustDDS; `MessageInfo` from `SampleInfo`. QoS = `QosPolicies`. |
| **ROS 2** | Publisher/Subscription with QoS. |
| **rmw_zenoh** | Volatile: `declare_publisher`/`declare_subscriber` + `put` with attachment `(seq, ts, gid)`. `TRANSIENT_LOCAL`: `zenoh-ext` `AdvancedPublisher` (publication cache) + `AdvancedSubscriber` (history query). `KEEP_ALL`+`RELIABLE` → congestion-control `BLOCK`. |
| **Mismatch** | No DataWriter/Reader; async by construction (Zenoh subscriber callback/stream on Zenoh runtime threads); `MessageInfo` fields (seq, source ts, publisher gid) come from the **attachment** not `SampleInfo`; `depth==0`→42; transient-local needs `zenoh-ext` + session timestamping. |
| **Refactor** | Backend-specific `Publisher`/`Subscription` internals; a shared `MessageInfo` populated from attachments in the Zenoh path. Publisher assigns a per-publisher monotonic sequence number. Map QoS profile → (volatile vs advanced, congestion control). |

---

## 5. QoS — 🔴 public-API decoupling

| | |
|---|---|
| **ros2-client today** | `rustdds::QosPolicies` / `QosPolicyBuilder` / `policy::*` in the public API of `create_topic/publisher/subscription/client/server`, action QoS structs, and re-exported via `ros2::`. |
| **ROS 2** | `rmw_qos_profile_t`: reliability, durability, history, depth, deadline, lifespan, liveliness, lease. |
| **rmw_zenoh** | QoS behaviourally emulated (reliability→transport, transient_local→cache, keep_all→BLOCK, deadline/lifespan carried-not-enforced, liveliness AUTOMATIC only) **and** delta-encoded into the liveliness keyexpr `<qos>` field against a canonical default. |
| **Mismatch** | The DDS QoS type has knobs Zenoh ignores (`max_blocking_time`, `Ownership`) and a different default matrix. Cannot pull `rustdds` under the `zenoh` feature just for the type. |
| **Refactor** | Introduce an **owned** `ros2_client::qos::QosProfile` (the ROS 2 profile: reliability/durability/history/depth/deadline/lifespan/liveliness/lease). Under `dds`, `From`/`Into` `rustdds::QosPolicies`. Under `zenoh`, drive publisher/subscriber options + the `<qos>` keyexpr encoding. Keep `ros2::QosPolicies` as a compat alias where feasible to minimise churn (see ADR-0004). This is the single largest public-API change. |

---

## 6. Discovery / ROS graph — 🔴 mechanism swap

| | |
|---|---|
| **ros2-client today** | Dual: (a) RTPS `DomainParticipantStatusEvent` stream → match maps → `wait_for_reader/writer`, pub/sub counts; (b) `ros_discovery_info` data topic carrying `rmw_dds_common::ParticipantEntitiesInfo` (`Gid` lists) → ROS node/topic graph. `NodeEvent::{DDS,ROS}`. |
| **ROS 2** | Node/topic/service/action graph introspection. |
| **rmw_zenoh** | **Liveliness tokens**: every entity declares a token whose key encodes all metadata (`@ros2_lv/<domain>/<zid>/<nid>/<eid>/<kind>/<enclave>/<ns>/<node>/<name>/<type>/<hash>/<qos>`). A liveliness subscriber on `@ros2_lv/<domain>/**` + an initial `liveliness_get` build/maintain a graph cache. |
| **Mismatch** | Entirely different mechanism. No `ros_discovery_info` topic, no SEDP. Entity counts / `wait_for_*` must be derived from the graph cache (matching by name/type/hash), not GUID matching. `NodeEvent::DDS(...)` leaks a RustDDS type. |
| **Refactor** | Zenoh backend maintains a graph cache from liveliness tokens. `wait_for_reader/writer` and `get_*_count` reimplemented over the cache. Introduce a backend-neutral `NodeEvent`/graph-event type (deprecate `NodeEvent::DDS`). Liveliness key build/parse (+ QoS + mangling + type hash) is a large new module. |

---

## 7. Services — 🔴 correlation redesign

| | |
|---|---|
| **ros2-client today** | Request topic `rq/.../Request` + reply topic `rr/.../Reply`. Three `ServiceMapping`s (Basic/Enhanced/Cyclone). Correlation = `RmwRequestId {GUID, SequenceNumber}` (== `rpc::SampleIdentity`). Enhanced uses RTPS **inline-QoS `related_sample_identity`**. |
| **ROS 2** | Service client/server, request/response correlated by a request id. |
| **rmw_zenoh** | Server = `declare_queryable(key).complete(true)`; client = `declare_querier(key)`/`get` with `target=ALL_COMPLETE`, `consolidation=None`. Request/response carry attachment `(seq, ts, client_gid)`; server keys pending queries by `hash(client_gid)→seq`, echoes seq+gid in the reply; client correlates by the echoed seq. Long timeout for `**/_action/get_result/**`. |
| **Mismatch** | No request/reply *topics*; it's query/reply. No `SampleIdentity`/inline-QoS. `RmwRequestId` semantics change: `writer_guid` becomes the client entity GID (XXH3), `sequence_number` a client-local counter. The three DDS `ServiceMapping`s are irrelevant over Zenoh — Zenoh has exactly one mapping (query/reply + attachment). |
| **Refactor** | A Zenoh service path: `Server` owns a queryable + a pending-query map; `Client` owns a querier + reply channels. Keep the public `Client`/`Server` API and `RmwRequestId` shape (opaque 16-byte id + seq) but change provenance. Under `zenoh`, `ServiceMapping` is ignored (document as a no-op / hidden). This is the most involved single feature. |

---

## 8. Actions — 🟡 (free once services+pubsub work)

| | |
|---|---|
| **ros2-client today** | Composed from 3 services (send_goal, cancel_goal, get_result) + 2 topics (feedback, status). `GoalId` (UUID) correlation is application-level. |
| **rmw_zenoh** | No action concept — pure services+topics. Only quirk: `get_result` querier gets an ~infinite timeout. |
| **Mismatch** | None structural; inherits service/pubsub porting. |
| **Refactor** | Almost none beyond services+pubsub. Add the `get_result` long-timeout heuristic (key intersects `**/_action/get_result/**`). `GoalId` correlation ports unchanged. |

---

## 9. Parameters — 🟢 (once services work)

| | |
|---|---|
| **ros2-client today** | Six `Server<rcl_interfaces::*>` (Enhanced mapping) + `rt/parameter_events` publisher. Enum↔raw conversion is DDS-agnostic. `ParameterEvent.timestamp` is a `rustdds::Timestamp`. |
| **rmw_zenoh** | Ordinary services + a topic. |
| **Mismatch** | Only the `Timestamp` field type and the fact it rides on services. |
| **Refactor** | Replace `rustdds::Timestamp` with an owned timestamp (§10). Otherwise inherits services+pubsub. |

---

## 10. Time — 🟢/🟡

| | |
|---|---|
| **ros2-client today** | `ROSTime`/`SystemTime`/`steady_time`; conversions to/from `rustdds::Timestamp` (RTPS `Time_t`); `Timestamp` re-exported as `ros2::Timestamp` and used in message structs (`Log`, `ParameterEvent`) and `MessageInfo`. |
| **rmw_zenoh** | Source timestamp is `int64` ns since epoch in the attachment. |
| **Mismatch** | `rustdds::Timestamp` is RTPS-specific and re-exported publicly. |
| **Refactor** | Introduce an owned time/timestamp type (or reuse `builtin_interfaces::Time` / `ROSTime`) for message fields and `MessageInfo`. Under `dds`, convert to/from `rustdds::Timestamp`. Under `zenoh`, ns-epoch i64 in attachments. |

---

## 11. Type hash (REP-2016 `RIHS01_...`) — 🔴 interop-critical, new capability

| | |
|---|---|
| **ros2-client today** | **Absent.** The crate never computes a type hash — it's a generic serde client. |
| **ROS 2 / rmw_zenoh** | Keys and liveliness tokens include a REP-2016 RIHS01 type hash (SHA-256 over a canonical JSON type description). Two entities only match if name **and** type **and** hash agree. |
| **Mismatch** | Without the correct hash, a `ros2-client` **publisher's** concrete key will not match a C++ `rmw_zenoh` **subscriber's** concrete key → no interop in the send direction. `ros2-client`↔`ros2-client` works with any agreed placeholder. |
| **Strategy (see ADR-0007)** | Layered: **(a)** subscribers/queryables declare with a `**` wildcard on the hash slot → receive from any hash (fixes the *receive* direction immediately). **(b)** For the *send* direction to C++ peers, provide correct hashes via a small **known-types table** for the common interop types (std_msgs, example_interfaces AddTwoInts/Fibonacci, geometry_msgs Twist, etc.), and let users supply a hash explicitly. **(c)** Longer term, compute RIHS01 from parsed `.msg`/`.srv` IDL in `msggen`. MVP ships (a)+(b). |

---

## 12. GID / entity identity — 🔴

| | |
|---|---|
| **ros2-client today** | `Gid` = padded RustDDS `GUID` (16/24 B, distro-gated); implements RustDDS `Key`/`CdrEncodingSize`; used in `ParticipantEntitiesInfo`, `MessageInfo`, service correlation. |
| **rmw_zenoh** | GID = XXH3-128 of the entity's liveliness keyexpr (low64‖high64, LE); 16 bytes. |
| **Mismatch** | Provenance changes from GUID to a hash; correlation and attachments must use it. `Gid` should stay a 16-byte opaque id but be generated differently under `zenoh`. |
| **Refactor** | Keep the `Gid` type (drop `Key`/`CdrEncodingSize` reliance under `zenoh`); add `zenoh` construction via a stable XXH3-128 (crate [`xxhash-rust`](https://crates.io/crates/xxhash-rust) `xxh3`) over the liveliness key. Verify byte order vs `rmw_zenoh` (ADR-0008). |

---

## 13. Error types — 🟡 public-API decoupling

| | |
|---|---|
| **ros2-client today** | `CreateError`/`ReadError`/`WriteError`/`WaitError` from `rustdds::dds`, re-exported via `ros2::`; `NodeCreateError::DDS(...)`, `GoalError::DDS*`, `CallServiceError`. |
| **Mismatch** | Cannot re-export `rustdds` errors under the `zenoh` feature. |
| **Refactor** | Define owned error enums (`ros2_client::Error`, per-operation variants) with `#[cfg]` inner variants wrapping the backend error (`rustdds::dds::*` vs `zenoh::Error`). Keep `ros2::` names as aliases where possible (ADR-0004). |

---

## 14. `mio::Evented` sync polling — 🟡 drop under zenoh

| | |
|---|---|
| **ros2-client today** | `Subscription`/`Client`/`Server` implement `mio::Evented` by delegating to RustDDS. |
| **Mismatch** | Zenoh has no mio integration; it's async-first. |
| **Refactor** | Under `zenoh`, do **not** implement `mio::Evented`; provide async streams only (the README already recommends async). Document sync/mio polling as a `dds`-only capability. |

---

## 15. Security — 🟡 divergent

| | |
|---|---|
| **ros2-client today** | Optional DDS Security (`security` feature → RustDDS security). |
| **rmw_zenoh** | Security via Zenoh (mTLS/QUIC, access control) configured in the session/router JSON5, not SROS artifacts. |
| **Mismatch** | Completely different models. |
| **Refactor** | Out of scope for MVP. Keep `security` as a `dds`-only feature; document Zenoh transport security as configuration-driven (future work). |

---

## Summary: what ports vs what is rebuilt

**Ports directly (shared):** CDR (via `cdr-encoding`), `names.rs` string algorithms
and type-name mangling, the message/IDL struct definitions, parameter enum↔raw
layer, action `GoalId` correlation, `ROSTime`/duration arithmetic.

**Owned types to introduce (decouple public API):** `QosProfile`, timestamp,
error enums, backend-neutral `NodeEvent`/graph events. `Gid` stays but changes
provenance; `RmwRequestId` stays but changes provenance.

**Rebuilt for Zenoh:** session/context, pub/sub over `put`/`declare_subscriber`
(+ `zenoh-ext` for transient-local), liveliness-token discovery + graph cache,
services over queryable/get, GID via XXH3-128, QoS keyexpr encoding, and a new
type-hash capability.

**Dropped under `zenoh`:** `mio::Evented`, DDS `ServiceMapping` variants, DDS
security, `ros_discovery_info` topic, `DomainParticipantStatusEvent`.

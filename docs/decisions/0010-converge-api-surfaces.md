# 10. Converge DDS and Zenoh public API surfaces

- Status: accepted (directional plan; implementation is phased)
- Date: 2026-08-05
- Relates to: ADR-0002, ADR-0004, PR #73 / branch `zenoh`, issue #71
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

This record captures the agreed convergence plan.

## Decision

**End state:** one public API whose types are ROS-shaped and backend-neutral.
RustDDS and Zenoh are private adapters behind `#[cfg(feature = "dds"|"zenoh")]`.
Compile-time feature selection (ADR-0002) remains; convergence is about **type
identity**, not runtime `dyn Middleware` polymorphism.

Backend-only escape hatches belong in explicit modules (`ros2_client::dds::…`,
`ros2_client::zenoh::…`), not on the main path. Crate-root `pub use rustdds`
should eventually move or drop.

### Leak inventory (current DDS public surface)

In priority order for unblocking a shared API:

1. **QoS** — `QosPolicies` / `QosPolicyBuilder` / `policy::*` on create APIs;
   action QoS structs; `DEFAULT_*_QOS`.
2. **Errors** — `CreateError` / `ReadError` / `WriteError` / `WaitError` (and
   `CreateResult`).
3. **Time / identity** — `Timestamp` in `Log`, `ParameterEvent`, `MessageInfo`;
   `GUID` / `SampleIdentity` in metadata; `RmwRequestId` ≡ DDS sample identity.
4. **Discovery / events** — `NodeEvent::DDS(DomainParticipantStatusEvent)`,
   `discovered_topics() -> DiscoveredTopicData`.
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
- Keep Zenoh experimental on branch `zenoh` until at least Phase 2 has landed on
  the DDS path as well, or accept a documented dual-API window with deprecations.

**Phase 1 — QoS (highest leverage)**

- Make DDS `create_topic` / `create_publisher` / `create_subscription` / client /
  server / action QoS take `&QosProfile` / `Option<QosProfile>` (same as Zenoh).
- Convert at the boundary: `QosPolicies::from(&profile)` inside the DDS backend.
- Replace `DEFAULT_*_QOS: QosPolicies` with `QosProfile::subscription_default()` /
  `publisher_default()`.
- Soften the break if needed for one release: `Into<QosProfile>` bounds,
  deprecated helpers, migration notes — not a permanent dual QoS API.
- DDS-only knobs (`max_blocking_time`, `Ownership`, …): fixed defaults in the
  adapter, or a `dds`-only extension (`DdsQosExt` / escape hatch) — **not**
  fields on the common `QosProfile`. Do not expand `QosProfile` into full DDS QoS.

**Phase 2 — Metadata and IDs**

- Owned `MessageInfo`: times as `ROSTime` / `builtin_interfaces::Time`, publisher
  as `Gid` / `[u8; 16]`, sequence as an integer.
- `Log` / `ParameterEvent` drop `rustdds::Timestamp`.
- One `RmwRequestId { writer_guid: Gid, sequence_number }` for both backends.
- Neutral graph events (`NodeEvent` / `GraphEvent`); deprecate
  `NodeEvent::DDS(...)`.

**Phase 3 — Errors**

- Owned error enums with `#[cfg]`-gated backend variants or `Error::source()` to
  middleware errors.
- Public methods return `Result<T, ros2_client::…Error>`, never `zenoh::Result`
  or RustDDS error types at the boundary.

**Phase 4 — Entity API parity**

| Topic | Convergence target |
| ----- | ------------------ |
| Services | Prefer generic `Req`/`Resp` + serde; keep `Service`/`AService` as thin aliases or DDS-only helpers |
| `NodeOptions` | One struct; Zenoh no-ops or documents unsupported fields |
| Parameters / rosout | Same methods on `Node` for both |
| Async | Same stream / async method names; internals differ |
| Discovery waits | `wait_for_*`, counts, graph stream — match both backends |

**Phase 5 — Escape hatches and re-exports**

- Move `pub use rustdds` → `ros2_client::dds::rustdds` (or drop).
- `domain_participant()` / `from_domain_participant()` → `dds`-only module.
- Symmetric Zenoh: `session()` behind a `zenoh`-only module.

### Migration posture

1. Prefer doing **Phase 1 on the DDS path** (even before treating Zenoh as
   non-experimental) so both backends share QoS shapes.
2. Ship a short migration guide: `QosPolicyBuilder` → `QosProfile`; `Timestamp` →
   `ROSTime`; `GUID` → `Gid`.
3. Bump major (or a clear 0.x minor) with release notes; do not pretend default
   features keep every dependent compiling if signatures change.
4. Do **not** rely on docs alone (“almost the same API”) while types diverge.
5. Do **not** put Zenoh `unstable` APIs or RustDDS discovery types in the stable
   public surface.

### Recommended sequence (summary)

`QosProfile` on DDS `create_*` → owned `MessageInfo` / time / `Gid` → owned
errors → demote `pub use rustdds` → declare APIs converged → promote Zenoh out of
experimental (e.g. `zenoh` → `master` when ready).

## Consequences

- **Pro:** one portable programming model; Zenoh stops being a second dialect;
  dependents are insulated from middleware crate upgrades; matches ROS mental
  model (`rmw_qos_profile_t`-like QoS).
- **Con:** deliberate breaking change for DDS users who construct
  `QosPolicies` / use `ros2::Timestamp` / depend on `pub use rustdds`; more
  adapter code; temporary dual surface until phases complete.
- Work continues on branch `zenoh` until promotion to `master` is justified by
  API convergence progress, not merely by feature completeness of the Zenoh
  backend.

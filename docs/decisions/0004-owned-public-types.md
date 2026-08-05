# 4. Introduce owned public types to decouple from middleware crates

- Status: accepted (end-state / DDS-path migration refined by ADR-0010)
- Date: 2026-07-07
- Updated: 2026-08-05
- See also: ADR-0002, ADR-0005, ADR-0010

## Context

Middleware types appear in `ros2-client`’s public API: RustDDS `QosPolicies` /
`QosPolicyBuilder` / `policy::*`, `Timestamp`, `GUID`/`Gid`, `RmwRequestId`
(≡ `SampleIdentity`), `MessageInfo` fields, `NodeEvent::DDS(...)`, discovery
snapshots, and the `CreateError`/`ReadError`/`WriteError` families — often
re-exported via `ros2::`.

A zenoh-only build cannot name those RustDDS types. Even when **both** backends
are enabled in one build (ADR-0002 direction), the stable public surface should
still be ROS-shaped so apps are not tied to a particular middleware crate.
Several DDS types have no clean Zenoh equivalent and must not remain the
shared vocabulary.

## Decision

Introduce a small set of **owned** types used by both backends:

- `qos::QosProfile` — the ROS 2 QoS profile (reliability, durability, history,
  depth, deadline, lifespan, liveliness, lease). Under `dds`, `From`/`Into`
  `rustdds::QosPolicies`; under `zenoh`, drives pub/sub options and the compact
  `<qos>` liveliness encoding.
- an owned timestamp for message/metadata fields (reusing `builtin_interfaces::
  Time`/`ROSTime` where natural).
- owned `error` enums, wrapping the backend error in a `#[cfg]`-gated variant
  (or `source()`), suitable when one or both backends are linked.
- owned **discovery / graph** event and info types (ADR-0005): backend-neutral
  graph events and snapshots; raw `DomainParticipantStatusEvent` /
  `DiscoveredTopicData` (and Zenoh liveliness samples) stay behind the adapter.

Interim landing may keep `ros2::` re-exports and DDS `create_*` on
`QosPolicies` to limit churn; that is not the end state (ADR-0010).
`RmwRequestId` and `Gid` keep their shape (16-byte id + seq) but change
provenance under Zenoh.

## Consequences

- **Pro:** the public API stops *requiring* a RustDDS (or Zenoh) type for common
  use; both backends—and future dual-backend builds—share one surface; DDS-only
  users can migrate in phases.
- **Con:** a real API evolution — the largest single early change is QoS.
  Conversions add code. Some rarely-used DDS-only knobs (`max_blocking_time`,
  `Ownership`) are not represented in `QosProfile` and are either mapped to
  sensible DDS defaults or exposed only via a `dds` escape hatch.
- This is the crux of "some DDS abstractions won't stand the transition, so we
  use ROS 2 abstractions to replace them."

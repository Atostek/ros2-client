# 4. Introduce owned public types to decouple from RustDDS

- Status: accepted
- Date: 2026-07-07

## Context

RustDDS types appear in `ros2-client`'s public API: `QosPolicies`/
`QosPolicyBuilder`/`policy::*`, `Timestamp`, `GUID`/`Gid`, `RmwRequestId`
(≡ `SampleIdentity`), `MessageInfo` fields, `NodeEvent::DDS(...)`, and the
`CreateError`/`ReadError`/`WriteError` families — all re-exported via `ros2::`.
Under the `zenoh` feature we cannot depend on `rustdds` just to name these types,
and several have no clean Zenoh equivalent.

## Decision

Introduce a small set of **owned** types used by both backends:

- `qos::QosProfile` — the ROS 2 QoS profile (reliability, durability, history,
  depth, deadline, lifespan, liveliness, lease). Under `dds`, `From`/`Into`
  `rustdds::QosPolicies`; under `zenoh`, drives pub/sub options and the compact
  `<qos>` liveliness encoding.
- an owned timestamp for message/metadata fields (reusing `builtin_interfaces::
  Time`/`ROSTime` where natural).
- owned `error` enums, wrapping the backend error in a `#[cfg]`-gated variant.
- a backend-neutral graph `NodeEvent`; `NodeEvent::DDS(...)` remains `dds`-only
  and is deprecated.

To honour the "minimal churn" constraint, keep the `ros2::` names as
aliases/re-exports so existing examples and downstream code compile unchanged on
the default `dds` build. `RmwRequestId` and `Gid` keep their shape (16-byte id +
seq) but change provenance under `zenoh`.

## Consequences

- **Pro:** the public API stops *requiring* a RustDDS type for common use; both
  backends share one surface; DDS users see minimal or no changes.
- **Con:** a real (if contained) API evolution — the largest single change is
  QoS. Conversions add code. Some rarely-used DDS-specific knobs
  (`max_blocking_time`, `Ownership`) are not represented in `QosProfile` and are
  either mapped to sensible DDS defaults or exposed only on the `dds` build.
- This is the crux of "some DDS abstractions won't stand the transition, so we
  use ROS 2 abstractions to replace them" from the task brief.

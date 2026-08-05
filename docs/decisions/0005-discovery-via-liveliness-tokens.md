# 5. Discovery via Zenoh liveliness tokens + graph cache

- Status: accepted
- Date: 2026-07-07

## Context

`ros2-client` discovers the ROS graph two ways: RTPS reader/writer matching
(`DomainParticipantStatusEvent`) driving `wait_for_*` and entity counts, and the
`ros_discovery_info` data topic (`rmw_dds_common::ParticipantEntitiesInfo`)
assembling the node/topic graph. Neither exists over Zenoh.

`rmw_zenoh` instead declares a **liveliness token** per entity whose key encodes
all metadata (`@ros2_lv/<domain>/<zid>/<nid>/<eid>/<kind>/…/<name>/<type>/<hash>/
<qos>`), and builds a **graph cache** from a liveliness subscriber on
`@ros2_lv/<domain>/**` plus an initial `liveliness_get`.

## Decision

Implement Zenoh discovery exactly per `rmw_zenoh`:

- Each entity (node `NN`, publisher `MP`, subscription `MS`, service server `SS`,
  service client `SC`) declares a liveliness token with the ground-truth key
  format (name mangling `/`→`%`, empty→`%`; compact `<qos>` encoding).
- The context declares a liveliness subscriber on `@ros2_lv/<domain>/**` and runs
  an initial `liveliness().get()` to seed a graph cache; PUT/DELETE keep it live.
- `wait_for_reader/writer` and `get_publisher/subscription_count` are
  reimplemented over the cache, matching by name + type (hash treated liberally,
  see ADR-0007), instead of GUID matching.
- A backend-neutral `NodeEvent`/graph-event type is emitted (ADR-0004).

## Consequences

- **Pro:** full interop graph introspection (`ros2 node/topic/service list`),
  matching the official design.
- **Con:** a substantial new module (key build/parse + cache); the semantics of
  "matched count" differ subtly from RTPS matching (cache-based, name/type keyed).
  `NodeEvent::DDS` is deprecated/`dds`-only.
- The `ros_discovery_info` topic and `DomainParticipantStatusEvent` are not used
  under `zenoh`.

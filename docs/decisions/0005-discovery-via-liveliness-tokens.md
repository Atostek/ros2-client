# 5. Discovery via Zenoh liveliness tokens + graph cache

- Status: accepted
- Date: 2026-07-07
- Updated: 2026-08-05
- Relates to: ADR-0004, ADR-0010

## Context

`ros2-client` discovers the ROS graph two ways today on DDS: RTPS reader/writer
matching (`DomainParticipantStatusEvent`) driving `wait_for_*` and entity
counts, and the `ros_discovery_info` data topic
(`rmw_dds_common::ParticipantEntitiesInfo`) assembling the node/topic graph.
Neither exists over Zenoh.

`rmw_zenoh` instead declares a **liveliness token** per entity whose key encodes
all metadata (`@ros2_lv/<domain>/<zid>/<nid>/<eid>/<kind>/…/<name>/<type>/<hash>/
<qos>`), and builds a **graph cache** from a liveliness subscriber on
`@ros2_lv/<domain>/**` plus an initial `liveliness_get`.

Public discovery **info and events** currently leak DDS types
(`NodeEvent::DDS(...)`, `DiscoveredTopicData`, etc.). Those should eventually
be **owned, backend-neutral** types (same motivation as QoS / `MessageInfo` in
ADR-0004 and ADR-0010), whether the process uses DDS, Zenoh, or both
(ADR-0002).

## Decision

### Zenoh wire / cache behaviour

Implement Zenoh discovery exactly per `rmw_zenoh`:

- Each entity (node `NN`, publisher `MP`, subscription `MS`, service server `SS`,
  service client `SC`) declares a liveliness token with the ground-truth key
  format (name mangling `/`→`%`, empty→`%`; compact `<qos>` encoding).
- The context declares a liveliness subscriber on `@ros2_lv/<domain>/**` and runs
  an initial `liveliness().get()` to seed a graph cache; PUT/DELETE keep it live.
- `wait_for_reader/writer` and `get_publisher/subscription_count` are
  reimplemented over the cache, matching by name + type (hash treated liberally,
  see ADR-0007), instead of GUID matching.

### Owned discovery surface (direction)

- Expose graph **events** and **snapshots** (node/topic/service lists, counts,
  match notifications used by `wait_for_*`) as **owned** types shared by both
  backends—not `DomainParticipantStatusEvent`, not raw Zenoh liveliness
  samples.
- Map DDS discovery (participant status + `ros_discovery_info`) and Zenoh
  liveliness/cache updates into that common model at the backend boundary.
- Interim: `NodeEvent::DDS(...)` (or similar) may remain as a deprecated
  escape hatch on DDS-only builds; it is not the end state.
- Harmonization of discovery info/events is part of API convergence
  (ADR-0010 Phase 2 / discovery waits in Phase 4), not Zenoh-only polish.

## Consequences

- **Pro:** full interop graph introspection (`ros2 node/topic/service list`) on
  Zenoh; a path to one discovery API for DDS and Zenoh (and dual-backend
  builds); dependents stop coupling to RustDDS discovery types.
- **Con:** a substantial module (key build/parse + cache); “matched count”
  semantics differ subtly from RTPS matching (cache-based, name/type keyed);
  defining a good owned graph model takes care (ROS graph ≠ raw RTPS
  endpoints).
- The `ros_discovery_info` topic and `DomainParticipantStatusEvent` remain
  DDS-backend implementation details, not public API forever.

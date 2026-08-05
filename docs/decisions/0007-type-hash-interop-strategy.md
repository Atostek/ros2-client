# 7. Type-hash (REP-2016) interop strategy

- Status: accepted
- Date: 2026-07-07
- Relates to: `docs/zenoh_study/feature_map.md` §11

## Context

`rmw_zenoh` key expressions and liveliness tokens include a REP-2016 type hash
(`RIHS01_<64 hex>`), a SHA-256 over a canonical JSON description of the message
type. Two Zenoh entities match only if **name AND type AND hash** agree.
`ros2-client` is a generic serde client and computes **no** type hash today;
computing a correct RIHS01 requires the full IDL type description (all nested
fields), which the crate does not generally have at hand.

Without the correct hash:
- a `ros2-client` **subscriber/queryable** with a concrete-hash key won't receive
  from a C++ publisher that uses the real hash;
- a `ros2-client` **publisher** with a wrong/placeholder hash won't be received by
  a C++ subscriber that subscribes to the real-hash key.

`ros2-client`↔`ros2-client` works with any consistently-used placeholder.

## Decision

A layered strategy, MVP = (a) + (b):

- **(a) Liberal receive.** Subscribers and queryables declare their key with a
  wildcard (`*`/`**`) in the type-hash position, so they receive from **any**
  publisher/client regardless of hash. This fixes the receive direction against
  real ROS 2 immediately and is legal because Zenoh subscriber keys may contain
  wildcards.
- **(b) Known-types hash table.** For the send direction to C++ peers, ship a
  small table of correct RIHS01 hashes for the common interop types
  (`std_msgs/String`, `example_interfaces/srv/AddTwoInts`,
  `action_tutorials_interfaces/action/Fibonacci`, `geometry_msgs/msg/Twist`, …),
  and expose an API to supply a hash explicitly per topic/type. When a type is
  unknown, fall back to a wildcard-or-placeholder and log that send-direction
  interop with C++ peers may not match.
- **(c) Future: compute RIHS01 from parsed `.msg`/`.srv`/`.action` IDL** in
  `msggen`. Tracked as a follow-up issue, not required for MVP.

## Update: RIHS01 computation implemented

The core REP-2016 computation for (c) now exists in
`src/zenoh_backend/type_description.rs`: a backend-neutral, from-scratch
implementation of `rosidl_generator_type_description::calculate_type_hash`
(the `FieldType` id scheme, the canonical hashable JSON — struct-order keys,
`", "`/`": "` separators, `default_value` stripped, referenced types sorted by
name — and `RIHS01_` + SHA-256). It reproduces the published hashes of
`std_msgs/msg/String` and `example_interfaces/srv/AddTwoInts` byte-exactly
(services compose a synthetic top type over `request_message`/`response_message`/
`event_message` plus the transitive closure), and those values are cross-checked
against the known-types table so the two can never silently diverge.

What remains for (c) is the *pipeline* work, not the algorithm: teaching
`msggen` to build a `TypeDescription` for each generated type by resolving the
transitive closure of nested types across packages (and pinning a distro's
builtin definitions, e.g. `service_msgs/ServiceEventInfo`, whose `client_gid`
field type changed between distros and changes the hash), then emitting the
computed hash as a generated constant the Zenoh backend can use directly. Until
then, the send direction still relies on the known-types table for types not
generated with a hash.

## Consequences

- **Pro:** interop works today for the receive direction universally and for the
  send direction on the common types, without building a full IDL hasher.
- **Con:** send-direction interop with C++ peers is limited to known types until
  (c) lands; wildcard-receive slightly weakens type-safety at the transport layer
  (mitigated by the type name still being in the key and CDR deserialization
  failing loudly on a true mismatch).
- The hash table must be kept in sync with upstream type definitions; a test
  compares a few entries against values observed from a live `rmw_zenoh` peer.

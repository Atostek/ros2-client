# Changelog

All notable changes to `ros2-client-multi-rmw`, the `semio-ai/ros2-client` fork
of [`Atostek/ros2-client`](https://github.com/Atostek/ros2-client). The format
follows [Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/). Upstream keeps no changelog, so the
entries start where the fork's own versions do; the library is imported as
`ros2_client` throughout.

## [0.13.0] - 2026-09-10

### Added

- `Node::create_raw_server_with_type_hash` and
  `Node::create_raw_action_server_with_type_hashes`: a raw server keyed on
  explicit REP-2016 type hashes. `rmw_zenoh` keys a service as it keys a topic
  — `<domain>/<name>/<type>/<hash>` — and a native client sends its request to
  the key its generated type computes; a raw server announcing the type-hash
  table's placeholder for a runtime-defined type was listed and never reached.
  `create_raw_server` and `create_raw_action_server` keep their behaviour and
  delegate to the new constructors. The DDS backend accepts and ignores the
  hashes, so a caller compiles against one signature on either backend.

## [0.12.1] - 2026-09-09

### Added

- `Node::create_raw_publisher_with_type_hash`, the dynamic counterpart of
  `create_publisher_with_type_hash`: a raw publisher keyed on the hash the
  caller computes from the type's full description, so a native `rmw_zenoh`
  subscriber listening on the concrete-hash key hears it (`create_raw_publisher`
  announces the all-zero placeholder, which only other ros2-client peers
  match). DDS accepts and ignores the hash.

## [0.12.0] - 2026-07-30

### Added

- `RawSubscription` / `Node::create_raw_subscription` on both backends: each
  message is delivered as a full standalone CDR message (encapsulation header
  included), to be decoded against a type description known only at runtime.
  Interoperates with an ordinary typed publisher of the topic's declared type;
  a raw publisher and a raw subscription round-trip the exact bytes.

## [0.11.0] - 2026-07-29

### Added

- `RawPublisher` / `Node::create_raw_publisher`: pre-serialized standalone CDR
  messages on a topic whose type is known only at runtime, on both backends.
- `RawActionServer` / `Node::create_raw_action_server`: an action server raw
  where the action's user-defined types appear (the send_goal and get_result
  services, the feedback topic), with cancel_goal and status on their fixed
  `action_msgs` types. Goal lifecycle stays with the caller: transport, not
  policy.

## [0.10.2] - 2026-07-28

### Added

- `RawServer` / `Node::create_raw_server`: a raw (dynamic) service server on
  both backends, for services whose request and response types are known only
  at runtime.

### Changed

- A release is cut by merging a version bump to `master`: the release workflow
  publishes through crates.io trusted publishing when the version is not on
  crates.io yet.

## [0.10.1] - 2026-07-27

### Added

- Zenoh backend: `publish_raw` / `take_raw` (and `take_raw_keyed`) carry raw
  CDR payloads for runtime-typed messages; the type description is exposed and
  a publisher can be keyed on a given type hash
  (`create_publisher_with_type_hash`).

## [0.10.0] - 2026-07-22

First release under the `ros2-client-multi-rmw` name. Upstream 0.10.0 (RustDDS
0.13) plus an `rmw_zenoh`-compatible ROS 2-over-Zenoh backend, selected at
compile time by the mutually exclusive `dds` (default) and `zenoh` features:
topics, services, actions, parameters, `rosout`, graph discovery from
liveliness tokens, session configuration from the `rmw_zenoh` environment
variables, and REP-2016 RIHS01 type hashes computed from type descriptions.

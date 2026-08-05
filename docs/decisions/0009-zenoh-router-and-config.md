# 9. Zenoh router requirement and session configuration

- Status: accepted
- Date: 2026-07-07

## Context

`rmw_zenoh` runs each ROS process as a Zenoh **peer** that, by default, connects
to a local Zenoh **router** (`zenohd`) which gossips peer locators so peers form
direct p2p connections. Discovery relies on the router unless UDP multicast
scouting is enabled (disabled by default to avoid cross-robot interference).
Configuration comes from JSON5 files selected by `ZENOH_SESSION_CONFIG_URI` /
`ZENOH_ROUTER_CONFIG_URI`, with `ZENOH_CONFIG_OVERRIDE` for inline overrides.

## Decision

- `Context` opens one `zenoh::Session` in **peer** mode with defaults mirroring
  `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5` (connect `tcp/localhost:7447`, listen
  `tcp/localhost:0`, gossip on, multicast off, timestamping on). We bundle an
  equivalent default JSON5 and honour `ZENOH_SESSION_CONFIG_URI` and
  `ZENOH_CONFIG_OVERRIDE` so `ros2-client` obeys the same environment as
  `rmw_zenoh`.
- A running `zenohd`/`rmw_zenohd` router is **required by default**, documented in
  the README. Initialization tolerates a missing router (log + periodic retry,
  like `rmw_zenoh`) and connects when one appears.
- `ContextOptions` keeps `domain_id` (used as the key-expression prefix, not a
  transport setting) and gains zenoh-only config knobs (config path/overrides).
- **Testing compromise:** hermetic in-process tests (Tier B) connect two peers
  **directly** via explicit connect/listen endpoints, so they need **no** router.
  Real-ROS-2 interop tests (Tier C) launch `rmw_zenohd`.

## Consequences

- **Pro:** drop-in compatibility with the `rmw_zenoh` runtime environment and
  config; users configure `ros2-client` exactly as they configure ROS 2 nodes.
- **Con:** an external router process is part of the normal runtime (a real
  operational difference from the self-contained RustDDS backend). CI must run
  `zenohd` for interop. Multicast-only (router-free) operation is possible via
  override but not the default.

# Tier-C interop runbook (ros2-client `zenoh` ↔ real ROS 2 + `rmw_zenoh`)

The in-tree tests cover Tier A (byte-exact wire-format unit tests) and Tier B
(two `ros2-client` peers in-process over loopback, plus the client→router→client
path in `pub_sub_through_router`). **Tier C** — validating against a *real* ROS 2
stack using the official `rmw_zenoh` middleware — needs a ROS 2 installation and
is therefore run manually rather than in this repo's CI (which has no ROS 2
environment). This runbook is the concrete procedure; each step maps to a `C#`
acceptance criterion from the epic (#2) work items.

## 0. Prerequisites

Install ROS 2 (Jazzy or newer) and the Zenoh RMW, then start a router. `rmw_zenoh`
requires a running router (`zenohd`); `ros2-client`'s Zenoh backend does too for
multi-process discovery (ADR-0009).

```bash
# ROS 2 + rmw_zenoh (Jazzy example)
sudo apt install ros-jazzy-desktop ros-jazzy-rmw-zenoh-cpp
source /opt/ros/jazzy/setup.bash
export RMW_IMPLEMENTATION=rmw_zenoh_cpp

# Terminal R: the router (from rmw_zenoh, or a standalone zenohd)
ros2 run rmw_zenoh_cpp rmw_zenohd
```

Point `ros2-client` at the same router. With the default config it listens on
loopback in peer mode; for interop set it to connect to the router:

```bash
export ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/localhost:7447"]'
```

(`ZENOH_SESSION_CONFIG_URI` can instead point at the full `rmw_zenoh` JSON5
session config. Both are honoured by `Context` — see
`config_from_env` and ADR-0009.)

Keep the ROS domain id aligned on both sides (`ROS_DOMAIN_ID`, and
`ContextOptions::domain_id`; the Zenoh backend uses it as the key-expression
prefix).

## 1. Topics — C4

`ros2-client` publishes, ROS 2 subscribes:

```bash
# Run the bundled example against the router (edit it to use ZENOH_CONFIG_OVERRIDE
# or run your own talker built with `--no-default-features --features zenoh`).
cargo run --no-default-features --features zenoh --example zenoh_demo
# ROS 2 side:
ros2 topic echo /chatter std_msgs/msg/String
```

ROS 2 publishes, `ros2-client` subscribes:

```bash
ros2 topic pub /chatter std_msgs/msg/String "{data: 'hello from ros2'}"
# ros2-client subscription should receive "hello from ros2"
```

> Note on the **send** direction to C++ peers: for types outside the
> known-types table, `ros2-client` currently emits a placeholder REP-2016 hash,
> so a C++ subscriber that keys on the real hash may not match. Types in the
> table (`std_msgs/String`, `example_interfaces/srv/AddTwoInts`, …) match. The
> **receive** direction is universal (wildcard hash). See ADR-0007; the
> `type_description` module now computes real hashes and the remaining `msggen`
> integration closes this gap.

## 2. Services — C6

`ros2-client` server, ROS 2 client:

```bash
# ros2-client: create_server on /add_two_ints (example_interfaces/AddTwoInts)
ros2 service call /add_two_ints example_interfaces/srv/AddTwoInts "{a: 2, b: 40}"
# expect: sum=42
```

ROS 2 server, `ros2-client` client:

```bash
ros2 run examples_rclpy_minimal_service service
# ros2-client: create_client on /add_two_ints, call {a,b}, expect a+b
```

## 3. Actions — C7

```bash
# ROS 2 server:
ros2 run action_tutorials_cpp fibonacci_action_server
# ros2-client: create_action_client on /fibonacci
#   (action_tutorials_interfaces/Fibonacci), send_goal {order: 5},
#   observe feedback + /status, fetch result [0,1,1,2,3].
# Also exercise cancel:
#   send a long goal, cancel_goal(goal_id), observe Canceled on /status.
```

And the reverse (ros2-client server, `ros2 action send_goal` client):

```bash
ros2 action send_goal /fibonacci action_tutorials_interfaces/action/Fibonacci "{order: 5}" --feedback
```

## 4. Parameters — C8

```bash
# ros2-client: node with create_parameter_server([("speed", 1.0), ...]), spinning.
ros2 param list /param_holder
ros2 param get  /param_holder speed
ros2 param set  /param_holder speed 2.5
ros2 param describe /param_holder speed
```

Each should round-trip against the six `rcl_interfaces` services, and
`ros2 param set` should emit a `/parameter_events` message.

## 5. rosout — C9

```bash
# ros2-client: create_logger(); rosout!(logger, LogLevel::Info, "…")
ros2 topic echo /rosout rcl_interfaces/msg/Log
# expect the log line, with matching level/name/msg/file/line.
```

## 6. Discovery / ROS graph — C5

```bash
ros2 node list        # should list the ros2-client node(s)
ros2 topic list       # should list their topics
ros2 topic info /chatter --verbose   # publisher/subscriber counts
```

On the `ros2-client` side, `Context::node_names()` / `publisher_count()` /
`graph_event_stream()` should reflect the ROS 2 entities.

## Recording results

Capture pass/fail per criterion in `interop/results/` (mirroring the DDS
backend's interop results layout) with the ROS distro, `rmw_zenoh` version, and
`zenohd` version noted — the type-hash values in particular are distro-sensitive
(see ADR-0007 on `ServiceEventInfo`).

## Automating this later

A `workflow_dispatch` CI job could run this against a `ros-jazzy` container with
`rmw_zenoh_cpp` installed and `rmw_zenohd` started as a service step. It is
intentionally **not** added to the always-on CI here because it needs a ROS 2
image and a router daemon; wire it up in an environment that has both, using the
commands above as the script.

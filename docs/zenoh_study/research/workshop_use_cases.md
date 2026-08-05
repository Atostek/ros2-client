# ROS 2 Use Cases from the ZettaScaleLabs RosCon 2025 Workshop

Source repository: <https://github.com/ZettaScaleLabs/roscon2025_workshop>

This document catalogues the concrete ROS 2 use cases exercised by the
ZettaScaleLabs "ROS 2 over Zenoh" (`rmw_zenoh`) workshop, so they can be turned
into smoke tests for a Rust ROS 2 client (`ros2-client`) that talks over Zenoh.

The workshop is built around the standard ROS 2 demo nodes and CLI, but with
`rmw_zenoh_cpp` as the middleware. The valuable payload for us is the set of
plain ROS 2 interactions (topics, services, actions, graph introspection) that
these nodes perform, plus the exact Zenoh middleware configuration they run
under.

---

## 1. Overview, prerequisites and setup

### What the workshop is

A hands-on workshop teaching participants to run ROS 2 (Jazzy) over Zenoh using
`rmw_zenoh`. It progresses from a trivial talker/listener up to a full Nav2 +
Gazebo + RViz2 simulation, and then explores Zenoh transport features (shared
memory, remote connectivity, mTLS, wireless tuning, congestion control, internet
traversal).

- README: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/README.md>

### Hardware / environment prerequisites

- Linux, macOS or Windows laptop; 8 cores, 16 GB RAM, 30 GB free disk.
- Docker configured with an 8 CPU / 16 GB memory limit.
- Pre-pull the Docker image `zettascaletech/roscon2025_workshop`
  (multi-arch: amd64 + arm64).

### How Zenoh + ROS 2 are launched

- `docker compose up -d` starts **two containers**, both running ROS 2 Jazzy
  from the same image:
  - **robot** container — VNC at <http://localhost:6080/>, IP `172.1.0.2`
  - **control** container — VNC at <http://localhost:6081/>, IP `172.1.0.3`
  - VNC unlock password: `ubuntu`
  - Custom bridge network `sim_network`, subnet `172.1.0.0/16`.
  - Zenoh ports exposed: `7447` (TCP/UDP).
  - Each container mounts a host volume at `~/container_data`
    (`container_volumes/robot_container` and
    `container_volumes/control_container`).
  - Containers grant `NET_ADMIN`, `seccomp:unconfined`, `rtprio: 99`, and
    ~640 MB `/dev/shm` for the shared-memory exercises.
  - docker-compose: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/docker-compose.yaml>
    (a `docker-compose-common-shm.yaml` variant also exists for shared memory).

### Key environment variables (middleware config)

- `RMW_IMPLEMENTATION=rmw_zenoh_cpp` — set in both containers by default. This is
  the switch that makes ROS 2 use Zenoh instead of DDS.
- `ZENOH_ROUTER_CONFIG_URI` — path to the Zenoh **router** config file.
- `ZENOH_SESSION_CONFIG_URI` — path to the Zenoh **session** (ROS node) config file.
- `ZENOH_CONFIG_OVERRIDE` — inline `key/path=value;...` overrides applied *after*
  the config file is loaded. Example:
  `export ZENOH_CONFIG_OVERRIDE="scouting/multicast/enabled=true"`.
- `ROS_DOMAIN_ID` — used in the internet-traversal exercise to isolate multiple
  robots sharing one cloud router (`export ROS_DOMAIN_ID=123456`).
- A helper script `~/workshop_env.bash` sets `ZENOH_ROUTER_CONFIG_URI` /
  `ZENOH_SESSION_CONFIG_URI` automatically when
  `~/container_data/ROUTER_CONFIG.json5` / `SESSION_CONFIG.json5` exist.

### The Zenoh router (`rmw_zenohd`)

- Started with: `ros2 run rmw_zenoh_cpp rmw_zenohd`
- Role: a **discovery service** for nodes on the same host. When a node starts it
  tries to connect to the local router; the router shares each node's locators
  (IP + port) via a gossip protocol so nodes then form **direct peer-to-peer**
  connections. Once peers are connected, the router is no longer needed for them
  to communicate — you can `CTRL+C` the router and talker/listener keep going.
- The router can be started *after* the nodes; nodes retry periodically.
- **Multicast vs unicast discovery:** default discovery is via the router
  (unicast). Multicast scouting can be enabled to let nodes discover each other
  *without a router*, via
  `ZENOH_CONFIG_OVERRIDE='scouting/multicast/enabled=true'`.
- Default config files ship with `rmw_zenoh_cpp`:
  - `DEFAULT_RMW_ZENOH_ROUTER_CONFIG.json5` (router)
  - `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5` (all ROS processes)
  - Located at `/opt/ros/${ROS_DISTRO}/share/rmw_zenoh_cpp/config/`
    (or `~/rmw_zenoh/install/rmw_zenoh_cpp/share/rmw_zenoh_cpp/config/` in the
    workshop's from-source build).
  - Notable fields: `connect` (endpoints to dial), `listen` (endpoints to accept
    on), `scouting` (discovery), `mode` (`peer` vs `client`), `transport`
    (shared memory, compression, TLS, QoS).

### The `just` wrapper

Later exercises drive the heavy simulation through a `justfile` in the home
directory rather than raw `ros2` commands. Relevant recipes:
`just router`, `just rox_simu [no_gui] [use_wall_time:=True]`, `just rox_nav2`,
`just rviz_nav2`, `just cam_latency [image]`, `just top`, `just iftop_lo`,
`just iftop_router`, `just rt_factor`, `just network_limit`, `just network_normal`.

---

## 2. Exercise-by-exercise catalogue

### Exercise 1 — Zenoh router and ROS nodes

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-1.md>
- ROS 2 features demonstrated: **topic pub/sub, service client/server, action
  client/server, graph introspection**, and middleware discovery behavior.
- Message/service/action types: `std_msgs/msg/String` (talker/listener),
  `example_interfaces/srv/AddTwoInts` (add_two_ints),
  `action_tutorials_interfaces/action/Fibonacci` (fibonacci action).
- Exact commands:
  - Router: `ros2 run rmw_zenoh_cpp rmw_zenohd`
  - Talker: `ros2 run demo_nodes_cpp talker`
  - Listener: `ros2 run demo_nodes_cpp listener`
  - Multicast-only discovery (no router):
    `ZENOH_CONFIG_OVERRIDE='scouting/multicast/enabled=true' ros2 run demo_nodes_cpp talker`
    (and the same prefix for `listener`)
  - Service server: `ros2 run demo_nodes_cpp add_two_ints_server`
  - Service client: `ros2 run demo_nodes_cpp add_two_ints_client`
  - Action server: `ros2 run action_tutorials_cpp fibonacci_action_server`
  - Action client: `ros2 run action_tutorials_cpp fibonacci_action_client`
  - Introspection: `ros2 node list`, `ros2 topic list`, `ros2 service list`,
    `ros2 action list`
- Notable detail: the `ros2` CLI keeps working even with the router stopped
  because the ROS 2 daemon is itself a peer node that caches the graph.

### Exercise 2 — Complete simulation with Nav2

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-2.md>
- ROS 2 features: full navigation stack — many topics (LaserScan, camera
  image + point cloud, map, odometry, `/tf`), Nav2 actions (goal poses), RViz2
  visualization. Robot: Neobotix ROX; simulator: Gazebo; nav: Navigation2.
- Commands are wrapped by `just`: `just router`, `just rox_simu` (`no_gui` for
  headless), `just rox_nav2`, `just rviz_nav2`, plus metrics recipes
  (`just top`, `just iftop_lo`, `just rt_factor`, `just cam_latency`).
- Navigation goals are issued interactively via RViz2 ("Nav2 goal" tool), which
  under the hood sends a `nav2_msgs`/`geometry_msgs` action goal. No raw
  `ros2 action send_goal` is shown, but the mechanism is a ROS 2 action.
- Note: running with `use_wall_time:=True` disables Nav2.

### Exercise 3 — Shared memory

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-3.md>
- Focus: Zenoh transport, not new ROS 2 API surface. Enable
  `transport/shared_memory.enabled = true` in `ROUTER_CONFIG.json5` and
  `SESSION_CONFIG.json5`; size via
  `transport/shared_memory/transport_optimization/pool_size` (≥ 2× total
  published message size). Verify by inspecting `.zenoh` files in `/dev/shm`.
- Reuses the Nav2 simulation (`just router`, `just rox_simu use_wall_time:=True`,
  `just cam_latency [image]`) to measure image / point-cloud latency.

### Exercise 4 — Remote connectivity

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-4.md>
- Focus: cross-host Zenoh. The robot router listens on `tcp/[::]:7447`; remote
  hosts reach it at `tcp/172.1.0.2:7447`.
  - Solution 1 — cascaded routers: control container's router adds a
    `connect.endpoints: ["tcp/172.1.0.2:7447"]`.
  - Solution 2 — direct node as `mode: "client"` with the same connect endpoint
    in `SESSION_CONFIG.json5`.
  - Override example:
    `ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/173.1.0.2:7447"];transport/shared_memory/enabled=true'`
- ROS 2 payload is the same Nav2 / RViz2 topics streamed to a remote viewer.
- **Directly relevant to a Zenoh client:** demonstrates `peer` vs `client` mode
  and explicit unicast connect endpoints — exactly what a `ros2-client` over
  Zenoh must support.

### Exercise 5 — Securing communication with mTLS

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-5.md>
- Focus: transport security. Generate a root CA + robot/control certs
  (`smallstep/step-cli`). Router listens on `tcp/localhost:7447` (internal) and
  `tls/172.1.0.2:7447` (external); control connects via `tls/172.1.0.2:7447`.
  `transport/link/tls` block with `root_ca_certificate`, listen/connect
  key+cert, `enable_mtls: true`, `verify_name_on_connect: false`. QUIC
  (`quic/...`) offered as an encrypted UDP alternative.
- No new ROS 2 API; same topic/graph traffic over an encrypted link.

### Exercise 6 — Tuning for wireless networks

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-6.md>
- Focus: bandwidth reduction — LZ4 compression
  (`transport/unicast/compression.enabled`), Zenoh **access control** (allow/deny
  rules on key expressions, e.g. block point-cloud topics), and **downsampling**
  (e.g. cap camera image publications to 3 Hz on egress). Mentions Zenoh
  primitives: `put`, `reply`, `declare_subscriber/queryable`,
  `liveliness_token/query`, and TRANSIENT_LOCAL advertising.
- Commands: `just router`, `just rox_simu`, `just rox_nav2`, `just rviz_nav2`,
  `just iftop_router`.

### Exercise 7 — Congestion and head-of-line blocking

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-7.md>
- Focus: QoS priority mapping and congestion control in `ROUTER_CONFIG.json5`.
  8 Zenoh priority levels (`control`, `real_time`, `interactive_high`,
  `interactive_low`, `data_high`, `data`, `data_low`, `background`); map
  `**/map/**` & `**/scan/**` to `interactive_high`, `**/camera/points/**` to
  `background`; switch large payloads from `congestion_control: drop` to
  `block_first`. Emulate WiFi with `just network_limit` / `just network_normal`,
  verify with `ping 172.1.0.3`.

### Exercise 8 — Internet traversal

- Doc: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-8.md>
- Focus: a cloud-hosted Zenoh router as a NAT/firewall rendezvous
  (`quic/roscon.zenoh.io:7447`); both robot and control set
  `connect/endpoints` to it. Multi-robot conflict resolution via ROS
  namespaces + `/tf` remap, `ROS_DOMAIN_ID` (`export ROS_DOMAIN_ID=123456`), or a
  Zenoh `namespace: "<name>"` in `SESSION_CONFIG.json5` (transparent key-expr
  prefix; adjust router patterns `*/camera/` → `**/camera/`).

---

## 3. Testable use cases (workshop-derived scenarios)

Phrased as smoke-test scenarios for `ros2-client` over Zenoh. Each should be
runnable against a stock `rmw_zenoh_cpp` peer (the C++ demo nodes) to prove
wire-level interop, and/or between two `ros2-client` instances.

1. **Topic publish → external subscriber receives.** Publish
   `std_msgs/msg/String` on `/chatter` (like `demo_nodes_cpp talker`) and have
   `ros2 topic echo /chatter` (or a `demo_nodes_cpp listener`) receive the
   messages.
2. **Topic subscribe from external publisher.** Subscribe to `/chatter` and
   receive the `std_msgs/msg/String` stream produced by `demo_nodes_cpp talker`.
3. **Round-trip talker/listener between two clients.** One `ros2-client`
   publishes, another subscribes; assert message content and monotonic counter.
4. **Discovery via router.** Start `ros2 run rmw_zenoh_cpp rmw_zenohd`, then
   bring up pub/sub; assert they discover each other through the router
   (unicast).
5. **Discovery without router (multicast scouting).** With
   `ZENOH_CONFIG_OVERRIDE='scouting/multicast/enabled=true'` and no router,
   pub/sub still find each other; assert delivery.
6. **Router-independence after peering.** After pub/sub are connected, stop the
   router; assert messages keep flowing (peer-to-peer already established).
7. **Service call — AddTwoInts.** Call `/add_two_ints`
   (`example_interfaces/srv/AddTwoInts`) with `a` and `b`; assert
   `sum == a + b`. Test as client against `add_two_ints_server`, and as server
   answering `add_two_ints_client`.
8. **Action goal — Fibonacci.** Send a Fibonacci goal
   (`action_tutorials_interfaces/action/Fibonacci`, `order = N`); assert
   feedback (`partial_sequence`) arrives and the final result is the full
   Fibonacci sequence of length N. Test both as action client (vs
   `fibonacci_action_server`) and as action server (vs `fibonacci_action_client`).
9. **Graph introspection — node list.** `ros2 node list` returns the running
   node(s) published by `ros2-client`.
10. **Graph introspection — topic list.** `ros2 topic list` includes the topics
    a `ros2-client` node advertises (with correct types via `ros2 topic list -t`).
11. **Graph introspection — service list.** `ros2 service list` includes a
    service a `ros2-client` node exposes.
12. **Graph introspection — action list.** `ros2 action list` includes an action
    a `ros2-client` node exposes.
13. **Client mode + explicit connect endpoint.** Run a `ros2-client` node in
    Zenoh `client` mode with `connect/endpoints=["tcp/<host>:7447"]` and confirm
    pub/sub/service/action still work through a single router uplink (Exercise 4).
14. **Cross-host over cascaded routers.** Two hosts, each with a router, one
    connecting to the other's `tcp/<ip>:7447`; assert a topic published on host A
    is received on host B.
15. **QoS / TRANSIENT_LOCAL durability.** A late-joining subscriber to a
    TRANSIENT_LOCAL topic (e.g. a latched `/map`-style topic) receives the last
    published sample (implied by Exercises 6–7).
16. **ROS_DOMAIN_ID isolation.** Two node sets on different `ROS_DOMAIN_ID`
    values do not see each other's topics; same domain do (Exercise 8).

(Exercises 3, 5, 6, 7, 8 also imply transport-level scenarios — shared memory,
mTLS/QUIC links, compression, priority mapping, congestion control — but these
are Zenoh-config concerns rather than ROS 2 client API assertions, so they are
out of scope for the core smoke tests and listed here only for completeness.)

---

## 4. Basic ROS 2 CLI use cases to cover explicitly

These are the baseline CLI interactions the user wants a `ros2-client`-over-Zenoh
to satisfy. For each: the client feature it exercises and what a smoke test
should assert. All assume `RMW_IMPLEMENTATION=rmw_zenoh_cpp` on the CLI side and
a running `rmw_zenohd` (or multicast scouting).

| CLI command | ros2-client feature exercised | Smoke test assertion |
|---|---|---|
| `ros2 topic list` | Topic advertisement + discovery/graph publication | The client's advertised topic name(s) appear in the CLI output (with type via `-t`). |
| `ros2 topic pub /chatter std_msgs/msg/String "{data: hi}"` | Subscriber reception from an external publisher | A client subscriber receives the published `std_msgs/String` samples. |
| `ros2 topic echo /chatter` | Publisher visibility to an external subscriber | Client-published messages are printed by `echo` with correct content/type. |
| `ros2 node list` | Node liveliness / graph participation | The client's node name appears in the listed nodes. |
| `ros2 service list` | Service server advertisement in the graph | A service the client exposes appears (type via `ros2 service list -t`). |
| `ros2 service call /add_two_ints example_interfaces/srv/AddTwoInts "{a: 2, b: 3}"` | Service server request handling + response | Client service server returns `sum: 5`; response reaches the caller. |
| `ros2 param list` / `ros2 param get` / `ros2 param set` | Parameter services (`list/get/set_parameters`, describe, `parameter_events`) on a node | Client node exposes parameters; get returns declared values; set updates them and (optionally) emits a `parameter_events` message. |
| `ros2 action send_goal /fibonacci action_tutorials_interfaces/action/Fibonacci "{order: 5}" --feedback` | Action server: goal accept, feedback, result | Goal is accepted, feedback (`partial_sequence`) is streamed, and final result sequence returned. |

Notes on the parameter case: the workshop does not exercise `ros2 param`
explicitly, but it is on the user's required list. It exercises the ROS 2
parameter service interfaces (`rcl_interfaces/srv/{ListParameters, GetParameters,
SetParameters, DescribeParameters, GetParameterTypes}` and the
`rcl_interfaces/msg/ParameterEvent` topic on `/parameter_events`), which a
`ros2-client` node must serve to be introspectable/tunable via the CLI.

---

## 5. Standard interface packages / types to reuse in tests

Well-known, stable types so tests need no custom message packages:

- **`std_msgs`** — `std_msgs/msg/String` (talker/listener `/chatter`). Also
  `Int32`, `Header` as generic building blocks.
- **`example_interfaces`** — `example_interfaces/srv/AddTwoInts` (the
  `add_two_ints` service). Modern ROS 2 also ships
  `example_interfaces/action/Fibonacci`.
- **`action_tutorials_interfaces`** — `action_tutorials_interfaces/action/Fibonacci`
  (the `fibonacci_action_*` nodes used in Exercise 1).
- **`geometry_msgs`** — `geometry_msgs/msg/Twist`, `PoseStamped`,
  `Twist`/`TwistStamped` (navigation / velocity commands in the Nav2 exercise).
- **`sensor_msgs`** — `sensor_msgs/msg/{LaserScan, Image, PointCloud2}`
  (Nav2 / camera exercises; large-payload cases in 3, 6, 7).
- **`nav_msgs`** — `nav_msgs/msg/{Odometry, OccupancyGrid}` (`/map`, odometry in Nav2).
- **`tf2_msgs`** — `tf2_msgs/msg/TFMessage` on `/tf` and `/tf_static`
  (transforms; namespace/remap concerns in Exercise 8).
- **`rcl_interfaces`** — parameter services + `/parameter_events` (for the
  `ros2 param` CLI cases).
- **`turtlesim`** — not used by this workshop, but the canonical ROS 2 tutorial
  package (`turtlesim/msg/Pose`, `geometry_msgs/msg/Twist` on
  `/turtle1/cmd_vel`, `turtlesim/srv/Spawn`, `/turtle1/rotate_absolute` action).
  Listed because the user named it as a reusable well-known type source; a
  turtlesim-based test would exercise topic + service + action together.

### Standard nodes used by the workshop (test counterparts)

- `demo_nodes_cpp`: `talker`, `listener`, `add_two_ints_server`,
  `add_two_ints_client`.
- `action_tutorials_cpp`: `fibonacci_action_server`, `fibonacci_action_client`.
- `rmw_zenoh_cpp`: `rmw_zenohd` (the Zenoh router).

These C++ reference nodes make ideal interop counterparts: run the stock node on
one side and the `ros2-client` implementation on the other to prove wire
compatibility over Zenoh.

---

## Source URLs

- Repo root / README: <https://github.com/ZettaScaleLabs/roscon2025_workshop>,
  <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/README.md>
- docker-compose: <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/docker-compose.yaml>
- Exercise 1 (router + nodes, topics/services/actions/introspection):
  <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-1.md>
- Exercise 2 (Nav2 simulation): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-2.md>
- Exercise 3 (shared memory): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-3.md>
- Exercise 4 (remote connectivity): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-4.md>
- Exercise 5 (mTLS): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-5.md>
- Exercise 6 (wireless tuning): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-6.md>
- Exercise 7 (congestion / head-of-line blocking): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-7.md>
- Exercise 8 (internet traversal): <https://github.com/ZettaScaleLabs/roscon2025_workshop/blob/main/exercises/ex-8.md>

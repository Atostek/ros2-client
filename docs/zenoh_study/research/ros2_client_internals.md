# `ros2-client` internals: a feature-by-feature architecture map

Scope: this document maps how each ROS 2 feature in the `ros2-client` crate is
implemented, and pinpoints every place the implementation depends on RustDDS.
The goal is to enable adding a second backend (Zenoh) behind the same public API.
Citations are `file:line`. `context.rs` and `pubsub.rs` are documented
separately; this file references them only where `Node` uses them.

RustDDS is imported crate-wide with glob imports (`use rustdds::*;`) in almost
every module, so DDS types appear unqualified (`GUID`, `Topic`, `Timestamp`,
`QosPolicies`, `SequenceNumber`, `Duration`, `SampleInfo`, ...). The whole of
RustDDS is also re-exported (`pub use rustdds;` in `src/lib.rs:139`), and a
curated subset is re-exported under the `ros2::` module (`src/lib.rs:126-135`).

---

## 0. RustDDS surface used (quick index)

Types/methods from RustDDS that the client couples to (detailed per feature
below):

- `DomainParticipant` (via `Context`), `DomainParticipantStatusEvent`, `.status_listener()`, `.as_async_status_stream()`
- `Topic`, `TopicKind::NoKey`, `DomainParticipant::create_topic(...)`
- `no_key::DataWriter`, `no_key::SimpleDataReader`, `no_key::DeserializedCacheChange`, `no_key::DeserializerAdapter`/`SerializerAdapter`/`DefaultDecoder`/`Decode`
- `Publisher`/`Subscriber` (inside `pubsub.rs`, not read here, but wrapped)
- `GUID`, `GUID::to_bytes()`, `GUID::from_bytes()`
- `QosPolicies`, `QosPolicyBuilder`, `policy::*` (`Reliability`, `History`, `Durability`, `Deadline`, `Ownership`, `Lifespan`), `Duration`
- `Timestamp` (RTPS `Time_t`), `Timestamp::now()`, `Timestamp::ZERO`, `Timestamp::INVALID/INFINITE`, `.to_ticks()`
- `rpc::SampleIdentity`, `rpc::SequenceNumber`
- `WriteOptionsBuilder`, `.source_timestamp()`, `.related_sample_identity()`
- `SampleInfo` (`.source_timestamp()`, `.publication_handle()`, `.sample_identity()`, `.related_sample_identity()`)
- `RepresentationIdentifier` (`CDR_LE`, `CDR_BE`), `serialization::{to_writer_with_rep_id, deserialize_from_cdr_with_rep_id}`
- `dds::key::{Key, CdrEncodingSize}`
- error types `CreateError/CreateResult`, `ReadError/ReadResult`, `WriteError/WriteResult`
- `mio::Evented` (register/reregister/deregister) implemented by `Client`/`Server`

---

## 1. Context (how Node uses it)

`Context` wraps the RustDDS `DomainParticipant` and is documented separately.
`Node` holds a `Context` (`ros_context: Context`, `src/node.rs:626`) and calls
into it for all entity creation. The `Context` methods `Node` relies on:

- `ros_context.domain_participant()` -> `&DomainParticipant` (used directly for service topic creation, `src/node.rs:1332,1339,1384,1391`, and by `Spinner` for the status listener, `src/node.rs:182`)
- `create_topic`, `create_publisher`, `create_subscription`, `create_simpledatareader`, `create_datawriter` (`src/node.rs:1243,1258,1276,1290,1303`)
- `get_parameter_events_topic()`, `get_rosout_topic()`, `ros_discovery_topic()` (`src/node.rs:675,676,186`)
- `update_node(NodeEntitiesInfo)`, `remove_node(&str)` — ROS-graph discovery publishing (`src/node.rs:923,1593`)
- `domain_id()`, `clone()` (`Context` is cheaply clonable / shared)

Key point for the port: `Context` is the single chokepoint that owns the
`DomainParticipant`. A Zenoh backend would replace `Context`'s internals (a
Zenoh `Session`) and re-expose the same creation methods.

---

## 2. Node

Central file `src/node.rs` (~1778 lines).

### Public API
- `Node::new(NodeName, NodeOptions, Context)` (crate-internal; produced by `Context::new_node`), `src/node.rs:670`
- `create_topic`, `create_subscription`, `create_publisher` (`src/node.rs:1236,1253,1271`)
- `create_client`, `create_server` (`src/node.rs:1315,1367`)
- `create_action_client`, `create_action_server` (`src/node.rs:1410,1487`)
- `spinner()` -> `Spinner`; `Spinner::spin().await` (`src/node.rs:784`, `:180`)
- `status_receiver() -> Receiver<NodeEvent>` (`src/node.rs:1112`)
- `wait_for_reader`/`wait_for_writer` (crate-internal, `src/node.rs:1127,1149`)
- `get_publisher_count`/`get_subscription_count` (crate-internal, `src/node.rs:1175,1188`)
- Parameter API: `set_parameter`, `get_parameter`, `has_parameter`, `list_parameters`, `undeclare_parameter` (`src/node.rs:960-1058`)
- `time_now()` (sim-time aware), `logging_handle()`, `rosout_subscription()`, `domain_id()`, `fully_qualified_name()`

### Reader/writer bookkeeping
`Node` tracks entities it created in two `BTreeSet<Gid>`: `readers` and
`writers` (`src/node.rs:630-631`). Every `create_*` call funnels through
`add_reader`/`add_writer` (`src/node.rs:927,934`), which insert the entity's
`Gid` (converted from its DDS `GUID` via `.guid().into()`, e.g.
`src/node.rs:1259,1277`) and then re-publish the node's ROS graph info via
`ros_context.update_node(self.generate_node_info())`.

`generate_node_info()` (`src/node.rs:897`) builds a `NodeEntitiesInfo` from the
`Gid`s of the parameter-events writer, rosout writer, and all tracked
readers/writers. `suppress_node_info_updates` (`AtomicBool`, `src/node.rs:633`)
batches updates during construction to avoid flooding.

Separately, `Node` maintains DDS-match maps updated by the spinner:
`readers_to_remote_writers` and `writers_to_remote_readers`, both
`Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>` (`src/node.rs:640-641`). Keys are
local entity GUIDs; values are matched remote entity GUIDs. These drive
`get_publisher_count`/`get_subscription_count` and the `wait_for_*` futures.

### Spinner / spin
`Spinner` (`src/node.rs:145`) is the background event loop, produced by
`node.spinner()` and driven by `.spin().await` (`src/node.rs:180`). It is a big
`futures::select!` loop (`src/node.rs:227-452`) over:

1. `stop_spin_receiver` (async_channel) — shutdown.
2. **DDS discovery events**: `dds_status_stream` = `domain_participant().status_listener().as_async_status_stream()` (`src/node.rs:182-184`). Matches `DomainParticipantStatusEvent::{RemoteReaderMatched, RemoteWriterMatched, ReaderLost, WriterLost}` and updates the two match maps (`src/node.rs:420-447`). Every event is also forwarded to status listeners as `NodeEvent::DDS(...)`.
3. **ROS discovery events**: a subscription to the `ros_discovery_info` topic of type `ParticipantEntitiesInfo` (`src/node.rs:186-191`). On update it stores `part_update.node_entities_info_seq` keyed by `Gid` in `external_nodes` and forwards `NodeEvent::ROS(...)`.
4. **Simulated clock**: subscription to `/clock` (`builtin_interfaces::Time`), stored into `sim_time` (`src/node.rs:193-197,233-242`).
5. **Parameter service requests** (six servers): each server exposes `receive_request_stream()`; the spinner answers `get/set/list/describe/...` (`src/node.rs:200-398`).

`NodeEvent` (`src/node.rs:124`) is the public discovery event enum with two
variants: `DDS(DomainParticipantStatusEvent)` and `ROS(ParticipantEntitiesInfo)`.
**Both variants leak DDS/RTPS concepts** (`DomainParticipantStatusEvent` is a
RustDDS type; `ParticipantEntitiesInfo` carries `Gid`s).

### ROS graph event / status stream API
`status_receiver()` (`src/node.rs:1112`) registers an `async_channel::Sender`
into `status_event_senders` and returns the `Receiver<NodeEvent>`. The spinner
fans out events via `send_status_event` (`src/node.rs:459`), pruning closed
channels. Panics if no spinner is running.

### wait_for_reader / wait_for_writer
`wait_for_writer(reader_guid)` / `wait_for_reader(writer_guid)`
(`src/node.rs:1127,1149`) return custom futures `WriterWait`/`ReaderWait`
(`src/node.rs:1649,1707`). They first check the match maps; if already matched,
resolve immediately (`Ready`), else poll the `status_receiver()` stream for the
matching `DomainParticipantStatusEvent::RemoteReaderMatched`/`RemoteWriterMatched`
whose `local_writer`/`local_reader` equals the given `GUID`
(`src/node.rs:1674-1684,1735-1745`). Used by `Client::wait_for_service`.

### get_subscription_count etc.
`get_subscription_count(publisher_guid)` = length of the matched-readers set for
that writer GUID; `get_publisher_count(subscription_guid)` = matched-writers set
size (`src/node.rs:1175-1199`).

### DDS coupling in Node
- Match maps keyed by **`GUID`** and populated only from `DomainParticipantStatusEvent` (`src/node.rs:640-641,420-447`).
- `NodeEvent::DDS` exposes RustDDS `DomainParticipantStatusEvent` publicly.
- `wait_for_*` take `GUID` and depend on RTPS reader/writer matching semantics.
- Service topic creation calls `domain_participant().create_topic(..., TopicKind::NoKey)` directly (`src/node.rs:1332-1345,1384-1396`).
- QoS for parameter services built with `QosPolicyBuilder`/`policy::Reliability`/`History`/`Duration` (`src/node.rs:792-797`).
- `NodeCreateError::DDS(CreateError)` wraps RustDDS creation errors (`src/node.rs:574`).

---

## 3. Topics & naming (`src/names.rs`) — critical for Zenoh key expressions

This is the DDS name-mangling layer. Two axes: **topic names** (with kind
prefixes `rt/`, `rq/`, `rr/`) and **type names** (with `dds_` infix and trailing
`_`).

### Types
- `NodeName { namespace, base_name }` (`src/names.rs:16`) — `fully_qualified_name()` = `namespace + "/" + base_name` (`src/names.rs:86`).
- `Name { base_name, preceeding_tokens: Vec<String>, absolute: bool }` (`src/names.rs:126`) — a topic/service name split into tokens, with an `absolute` flag (leading `/`). Constructed via `Name::new(namespace, base_name)` or `Name::parse("a/b/c")` (`src/names.rs:146,212`).
- `MessageTypeName { prefix, ros2_package_name, ros2_type_name }` (`src/names.rs:295`); `prefix` defaults to `"msg"` (or `"action"`).
- `ServiceTypeName { prefix, msg }` (`src/names.rs:343`); `prefix` defaults to `"srv"` (or `"action"`).
- `ActionTypeName(MessageTypeName)` (`src/names.rs:396`).

### Topic-name mangling — `Name::to_dds_name(kind_prefix, node, suffix)` (`src/names.rs:239`)
Algorithm:
1. Start with `kind_prefix` (e.g. `"rt"`, `"rq"`, `"rr"`); asserts it does not end with `/`.
2. If the `Name` is **relative**, append the node's namespace (`node.namespace()`); if **absolute**, do not.
3. Append `/`.
4. Append each preceding token followed by `/`.
5. Append `base_name`, then `suffix`.

Result examples:
- `create_topic` uses `kind_prefix="rt"`, `suffix=""` (`src/node.rs:1242`): topic `/turtle1/cmd_vel` -> `rt/turtle1/cmd_vel`.
- Service request topic: `to_dds_name("rq", node, "Request")` (`src/node.rs:1333,1386`).
- Service response topic: `to_dds_name("rr", node, "Reply")` (`src/node.rs:1340,1391`). **Note the suffix is `"Reply"`, not `"Response"`** (the code even comments on this oddity, `src/node.rs:1330`).

Prefix meanings (ROS 2 `topic_and_service_names` design doc):
- `rt/` — regular topics (ROS "topic")
- `rq/` — service request
- `rr/` — service reply/response
- `rs/` — service (used by some RMWs for the "service" concept; **not emitted by this crate** — it only uses `rt`/`rq`/`rr`).

### Type-name mangling
- `MessageTypeName::dds_msg_type()` (`src/names.rs:330`):
  `slash_to_colons(package + "/" + prefix + "/dds_/" + type + "_")`.
  e.g. `std_msgs/String` -> `std_msgs::msg::dds_::String_`.
- `ServiceTypeName::dds_request_type()` / `dds_response_type()` (`src/names.rs:371,382`):
  `package + "/" + prefix + "/dds_/" + type + "_Request_"` (resp. `_Response_`),
  colon-joined. e.g. `example_interfaces/AddTwoInts` ->
  `example_interfaces::srv::dds_::AddTwoInts_Request_` / `..._Response_`.
- `slash_to_colons` (`src/names.rs:337`) simply replaces `/` with `::`.
- Action type names (`src/names.rs:411-427`):
  - `dds_action_topic(topic)` -> `MessageTypeName` with prefix `"action"` and type `<Type><topic>` (e.g. topic `"_FeedbackMessage"` -> type `<pkg>::action::dds_::<Type>_FeedbackMessage_`).
  - `dds_action_service(srv)` -> `ServiceTypeName` with prefix `"action"` and type `<Type><srv>` (e.g. `"_SendGoal"` -> `<pkg>::action::dds_::<Type>_SendGoal_Request_`).

### DDS coupling in naming
The mangling itself is pure string manipulation (no RustDDS types), so it ports
cleanly. **But**: the `rt/`/`rq/`/`rr/` prefixes and the `dds_`/`_Request_`
suffixes are the DDS wire convention. For Zenoh interop you must reproduce the
**same** mangled strings as key expressions (`rmw_zenoh` uses a specific keyexpr
format: `<domain>/<mangled_topic>/<type>/<type_hash>`), so this module is the
canonical source of the topic/type string format and should be reused/adapted
rather than reinvented. `to_dds_name` is the function to mirror.

---

## 4. Pub/Sub QoS

`QosPolicies` (RustDDS) is used directly and is part of the public API of
`create_topic`/`create_publisher`/`create_subscription` (`src/node.rs:1236-1279`;
`qos: &QosPolicies` / `Option<QosPolicies>`). Built-in topic QoS profiles are
defined in `src/builtin_topics.rs` with `QosPolicyBuilder` and `policy::*`:

- `ros_discovery::QOS_PUB/QOS_SUB` (`src/builtin_topics.rs:6-26`): TransientLocal/Volatile durability, `Deadline(INFINITE)`, `Ownership::Shared`, `reliable(ZERO)`, `History::KeepLast{depth:1}`, `Lifespan{INFINITE}`.
- `parameter_events::QOS` (`:36-42`): TransientLocal, reliable, KeepLast(1).
- `rosout::QOS` (`:52-63`): TransientLocal, reliable, KeepLast(1), `Lifespan{10s}`.

Node passes `None` to inherit topic QoS, or an explicit `QosPolicies`. Service
QoS in `spinner()` built inline (`src/node.rs:792`).

### DDS coupling
`QosPolicies`, `QosPolicyBuilder`, and every `policy::*` variant are RustDDS
types exposed directly in the public API (also re-exported via
`ros2::{policy, QosPolicies, QosPolicyBuilder, Duration}`, `src/lib.rs:127`).
The DDS QoS model (Durability, Ownership, Deadline, Lifespan, History,
Reliability with `max_blocking_time`) does not map 1:1 to Zenoh. This is a major
public-API coupling point.

---

## 5. Services (`src/service/`)

### ServiceMapping (`src/service/mod.rs:98-118`)
Enum with three variants selecting the RPC-over-DDS wire mapping:
- `Basic` — OMG DDS-RPC "Basic": each request/response carries a CDR header struct (`BasicRequestHeader{ request_id: SampleIdentity, instance_name: String }`, `BasicReplyHeader{ related_request_id: SampleIdentity, remote_exception_code: u32 }`, `src/service/wrappers.rs:253-284`). Correlation via **payload-embedded `SampleIdentity`**.
- `Enhanced` — OMG DDS-RPC "Enhanced": **no** payload header; correlation via **DDS inline-QoS `related_sample_identity`** (RTPS). Used by eProsima/rmw_fastrtps and RTI extended mapping. This is the default the crate uses for parameter services (`src/node.rs:804`).
- `Cyclone` — CycloneDDS-specific, reverse-engineered. Payload header `CycloneHeader{ guid_second_half: [u8;8], sequence_number_high: i32, sequence_number_low: u32 }` (`src/service/wrappers.rs:293-313`) — only the **last 8 bytes** of the client GUID travel in the header; the other 8 bytes are reconstructed from the DDS `writer_guid` of the received sample.

### Types & public API
- `Service` trait pairs `Request`/`Response` (`: Message`) with type-name accessors (`src/service/mod.rs:25`). `AService<Q,S>` is a runtime constructor (`:38`).
- `Client<S>` (`src/service/client.rs:16`): `send_request`/`async_send_request` -> `RmwRequestId`; `receive_response`/`async_receive_response(req_id)`; `async_call_service`; `wait_for_service`.
- `Server<S>` (`src/service/server.rs:18`): `receive_request`/`async_receive_request`/`receive_request_stream` -> `(RmwRequestId, Request)`; `send_response`/`async_send_response(rmw_req_id, Response)`.

### Topic naming for services
`create_client`/`create_server` (`src/node.rs:1315,1367`) each create two DDS
topics with `TopicKind::NoKey`:
- request topic: name `to_dds_name("rq", node, "Request")`, type `dds_request_type()`
- reply topic: name `to_dds_name("rr", node, "Reply")`, type `dds_response_type()`

### Internal implementation
`Client` holds a `DataWriterR<RequestWrapper<Req>>` (request_sender) and a
`SimpleDataReaderR<ResponseWrapper<Resp>>` (response_receiver)
(`src/service/client.rs:22-27`); `Server` is the mirror
(`src/service/server.rs:24-27`). `DataWriterR`/`SimpleDataReaderR` are aliases
for `no_key::DataWriter`/`no_key::SimpleDataReader` with custom pass-through
serializer adapters (`src/service/wrappers.rs:345-347`).

The (de)serializer adapters (`ServiceSerializerAdapter`/`ServiceDeserializerAdapter`,
`src/service/wrappers.rs:349-408`) do **no** real (de)serialization — they hand
raw bytes to the `Wrapper` types, because the `ServiceMapping` (needed to
generate/parse headers) is only known at the `Wrapper` layer, not at the RustDDS
adapter layer. `WrapperDecoder::decode_bytes` just wraps bytes + `RepresentationIdentifier`.

### Request-ID / correlation (the heart of the DDS coupling)
- `RmwRequestId { writer_guid: GUID, sequence_number: SequenceNumber }` (`src/service/request_id.rs:9`) — structurally **identical** to RustDDS `rpc::SampleIdentity`, with `From` conversions both ways (`src/service/request_id.rs:14-40`).
- On send, the client generates `RmwRequestId` from its own **request-writer GUID** (`client_guid = request_sender.guid()`, `src/service/client.rs:55`) plus a locally incremented atomic `SequenceNumber` (`src/service/client.rs:67-72,222-233`).
- `WriteOptionsBuilder` (`src/service/client.rs:79-85`): always sets `source_timestamp(Timestamp::now())`; for `Basic`/`Cyclone` also sets `related_sample_identity(SampleIdentity::from(gen_rmw_req_id))`. For `Enhanced`, it relies on the DDS stack to fill related sample identity and returns the **actually-sent** `RmwRequestId` from `write_with_options` (`.map(RmwRequestId::from)`), i.e. the real RTPS sequence number.
- Wire encoding per mapping in `RequestWrapper::new`/`unwrap` and `ResponseWrapper::new`/`unwrap` (`src/service/wrappers.rs:46-249`):
  - **Basic**: prepend `BasicRequestHeader`/`BasicReplyHeader` (contains `SampleIdentity`) to CDR payload.
  - **Enhanced**: payload only; on receive, `RmwRequestId` comes from `message_info.related_sample_identity()` (inline QoS). Fallbacks for FastDDS quirks: if missing, use `sample_identity()`; if `sequence_number == SequenceNumber::UNKNOWN`, patch it with the actual DATA submessage sequence number (`src/service/wrappers.rs:74-99,191-197`).
  - **Cyclone**: prepend `CycloneHeader`; on decode, reconstruct the full 16-byte client GUID from header (8 bytes) + received sample's `writer_guid` (8 bytes) (`src/service/wrappers.rs:199-216,317-343`).
- The server echoes correlation back: `send_response`/`async_send_response` set `related_sample_identity(SampleIdentity::from(rmw_req_id))` in `WriteOptions` for **all** mappings (plus the payload header for Basic/Cyclone) (`src/service/server.rs:94-107,167-181`).
- `MessageInfo` (`src/message_info.rs`) extracts `source_timestamp`, `sequence_number`, `publisher` (`publication_handle()`), and `related_sample_identity` from RustDDS `SampleInfo` / `DeserializedCacheChange`. This is the bridge that surfaces RTPS correlation metadata into the service layer.

### `mio::Evented`
`Client` and `Server` implement `mio::Evented` by delegating to the underlying
RustDDS `SimpleDataReader` (`src/service/client.rs:277-300`,
`src/service/server.rs:184-207`) for readiness-based (non-async) polling.

### DDS coupling in Services (severe)
- Correlation is fundamentally `GUID` + RTPS `SequenceNumber` (`RmwRequestId` == `SampleIdentity`).
- `Enhanced` mapping depends on RTPS **inline-QoS related_sample_identity** — a wire feature with no Zenoh equivalent.
- `Cyclone` mapping reconstructs GUIDs by splicing DDS `writer_guid` bytes.
- `WriteOptionsBuilder`/`related_sample_identity`/`source_timestamp` are RustDDS write-path types.
- Public API returns/consumes `RmwRequestId` (contains `GUID`, `SequenceNumber`), and errors are `ReadError`/`WriteError`.
- `RepresentationIdentifier` (CDR_LE/BE) and `serialization::*` are RustDDS CDR helpers.

---

## 6. Actions (`src/action.rs`, `src/action/{client,server}.rs`)

Actions are **composed entirely from 3 services + 2 topics** — there is no
new DDS mechanism, so actions inherit exactly the service/pubsub coupling.

### Composition (built in `Node::create_action_client`/`create_action_server`, `src/node.rs:1410-1560`)
Given `action_name` (e.g. `/turtle1/rotate_absolute`) and `ActionTypeName`
(e.g. `turtlesim/RotateAbsolute`), the base name is `action_name.push("_action")`.
Sub-entities:

| Sub-entity | Kind | Name (via `Name::push`) | Type |
|---|---|---|---|
| send_goal | Service (Client/Server) | `<action>/_action/send_goal` | `dds_action_service("_SendGoal")` -> `<pkg>::action::dds_::<Type>_SendGoal_Request_`/`_Response_` |
| cancel_goal | Service | `<action>/_action/cancel_goal` | `action_msgs/CancelGoal` (`ServiceTypeName::new("action_msgs","CancelGoal")`) |
| get_result | Service | `<action>/_action/get_result` | `dds_action_service("_GetResult")` |
| feedback | Topic (Sub on client / Pub on server) | `<action>/_action/feedback` | `dds_action_topic("_FeedbackMessage")` -> `<pkg>::action::dds_::<Type>_FeedbackMessage_` |
| status | Topic | `<action>/_action/status` | `action_msgs/GoalStatusArray` |

Note `Name::push` (`src/names.rs:259`) moves the current base name into
preceding tokens and sets a new base — so `/turtle1/rotate_absolute` + `_action`
+ `send_goal` produces topic `rq/turtle1/rotate_absolute/_action/send_goal...`.
Concrete expected DDS strings are enumerated in comments at
`src/action.rs:144-165`.

### IDL-equivalent message types (`src/action.rs:105-142`)
- `SendGoalRequest<G>{ goal_id: GoalId, goal: G }`, `SendGoalResponse{ accepted: bool, stamp: Time }`
- `GetResultRequest{ goal_id }`, `GetResultResponse<R>{ status: GoalStatusEnum, result: R }`
- `FeedbackMessage<F>{ goal_id, feedback: F }`
- Uses `action_msgs::{CancelGoalRequest, CancelGoalResponse, GoalId, GoalInfo, GoalStatusEnum, GoalStatusArray}`.
- `GoalId` is a `unique_identifier_msgs::UUID`, generated with `UUID::new_random()` (`src/action/client.rs:84,125`) — **application-level correlation**, independent of DDS GUIDs.

### Client (`src/action/client.rs`)
Wraps three `Client<AService<...>>` and two `Subscription<...>`
(`src/action/client.rs:32-44`). `send_goal` sends a random `GoalId`; correlation
of goal responses uses the **service** `RmwRequestId`, while feedback/status/result
are correlated by the application-level `GoalId` (filtering streams by
`goal_id`, `src/action/client.rs:245-335`).

### Server (`src/action/server.rs`)
`ActionServer` (sync) wraps three `Server<...>` + two `Publisher<...>`.
`AsyncActionServer` (`src/action/server.rs:268`) adds a goal state machine
(`GoalStatusEnum` transitions Unknown->Accepted->Executing->Succeeded/Aborted/Canceled)
in synchronous `Mutex<BTreeMap<GoalId, AsyncGoal>>`, buffering result requests by
`GoalId` until the goal finishes (`src/action/server.rs:552-597`). Handle types
`NewGoalHandle`/`AcceptedGoalHandle`/`ExecutingGoalHandle` enforce state via the
type system; each carries the service `RmwRequestId` for eventually responding.

### DDS coupling in Actions
Only indirect: everything goes through Services (see §5 coupling) and Pub/Sub
(§4 QoS). `GoalId`-based correlation is application-level and **ports cleanly**.
`ActionClientQosPolicies`/`ActionServerQosPolicies` (`src/action.rs:88-103`)
expose five `QosPolicies` fields each — DDS QoS in the public API.

---

## 7. Parameters (`src/parameters.rs`)

### Public API
`ParameterValue` (Rust enum, `src/parameters.rs:20`), `Parameter`,
`ParameterType`, `ParameterDescriptor`, `NumericRange`, `SetParametersResult =
Result<(), String>`. Node methods `set_parameter`/`get_parameter`/etc. (§2).

### Wire types (`src/parameters.rs:291-383`, module `raw`)
Serde structs mirroring `rcl_interfaces` msgs: `ParameterEvent`, `Parameter`,
`ParameterValue` (flat struct with all typed fields), `SetParametersResult`,
`ParameterDescriptor`, `IntegerRange`, `FloatingPointRange`. `From` impls convert
between the ergonomic enums and the flat `raw` structs.

### Implementation
- **Parameter services**: six `Server<rcl_interfaces::*>` created inside `spinner()` (`src/node.rs:803-858`) using `ServiceMapping::Enhanced` and node-qualified names (`get_parameters`, `get_parameter_types`, `set_parameters`, `set_parameters_atomically`, `list_parameters`, `describe_parameters`). Requests are handled in the spinner select loop (§2). So parameter services are ordinary ROS 2 services — same DDS coupling as §5.
- **parameter_events topic**: a `Publisher<raw::ParameterEvent>` on topic `rt/parameter_events` (name+QoS from `src/builtin_topics.rs:33-47`), created in `Node::new` (`src/node.rs:681`). Every `set_parameter`/`undeclare_parameter` publishes a `raw::ParameterEvent` (`src/node.rs:552-561,966-977,1020-1029`).
- `raw::ParameterEvent.timestamp` is a RustDDS `Timestamp` (`src/parameters.rs:298`); populated with `rustdds::Timestamp::now()` in `Spinner::set_parameter` (`src/node.rs:555`) or `self.time_now().into()` in `Node::set_parameter`.
- `use_sim_time` is a built-in parameter (declared in `Node::new`, `src/node.rs:685`); setting it toggles `use_sim_time: AtomicBool` which switches `time_now()` between `ROSTime::now()` and the `/clock`-fed `sim_time` (`src/node.rs:764-770,510-516`).

### DDS coupling in Parameters
Only via §5 (services use `ServiceMapping::Enhanced`, i.e. RTPS related sample
identity) and the `Timestamp` field type in `raw::ParameterEvent`. The enum/raw
conversion layer is DDS-agnostic.

---

## 8. rosout logging (`src/rosout.rs`, `src/builtin_topics.rs`)

### Public API
`RosoutRaw` trait (`src/rosout.rs:11`) with `rosout_raw(timestamp, level,
log_name, log_msg, file, function, line)`; implemented by `Node`
(`src/node.rs:1597`) and `NodeLoggingHandle` (`src/rosout.rs:77`, a `Send`-able
handle). The `rosout!` macro (`src/node.rs:1626`) is the ergonomic entry point.

### Implementation
`rosout_raw` publishes a `log::Log` message (`rcl_interfaces::msg::Log`) via a
`Publisher<Log>` and also emits a `tracing` event (`src/rosout.rs:41-63`). The
publisher lives in `Node.rosout_writer: Arc<Option<Publisher<Log>>>`
(`src/node.rs:651`), created in `Node::new` on topic `rt/rosout`
(name/type/QoS from `src/builtin_topics.rs:49-68`). Optional inbound
`rosout_reader: Option<Subscription<Log>>` if `read_rosout(true)`.
`Log.timestamp` is `ros2::Timestamp` (RustDDS `Timestamp`), passed by the macro
as `Timestamp::now()` (`src/node.rs:1631`).

### DDS coupling in rosout
Just ordinary pub/sub plus the `Timestamp` type in the message and macro. Ports
cleanly modulo the `Timestamp` type.

---

## 9. Discovery (`src/entities_info.rs`, `ros_discovery_info` topic)

### The ROS graph model
ROS 2 maps **many Nodes to one DDS DomainParticipant**. Discovery of the ROS
graph (which nodes exist, which readers/writers they own) is published on a
dedicated topic `ros_discovery_info` (`src/builtin_topics.rs:28`), type
`rmw_dds_common::msg::dds_::ParticipantEntitiesInfo_` (`:30`).

### Types
- `ParticipantEntitiesInfo { gid: Gid, node_entities_info_seq: Vec<NodeEntitiesInfo> }` (`src/entities_info.rs:26`) — one DomainParticipant's `Gid` + all ROS nodes it hosts.
- `NodeEntitiesInfo { name: NodeName, reader_gid_seq: Vec<Gid>, writer_gid_seq: Vec<Gid> }` (`src/entities_info.rs:59`) — a node and the `Gid`s of its readers/writers. Serialized via a `repr::NodeEntitiesInfo` shadow struct with flat `node_namespace`/`node_name` strings (`src/entities_info.rs:129-142`), using serde `try_from`/`into`.

### Assembly of the local ROS graph
Each `Node` builds its own `NodeEntitiesInfo` in `generate_node_info()`
(`src/node.rs:897`) from its tracked reader/writer `Gid`s (§2), and pushes it to
`Context::update_node(...)`. `Context` (elsewhere) aggregates all local nodes
into one `ParticipantEntitiesInfo` (keyed by the participant `Gid`) and publishes
it on `ros_discovery_info` with `ros_discovery::QOS_PUB` (TransientLocal,
reliable). On `Node::drop`, `Context::remove_node(fqn)` is called
(`src/node.rs:1593`).

### Consumption
`Spinner` subscribes to `ros_discovery_info` (`src/node.rs:186-191`) and stores
remote participants' node lists in `external_nodes: BTreeMap<Gid,
Vec<NodeEntitiesInfo>>` (`src/node.rs:405-408`), also forwarding `NodeEvent::ROS`.
So the ROS node list / topic list is assembled by merging these
`ParticipantEntitiesInfo` messages — this is a data-topic-based discovery, layered
on top of DDS SPDP/SEDP discovery (which separately drives the
`DomainParticipantStatusEvent` match maps).

### How GUIDs become Gids
Every entity's RustDDS `GUID` is converted to `Gid` at registration
(`.guid().into()`, §2). Participant identity is also a `Gid` (from the
participant's GUID). See §10.

### DDS coupling in Discovery
- `ros_discovery_info` topic and its `rmw_dds_common` type are the DDS-RMW discovery convention; a Zenoh backend uses `rmw_zenoh`'s liveliness-token-based graph discovery instead (different mechanism entirely).
- `Gid` values are DDS GUIDs (16/24 bytes), meaningful only in the RTPS world.
- The dual-source model (data-topic `ParticipantEntitiesInfo` + RTPS `DomainParticipantStatusEvent`) is DDS-specific.

---

## 10. Gid (`src/gid.rs`)

### Format
`Gid([u8; GID_LENGTH])` (`src/gid.rs:29`). Length is **feature-gated**:
`16` bytes for `iron`-or-newer, `24` bytes for galactic/humble
(`src/gid.rs:9-12`) — the ROS 2 `rmw_dds_common` `Gid.msg` changed size in
Jan 2023 (between Humble and Iron). Debug prints as hex.

### Relationship to RustDDS GUID
- `From<GUID> for Gid` (`src/gid.rs:40-46`): copies `guid.to_bytes()` into the array, zero-padding if `GID_LENGTH > 16` (a DDS `GUID` is 16 bytes, so the 24-byte format is 16 bytes of GUID + 8 zero bytes).
- `From<Gid> for GUID` (`src/gid.rs:48-52`): takes the first 16 bytes.
- `impl Key for Gid {}` (`src/gid.rs:54`) and `derive(CdrEncodingSize)` — RustDDS keyed-topic traits, so `Gid` can be a DDS key.

### DDS coupling in Gid
`Gid` **is** a DDS GUID (16 bytes) with padding, and implements RustDDS `Key`/
`CdrEncodingSize`. It is used pervasively in `NodeEntitiesInfo` and the public
`ParticipantEntitiesInfo`. For Zenoh, entity identity would instead be a Zenoh
`ZenohId`/entity id; `rmw_zenoh` fabricates GUID-shaped ids to keep the
`rmw_dds_common` graph format, so the `Gid` type may survive as an opaque
16-byte id but its provenance changes.

---

## DDS-isms that won't survive a Zenoh port

Concrete coupling points, roughly in order of severity:

1. **Service RPC correlation = RTPS `SampleIdentity` (`GUID` + `SequenceNumber`).**
   `RmwRequestId` is byte-identical to `rpc::SampleIdentity`
   (`src/service/request_id.rs`). The `Enhanced` mapping relies on RTPS
   **inline-QoS `related_sample_identity`** carried out-of-band by the DDS stack
   (`src/service/wrappers.rs:74-99,186-197`; `src/service/{client,server}.rs`
   `WriteOptionsBuilder::related_sample_identity`). Zenoh has no inline QoS —
   correlation must move into the payload (like Basic/Cyclone) or use a
   Zenoh query/reply. All three `ServiceMapping` variants are DDS wire mappings.

2. **`WriteOptions` / `related_sample_identity` / `source_timestamp` write path.**
   `WriteOptionsBuilder` is RustDDS-only (`src/service/client.rs:79-90`,
   `src/service/server.rs:94-107,167-181`). No Zenoh analogue for related sample
   identity; source timestamp must be modeled manually.

3. **DDS discovery coupling (dual mechanism).**
   (a) `DomainParticipantStatusEvent` stream + `RemoteReader/WriterMatched`,
   `Reader/WriterLost` drive the match maps and `wait_for_reader/writer` and the
   pub/sub counts (`src/node.rs:182-184,420-447,1127-1199`). (b) The
   `ros_discovery_info` data topic with `rmw_dds_common::ParticipantEntitiesInfo`
   (`src/entities_info.rs`, `src/builtin_topics.rs:28-31`). `rmw_zenoh` replaces
   both with liveliness tokens. `NodeEvent::DDS(DomainParticipantStatusEvent)` is
   a public API leak of a RustDDS type.

4. **`GUID`-based identity everywhere.**
   Match maps keyed by `GUID` (`src/node.rs:640-641`); `wait_for_*` take `GUID`
   (`src/node.rs:1127,1149`); `Gid` is a padded DDS GUID (`src/gid.rs`);
   `MessageInfo.publisher`/`writer_guid` = RTPS `publication_handle`
   (`src/message_info.rs:23-46`); Cyclone mapping splices GUID byte halves
   (`src/service/wrappers.rs:199-216,301-311`).

5. **RustDDS QoS types in the public API.**
   `QosPolicies`, `QosPolicyBuilder`, `policy::*`, `Duration` appear in
   `create_topic/publisher/subscription`, `create_client/server`, action QoS
   structs, and are re-exported via `ros2::` (`src/lib.rs:127`). The DDS QoS
   model (Durability/Ownership/Deadline/Lifespan/Reliability-with-max_blocking_time)
   has no clean Zenoh mapping.

6. **RustDDS `Timestamp` (RTPS `Time_t`) in message/metadata types.**
   `raw::ParameterEvent.timestamp` (`src/parameters.rs:298`), `log::Log.timestamp`,
   `rosout!` macro, `MessageInfo` timestamps, and the `ROSTime<->Timestamp`
   conversions (`src/ros_time.rs:86-133`) all use RustDDS `Timestamp`. Re-exported
   as `ros2::Timestamp`.

7. **Topic-kind prefixes and CDR type-name mangling are the DDS wire format.**
   `rt/`/`rq/`/`rr/` and `<pkg>::<kind>::dds_::<Type>_[Request_|Response_]`
   (`src/names.rs`). Not RustDDS *types*, but the DDS naming convention;
   `rmw_zenoh` needs the same mangled strings inside its keyexpr format (plus a
   type hash and domain id), so this must be adapted, not dropped.

8. **RustDDS error types in signatures.**
   `CreateError/Result`, `ReadError/Result`, `WriteError/Result` (and
   `NodeCreateError::DDS`, `GoalError::DDS*`, `CallServiceError`) are RustDDS
   `dds::*` types threaded through nearly every public method and re-exported via
   `ros2::` (`src/lib.rs:129`).

9. **RustDDS (de)serialization + representation identifiers.**
   `RepresentationIdentifier::{CDR_LE,CDR_BE}`,
   `serialization::{to_writer_with_rep_id, deserialize_from_cdr_with_rep_id}`,
   and the `no_key::{DataWriter, SimpleDataReader, DeserializerAdapter,
   SerializerAdapter, DefaultDecoder, Decode, DeserializedCacheChange}` adapter
   machinery (`src/service/wrappers.rs`). CDR is the DDS wire encoding; the whole
   pass-through-adapter design exists to inject `ServiceMapping` header handling.

10. **`mio::Evented` sync polling.**
    `Client`/`Server` expose `mio` readiness registration delegating to the
    RustDDS `SimpleDataReader` (`src/service/client.rs:277-300`,
    `src/service/server.rs:184-207`) — tied to RustDDS's mio integration.

11. **`Gid` implements RustDDS `Key`/`CdrEncodingSize`** and its length is
    ROS-distro/feature gated to match DDS GUID conventions (`src/gid.rs`).

Things that **do** port cleanly: the `names.rs` string algorithms (pure string
ops), the parameter enum<->raw conversion layer, action `GoalId`-based
correlation (application-level UUIDs), `ROSTime`/`ROSDuration` arithmetic, and
the message/IDL struct definitions themselves (plain serde).

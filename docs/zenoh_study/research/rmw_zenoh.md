# `rmw_zenoh` as ground truth for ROS 2 ↔ Zenoh mapping

Research reference compiled from the official ROS 2 RMW implementation
[`ros2/rmw_zenoh`](https://github.com/ros2/rmw_zenoh) (branch `rolling`,
cloned 2026-07-07). This document is intended to drive a Rust `ros2-client`
Zenoh backend, so it quotes exact format strings, field orderings, and the
source locations they come from.

Primary sources (all paths relative to the repo root):

- `README.md`
- `docs/design.md` — the authoritative design description.
- `rmw_zenoh_cpp/src/detail/liveliness_utils.cpp` / `.hpp` — key expressions,
  liveliness tokens, QoS encoding, GID generation, name mangling.
- `rmw_zenoh_cpp/src/detail/attachment_helpers.cpp` — message/request attachments.
- `rmw_zenoh_cpp/src/detail/rmw_client_data.cpp` — service client (querier / get).
- `rmw_zenoh_cpp/src/detail/rmw_service_data.cpp` — service server (queryable / reply).
- `rmw_zenoh_cpp/config/DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5`
- `rmw_zenoh_cpp/config/DEFAULT_RMW_ZENOH_ROUTER_CONFIG.json5`

A note on the interface version: `rmw_zenoh` today builds on **zenoh-cpp /
zenoh-c ≈ Zenoh 1.x** and uses the **advanced pub/sub** and **querier** APIs
(`declare_advanced_publisher`, `declare_advanced_subscriber`,
`declare_querier`), plus `ext::Serializer`/`ext::Deserializer` for attachments.
The Rust `zenoh` crate exposes the same concepts under `zenoh-ext`.

---

## 1. Overall architecture

### Router requirement

- A Zenoh **router (`zenohd`) is required in the default configuration.**
  From `docs/design.md`: *"With the default configuration, `rmw_zenoh_cpp`
  relies on a Zenoh router to discover peers and forward this discovery
  information to other peers via Zenoh's `gossip scouting` functionality. Hence
  `rmw_zenoh_cpp` requires the Zenoh router to be running."*
- The router is **only used for discovery and host-to-host communication**, not
  as a message broker. Actual data flows over **direct peer-to-peer**
  connections between sessions (design.md "Brief overview": *"data is sent via
  direct peer-to-peer connections"*).
- `rmw_zenoh` ships its own router binary, launched with
  `ros2 run rmw_zenoh_cpp rmw_zenohd` (source at `rmw_zenoh_cpp/src/zenohd/main.cpp`).
- The router requirement is a *configuration choice*, not a protocol necessity:
  if you enable UDP multicast scouting (disabled by default) peers can discover
  each other without a router. The default disables multicast deliberately to
  avoid cross-robot interference on a shared LAN and misconfigured-network
  issues.

### Mode: peer vs client vs router

Default topologies (from `docs/design.md` table and the two config files):

| Property        | Session (node)        | Router (`zenohd`)   |
|-----------------|-----------------------|---------------------|
| `mode`          | `peer`                | `router`            |
| Listen          | `tcp/localhost:0`     | `tcp/[::]:7447`     |
| Connect         | `tcp/localhost:7447`  | —                   |
| Gossip scouting | enabled               | enabled             |
| UDP multicast   | disabled              | disabled            |

- Nodes run as **peers** that connect to the local router over loopback
  (`tcp/localhost:7447`) and also listen on an OS-chosen loopback port
  (`tcp/localhost:0`). Gossip scouting lets the router hand each peer the
  endpoints of the others, so peers then form **p2p connections directly**.
- A node can instead be configured as a **client** (e.g. remote RViz over a
  network) via a custom session config.

### Session model

- **One ROS 2 context ↔ one Zenoh session** (`docs/design.md` "Contexts":
  *"a context maps to a Zenoh session, along with a subscription to liveliness
  tokens for the graph cache and some additional metadata"*). All publishers,
  subscriptions, services, and clients in that context **share the one
  session**.
- The session is opened with `Session::open(config)`. Config comes from
  `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5` unless `ZENOH_SESSION_CONFIG_URI`
  points elsewhere; per-field overrides via `ZENOH_CONFIG_OVERRIDE=
  "key/path=value;..."`.
- Relevant default session-config knobs (from
  `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5`):
  - `mode: "peer"`, `connect.endpoints: ["tcp/localhost:7447"]`,
    `listen.endpoints: ["tcp/localhost:0"]`.
  - `open.return_conditions: { connect_scouted: true, declares: true }` — open
    blocks until scouted peers/routers are connected and initial declares are
    received.
  - `scouting.multicast.enabled: false`, `scouting.gossip.enabled: true`.
  - `timestamping.enabled: { router: true, peer: true, client: true }` (needed
    for advanced pub/sub caching/history to work).
  - `queries_default_timeout: 600000` (ms) — default `get` timeout (10 min).
- At context init the RMW also declares a **liveliness subscriber** for the
  graph cache and performs an initial **liveliness query** (see §3). The router
  connection attempt count is governed by `ZENOH_ROUTER_CHECK_ATTEMPTS`
  (0 = wait indefinitely, negative = skip, positive = N attempts at 1 s
  intervals); initialization proceeds even if no router is found, and the node
  auto-connects once a router appears.

---

## 2. Key expression scheme (topics and services)

### Exact format

From `docs/design.md` and confirmed by `liveliness_utils.cpp`
`TopicInfo::TopicInfo` (lines ~77-97):

```
<domain_id>/<fully_qualified_name>/<type_name>/<type_hash>
```

Construction code (verbatim intent):

```cpp
topic_keyexpr_  = std::to_string(domain_id);
topic_keyexpr_ += "/";
topic_keyexpr_ += strip_slashes(name_);   // leading/trailing '/' removed
topic_keyexpr_ += "/";
topic_keyexpr_ += type_;
topic_keyexpr_ += "/";
topic_keyexpr_ += type_hash_;
```

Fields:

- `<domain_id>` — value of `ROS_DOMAIN_ID` (default `0`). Prefixing with the
  domain id **prevents cross-domain communication** even on shared routers.
- `<fully_qualified_name>` — the fully qualified topic/service name *with
  leading and trailing slashes stripped*, so the internal `/` of a namespace is
  kept as real Zenoh path separators (namespaces become key hierarchy). NOTE:
  this is **different** from liveliness tokens, where the name is *mangled*
  (`/`→`%`). In the *data-plane* topic key expression the slashes are kept.
- `<type_name>` — DDS-style CDR type name, e.g. `std_msgs::msg::dds_::String_`.
- `<type_hash>` — REP-2016 type hash from `rosidl`, e.g.
  `RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18`.

Including type name + type hash **prevents pub/sub (or client/server) with the
same name but mismatched types from matching**.

### Examples (from design.md)

```
0/chatter/std_msgs::msg::dds_::String_/RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18
0/robot1/chatter/std_msgs::msg::dds_::String_/RIHS01_df668c...          (namespace /robot1)
2/add_two_ints/example_interfaces::srv::dds_::AddTwoInts_/RIHS01_e118de6bf5eeb66a2491b5bda11202e7b68f198d6f67922cf30364858239c81a
```

Services use the **same** `<domain_id>/<name>/<type>/<hash>` scheme; the service
type name is the DDS srv type (`..::srv::dds_::AddTwoInts_`).

### Buffer-aware suffixes (GPU / `rosidl::Buffer`)

Optional, only for messages carrying `rosidl::Buffer<T>` fields (native/GPU
transport). Derived from the base topic key:

| Suffix | Direction | Purpose |
|--------|-----------|---------|
| *(none)* | both | Standard CDR-serialized payload; always declared (legacy fallback). |
| `/_buf_cpu` | pub → sub | Shared CPU-group channel for all CPU-only buffer-aware subscribers. |
| `/_buf/<sub_gid_hex>` | pub → sub | Per-subscriber accelerated channel; carries only the buffer descriptor for non-CPU backends. |

A Rust backend that does not implement native buffers can ignore these and just
use the base key.

---

## 3. Discovery — liveliness tokens and the graph cache

### Why

Zenoh does minimal discovery; ROS 2 assumes full graph introspection. Each
context keeps a **graph cache** of every entity (node, publisher, subscription,
service server, service client). Each entity, on creation, declares a
**liveliness token** whose *key expression encodes all of the entity's
metadata*; on destruction the token is dropped. Peers subscribe to liveliness
tokens to build/update the cache.

### Liveliness token key expressions

From `docs/design.md` and `liveliness_utils.cpp` (`Entity` constructor and the
`KeyexprIndex` enum, lines ~161-215, 502-589).

Node token (7 components):

```
@ros2_lv/<domain_id>/<session_id>/<node_id>/<node_id>/<entity_kind>/<mangled_enclave>/<mangled_namespace>/<node_name>
```

Publisher / subscription / service server / service client token:

```
@ros2_lv/<domain_id>/<session_id>/<node_id>/<entity_id>/<entity_kind>/<mangled_enclave>/<mangled_namespace>/<node_name>/<mangled_qualified_name>/<type_name>/<type_hash>/<qos>[/<backends>]
```

Component order is defined by the `KeyexprIndex` enum:

```
AdminSpace, DomainId, Zid, Nid, Id, EntityStr, Enclave, Namespace, NodeName,
TopicName, TopicType, TopicTypeHash, TopicQoS, Backends
```

Field semantics:

- `@ros2_lv` — constant admin-space prefix (`ADMIN_SPACE`). It marks a Zenoh
  "hermetic namespace": wildcards `*`/`**` never match this chunk.
- `<domain_id>` — `ROS_DOMAIN_ID`.
- `<session_id>` (a.k.a. `Zid`) — the Zenoh session id (one per context).
- `<node_id>` (`Nid`) — id of the node within the context.
- `<entity_id>` (`Id`) — id of the entity within the node. For a **node token**
  the `Nid` and `Id` slots are **both the node id** (hence the duplicated
  `<node_id>/<node_id>`).
- `<entity_kind>` (`EntityStr`) — two-letter code:
  - `NN` node, `MP` message publisher, `MS` message subscription,
    `SS` service server, `SC` service client.
- `<mangled_enclave>` — SROS enclave, mangled; `%` if unset.
- `<mangled_namespace>` — namespace, mangled; `%` if empty (the default).
- `<node_name>` — node name (mangled, but node names contain no `/`).
- `<mangled_qualified_name>` — fully qualified topic/service name, mangled.
- `<type_name>`, `<type_hash>` — as in §2.
- `<qos>` — compact QoS encoding, see §6.
- `<backends>` — optional; only for buffer-aware entities. Format
  `backends:<name>:<meta>;<name>:<meta>` with names sorted lexicographically and
  fields percent-escaped (`%`→`%25`, `;`→`%3B`, `:`→`%3A`, `/`→`%2F`).

**Name mangling** (`mangle_name`/`demangle_name`): every `/` is replaced by `%`.
This is required because Zenoh liveliness tokens cannot contain empty chunks
(`//`) and cannot be empty; an empty namespace is represented as a single `%`.
(Contrast with §2 topic keys, which keep real slashes.)

### Examples (from design.md)

```
@ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/0/NN/%/%/listener
@ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/10/MS/%/%/listener/%chatter/std_msgs::msg::dds_::String_/RIHS01_df668c.../::,10:,:,:,,
@ros2_lv/0/8b20917502ee955ac4476e0266340d5c/0/10/MP/%/%/talker/%chatter/std_msgs::msg::dds_::String_/RIHS01_df668c.../::,7:,:,:,,
@ros2_lv/0/f9980ee0495eaafb3e38f0d19e2eae12/0/10/SS/%/%/add_two_ints_server/%add_two_ints/example_interfaces::srv::dds_::AddTwoInts_/RIHS01_e118de.../::,10:,:,:,,
@ros2_lv/0/e1dc8d1b45ae8717fce78689cc655685/0/10/SC/%/%/add_two_ints_client/%add_two_ints/example_interfaces::srv::dds_::AddTwoInts_/RIHS01_e118de.../::,10:,:,:,,
```

Note the topic name is prefixed with a mangled leading slash (`%chatter`),
because the fully qualified name is `/chatter`.

### Late joiners / initial graph view

- The graph-cache liveliness **subscriber** is declared over the whole domain.
  The subscription key expression is (`liveliness_utils.cpp` `subscription_token`):

  ```cpp
  std::string token = std::string(ADMIN_SPACE) + "/" + std::to_string(domain_id) + "/**";
  // e.g. "@ros2_lv/0/**"
  ```

- On context init the RMW calls **`Session::liveliness_get`** on that same
  keyexpr to pull the **current** set of tokens (the existing graph) — this is
  how a late joiner learns already-present entities. Thereafter the liveliness
  **subscriber** delivers PUT (entity appeared) / DELETE (entity left) samples
  to keep the cache live.
- Zenoh APIs used (design.md "Contexts"/"Graph Cache" → Related Zenoh APIs):
  `Session::liveliness_declare_token`, `Session::liveliness_declare_subscriber`,
  `Session::liveliness_get`.

The Rust equivalents are `Session::liveliness().declare_token(...)`,
`...declare_subscriber(...)`, and `...get(...)`.

---

## 4. Message attachments

Every publication (and every service request/response) carries a Zenoh
**attachment** with three fields, in this order (from
`attachment_helpers.cpp` and design.md "Publishers"):

1. **sequence number** — `int64_t`
2. **source timestamp** — `int64_t`, nanoseconds since UNIX epoch
3. **source GID** — 16-byte array (`RMW_GID_STORAGE_SIZE == 16`)

The serialization uses `zenoh::ext::Serializer` / `Deserializer`:

```cpp
zenoh::Bytes AttachmentData::serialize_to_zbytes() {
  auto serializer = zenoh::ext::Serializer();
  serializer.serialize(this->sequence_number_);   // int64
  serializer.serialize(this->source_timestamp_);  // int64
  serializer.serialize(this->source_gid_);        // std::array<uint8_t,16>
  return std::move(serializer).finish();
}
```

The on-the-wire layout produced by that serializer (documented in design.md) is:

- 8 bytes — sequence number (int64, little-endian)
- 8 bytes — source timestamp (int64, little-endian)
- 1 byte — GID length (currently always `16`)
- 16 bytes — GID (16× int8)

**Rust note:** the `zenoh-ext` serializer prefixes variable-length items (the
array) with a length. To interoperate byte-for-byte, a Rust backend should use
the `zenoh_ext` serialization (`zenoh_ext::z_serialize`) or reproduce this exact
framing (two LE i64 + 1-byte length `16` + 16 bytes), rather than a naive
concatenation.

Why these fields:

- **sequence number** — correlates service requests to responses; also used for
  `MESSAGE_LOST` detection on subscriptions.
- **source timestamp** — populates `rmw_message_info_t.source_timestamp` /
  request headers.
- **source GID** — identifies the origin publisher/client (fills
  `publisher_gid` / `request_id.writer_guid`); on the service side it is hashed
  to index the pending-request map (§5).

---

## 5. Services (request/response)

Services are implemented with Zenoh **queryables** (server side) and **queriers
/ `get`** (client side). Both sides declare liveliness tokens (`SS` and `SC`).

### Server (`rmw_service_data.cpp`)

- Declares a **queryable** on the service key expression
  (`<domain>/<name>/<type>/<hash>`) with:
  ```cpp
  QueryableOptions qable_options = ...create_default();
  qable_options.complete = true;   // no wildcards -> can answer every request
  session->declare_queryable(keyexpr, on_query_cb, qable_options);
  ```
  `complete = true` advertises that this queryable can satisfy any matching
  query (it maps to `ALL_COMPLETE` on the querier side).
- Each incoming `zenoh::Query` is wrapped (`ZenohQuery`, with a received
  timestamp) and pushed on a `query_queue_`. `rmw_take_request` pops it,
  reads the attachment:
  - `request_id.sequence_number` ← attachment sequence number.
  - `request_id.writer_guid` ← attachment source GID (the **client** GID).
  - `request_header.source_timestamp` ← attachment source timestamp.
- The pending query is stored in a nested map so the response can find it later:
  ```
  sequence_to_query_map_ : hash_gid(client_gid) -> ( sequence_number -> ZenohQuery )
  ```
  Key = `hash_gid(writer_guid)` (the client's GID hashed to `size_t`),
  inner key = sequence number. Duplicate sequence numbers for the same client
  are rejected.
- `rmw_send_response` looks up `(hash_gid(request_id.writer_guid),
  request_id.sequence_number)`, retrieves the stored `Query`, and calls
  `Query::reply(...)` (Zenoh `reply` operation) with:
  - the serialized response payload, and
  - an attachment **echoing the request's sequence number and the client GID**,
    plus a **fresh source timestamp** for the reply.

### Client (`rmw_client_data.cpp`)

- Declares a **querier** (`Session::declare_querier`) on the service key
  expression with:
  ```cpp
  QuerierOptions options = ...create_default();
  options.target = Z_QUERY_TARGET_ALL_COMPLETE;                 // only complete queryables
  options.consolidation = Z_CONSOLIDATION_MODE_NONE;            // allow any number/order of replies
  // action get_result services get an effectively infinite timeout:
  if (querier_ke.intersects("**/_action/get_result/**"))
      options.timeout_ms = std::numeric_limits<uint64_t>::max();
  ```
- `rmw_send_request`:
  - assigns `*sequence_id = sequence_number_++` (client-local counter, starts at 1);
  - builds attachment `{sequence_id, now_ns, client_entity_gid}` via
    `entity_->copy_gid()`;
  - calls `querier_.get(parameters, reply_callback, opts)` with the serialized
    request as `opts.payload` and the attachment as `opts.attachment`.
- The reply callback receives `zenoh::Reply`s; each OK reply's `Sample` is
  wrapped in a `ZenohReply` (with received timestamp) and pushed to
  `reply_queue_` (bounded by the QoS depth).
- `rmw_take_response` pops a reply, reads the attachment, and sets
  `request_header->request_id.sequence_number` from the **reply's** attachment
  (the server echoed it) — this is how the client matches the response back to
  its request. `received_timestamp` comes from the wrapper.

### Correlation summary

`sequence_number` (client-assigned, monotonically increasing) plus the client
`GID` uniquely identify a request. The server keys its pending map by
`hash_gid(client_gid) -> sequence_number`, and echoes both back in the reply
attachment so the client can match. Zenoh's query/reply plumbing also ties each
reply to the originating `get`.

---

## 6. QoS mapping

From `docs/design.md` "Quality of Service" and `liveliness_utils.cpp`
(`qos_to_keyexpr`, `keyexpr_to_qos`, and the `qos_*_to_str` tables).

General principle: **in Zenoh there are no "incompatible" QoS** — any publisher
can match any subscription. QoS is (a) enforced/emulated with Zenoh mechanisms
where it maps naturally, and (b) *encoded into the liveliness token* so the
graph/introspection layer can report actual QoS and compute matches.

### Behavioural mapping

- **RELIABILITY**
  - `RELIABLE` (publishers): delivery over a reliable transport (TCP/QUIC) when
    such endpoints exist.
  - `BEST_EFFORT`: may drop; uses non-reliable endpoints (e.g. UDP) if
    configured, else falls back to reliable. Default config only has TCP.
    `BEST_EFFORT` is also `SYSTEM_DEFAULT`.
- **HISTORY**
  - `KEEP_LAST`: subscriptions keep up to `DEPTH` samples; for `TRANSIENT_LOCAL`
    publishers `DEPTH` sizes the publication cache. `SYSTEM_DEFAULT`.
  - `KEEP_ALL`: subscriptions keep all; reliable publishers set
    `CongestionControl::BLOCK` (publisher blocks under congestion → back-pressure).
- **DEPTH**: max samples for `KEEP_LAST`. **If `DEPTH == 0`, rmw_zenoh uses 42.**
- **DURABILITY**
  - `VOLATILE` (default): only live subscriptions receive data.
  - `TRANSIENT_LOCAL`: late joiners get history. Implemented via the
    **advanced publisher** with a **publication cache**, and the **advanced
    subscriber** configured to **query that cache** for historical data on
    startup. (This is the "querying subscriber" / latched-topic mechanism.)
- **LIVELINESS**
  - `AUTOMATIC` only — managed by the RMW via liveliness tokens.
  - `MANUAL_BY_TOPIC` unsupported.
- **DEADLINE**, **LIFESPAN**: currently **unimplemented** (values are still
  carried in the QoS keyexpr, but not enforced).

### Compact QoS encoding in liveliness tokens (`qos_to_keyexpr`)

The `<qos>` component is a **delta encoding against the RMW default QoS**: each
field is emitted only if it differs from the default, otherwise the slot is left
empty. Field/separator layout (built in `qos_to_keyexpr`):

```
<reliability>:<durability>:<history>,<depth>:<deadline_sec>,<deadline_nsec>:<lifespan_sec>,<lifespan_nsec>:<liveliness>,<lease_sec>,<lease_nsec>
```

- Separators: `:` (`QOS_DELIMITER`) between groups, `,`
  (`QOS_COMPONENT_DELIMITER`) between sub-fields.
- Policy enum values are the numeric `rmw_qos_policy_*` codes rendered as
  decimal strings (e.g. history KEEP_LAST, reliability RELIABLE, etc.).
- An empty field means "same as default" and is filled from
  `QoS::get().default_qos()` on decode (`keyexpr_to_qos`).

Worked example — `::,7:,:,:,,`:

- reliability empty (default), durability empty (default),
- history empty (default) / **depth = 7**,
- deadline empty/empty, lifespan empty/empty,
- liveliness empty / lease empty / lease empty.

i.e. only a non-default depth of 7 is encoded (the talker example). The listener
example `::,10:,:,:,,` encodes only depth 10.

`keyexpr_to_qos` reverses this: it splits on `:` (expects ≥6 groups), splits the
history/deadline/lifespan/liveliness groups on `,`, and fills empty fields with
the defaults.

---

## 7. GID (16-byte RMW GID)

Generated in `liveliness_utils.cpp`, `Entity` constructor (lines ~578-589):

- The GID is the **128-bit XXH3 hash of the entity's full liveliness key
  expression string**:
  ```cpp
  simplified_XXH128_hash_t keyexpr_gid =
      simplified_XXH3_128bits(liveliness_keyexpr_.c_str(), liveliness_keyexpr_.length());
  memcpy(gid_.data(),      &keyexpr_gid.low64,  8);
  memcpy(gid_.data() + 8,  &keyexpr_gid.high64, 8);
  ```
  So `gid[0..8]` = `low64` and `gid[8..16]` = `high64`, each written in host
  (little-endian) byte order. Implementation of the hash is in
  `simplified_xxhash3.cpp` (a self-contained XXH3-128 so the result is stable
  across platforms/versions).
- Because the liveliness keyexpr encodes domain, session id, node id, entity id,
  kind, names, type, and QoS, the GID is **deterministically unique per
  entity** and identical everywhere it is observed.
- A second `size_t` hash of the 16-byte GID (`hash_gid`, lines ~860-869) is used
  as an in-memory map key. `hash_gid` builds a hex string of the bytes and runs
  `std::hash<std::string>`; it is a *local* index only, not on the wire.
- Usage: the GID is placed in message attachments as `source_gid`
  (publisher/client identity), returned to the RMW via
  `rmw_get_gid_for_publisher` / `rmw_get_gid_for_client`, and used server-side
  (hashed) to demux pending service requests.

`RMW_GID_STORAGE_SIZE` is 16.

---

## 8. Actions

There is **no action concept at the RMW level** (design.md "Actions"). Actions
are composed entirely of ordinary **services + topics** at a higher ROS layer,
so `rmw_zenoh` has **no special Zenoh handling for actions**. The only
action-aware code is a client-side optimization: queriers whose key expression
intersects `**/_action/get_result/**` get an effectively infinite `get` timeout
(`rmw_client_data.cpp`), because `get_result` may block for a long time.

Implication: a Rust backend gets actions "for free" once services and topics
work; just replicate the `get_result` long-timeout heuristic.

---

## 9. Implications for a Rust `ros2-client` Zenoh backend

Concrete building blocks the Rust `zenoh` (+ `zenoh-ext`) crate must provide,
and how each maps to what rmw_zenoh does:

**Session / config**
- `zenoh::open(config)` — one session per ROS context. Load a JSON5 config
  equivalent to `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5` (peer mode, connect
  `tcp/localhost:7447`, listen `tcp/localhost:0`, gossip on, multicast off,
  timestamping on, `open.return_conditions`). Honor `ZENOH_SESSION_CONFIG_URI`
  and `ZENOH_CONFIG_OVERRIDE`. Require/expect an external `zenohd` router.
- Domain isolation is achieved purely by the `<domain_id>/` prefix in every key,
  not by config.

**Key expressions**
- Build data-plane keys as `"<domain>/<fqn_no_slashes>/<type_name>/<type_hash>"`
  (real slashes preserved in the name). Type name in DDS form
  (`pkg::msg::dds_::Type_`), type hash in `RIHS01_...` form.

**Publish / subscribe**
- `session.declare_publisher(key)` for volatile; for `TRANSIENT_LOCAL` use the
  **advanced publisher** with a **publication cache** sized by `DEPTH`
  (`zenoh_ext` `AdvancedPublisher` / `CacheConfig`).
- `session.declare_subscriber(key, cb)`; for `TRANSIENT_LOCAL` use the
  **advanced subscriber** with **history/query-on-startup** to pull cached
  samples (querying subscriber). Requires session `timestamping.enabled`.
- Apply congestion control `BLOCK` for `KEEP_ALL` + `RELIABLE` publishers.
- Every `put` must carry the attachment (§4).

**Attachments**
- Reproduce the exact `zenoh_ext` serialization of `(i64 seq, i64 ts_ns,
  [u8;16] gid)` — use `zenoh_ext::z_serialize` (length-prefixed array) for
  byte compatibility, not a hand-rolled concat.

**Services**
- Server: `session.declare_queryable(key)` with `complete(true)`; read the
  request attachment (seq, client gid, ts); keep a
  `HashMap<gid_hash, HashMap<seq, Query>>` of pending queries; answer with
  `query.reply(key, payload)` carrying an attachment that echoes seq + client
  gid and a fresh timestamp.
- Client: `session.declare_querier(key)` (or `session.get`) with
  `target = ALL_COMPLETE`, `consolidation = None`; monotonically increasing
  per-client `sequence_number` starting at 1; attach `(seq, ts, client_gid)`;
  correlate replies by the echoed sequence number.
- Long timeout for `**/_action/get_result/**`.

**Liveliness / discovery**
- Declare a **liveliness token** per entity (node/pub/sub/service/client) with
  the exact key format and component order from §3, including name mangling
  (`/`→`%`, empty→`%`) and the compact QoS encoding from §6.
- Declare a **liveliness subscriber** on `@ros2_lv/<domain>/**` and run an
  initial **liveliness get** on the same key to seed the graph cache; process
  PUT/DELETE to maintain it.
- Rust APIs: `session.liveliness().declare_token(key)`,
  `session.liveliness().declare_subscriber(key)`,
  `session.liveliness().get(key)`.

**GID**
- Compute the 16-byte GID as XXH3-128 of the entity's liveliness keyexpr string
  (low64 then high64, LE), so GIDs match rmw_zenoh peers exactly. Use a
  stable/simplified XXH3-128 implementation.

**QoS**
- Encode/decode the compact `<qos>` delta-vs-default string. Maintain a single
  canonical "default QoS profile" identical to rmw_zenoh's so the delta encoding
  interoperates. Treat DEADLINE/LIFESPAN as carried-but-not-enforced;
  LIVELINESS AUTOMATIC only; `DEPTH==0` → 42.

**Not needed for a first cut**
- Buffer-aware (`/_buf_cpu`, `/_buf/<gid>`) native/GPU transport — optional; the
  base key path fully interoperates.
- Shared-memory optimization — transparent, not required for correctness.
</content>
</invoke>

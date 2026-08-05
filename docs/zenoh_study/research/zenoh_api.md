# Zenoh Rust API Reference (for a ROS 2 middleware backend)

> **Target version:** `zenoh` / `zenoh-ext` **1.9.0** (current stable 1.x line, released ~2026).
> All code below targets the **1.x** API. The API changed drastically between 0.x and 1.0
> (builders, `ZBytes`, `Encoding`, session lifetime, liveliness, `zenoh-ext` advanced pub/sub),
> so 0.x snippets on the web will **not** compile against 1.x. Pin the version.
>
> Primary sources:
> - Crate docs: <https://docs.rs/zenoh/1.9.0/zenoh/> (and `latest`: <https://docs.rs/zenoh/latest/zenoh/>)
> - `zenoh-ext` docs: <https://docs.rs/zenoh-ext/latest/zenoh_ext/>
> - Repo + examples: <https://github.com/eclipse-zenoh/zenoh/tree/main/examples/examples>
> - `zenoh-ext` examples: <https://github.com/eclipse-zenoh/zenoh/tree/main/zenoh-ext/examples/examples>

---

## 1. Crate dependency, features, async/sync, compile considerations

### Cargo.toml

```toml
[dependencies]
# Core. `unstable` unlocks liveliness, matching listeners, advanced APIs, etc.
zenoh = { version = "1.9.0", features = ["unstable"] }
# Extended: AdvancedPublisher/AdvancedSubscriber (transient-local-like), serialization helpers.
zenoh-ext = { version = "1.9.0", features = ["unstable"] }
# You provide the async runtime yourself:
tokio = { version = "1", features = ["full"] }
```

### Key cargo features (from `zenoh/Cargo.toml`)

- `default` = a set of transports: `transport_tcp`, `transport_udp`, `transport_tls`,
  `transport_quic`, `transport_ws`, `transport_unixsock-stream`, `transport_compression`,
  `transport_multilink`, plus `auth_pubkey`, `auth_usrpwd`.
- **`unstable`** — REQUIRED for a ROS-2-style backend. Gates: **liveliness**,
  `matching_listener`, `SampleKind` in some paths, `Reliability` selection on subscribers,
  and the whole `zenoh-ext` advanced pub/sub surface. Without it these examples won't compile.
- `shared-memory` — SHM transport + `zenoh::shm` provider API (large-payload zero-copy).
- Optional transports you may want to *disable* to cut compile time: `transport_quic`,
  `transport_ws`, `transport_serial`, `transport_vsock`, `transport_unixpipe`.
- `internal` / `internal_config` — expose internal config plumbing (rarely needed).

### Async vs sync

Every terminal builder implements `IntoFuture`, so `.await` is the async form.
The **same** builder also implements `zenoh::Wait`, giving a blocking `.wait()`:

```rust
use zenoh::Wait;
let session = zenoh::open(config).wait().unwrap();          // sync
let publisher = session.declare_publisher("k").wait().unwrap();
publisher.put(data).wait().unwrap();
// async equivalents just use .await instead of .wait()
```

- The runtime is **tokio** (zenoh depends on `tokio` with `["macros","rt","time"]`).
  You do **not** need `async-std`; 1.x standardized on tokio internally.
- Zenoh spins up its **own** internal tokio runtime for its background I/O, so `.wait()`
  works even outside an async context. But if you call the async (`.await`) API you must be
  inside *your* runtime (`#[tokio::main]` or a manually built runtime). Mixing a blocking
  `.wait()` on the zenoh runtime *inside* a tokio worker thread can deadlock — prefer `.await`
  when already async.
- Handlers (`recv_async`) use the `flume` channel internally.

### Compile-time note

`zenoh` is a large crate with a deep dependency tree (QUIC/TLS via rustls, protocol, transport,
config). A cold build is heavy. Trim `default-features = false` + only the transports you use to
speed builds. Docs: <https://docs.rs/zenoh/latest/zenoh/> (crate-level "Cargo Features").

---

## 2. Config & Session

Type: `zenoh::Config` (re-export of `zenoh_config::Config`).
Docs: <https://docs.rs/zenoh/latest/zenoh/config/struct.Config.html>

Config is edited as a JSON5 document via string paths. There is no large typed builder;
you mutate paths with `insert_json5`.

```rust
use zenoh::config::{Config, WhatAmI};
use serde_json::json;

// Start from defaults, a file, or a JSON5 string:
let mut config = Config::default();
// let mut config = Config::from_file("zenoh.json5").unwrap();
// let mut config = Config::from_json5(r#"{ mode: "peer" }"#).unwrap();

// Session mode: "peer" (default), "client", or "router".
config.insert_json5("mode", &json!("peer").to_string()).unwrap();

// Explicit endpoints to connect to (e.g. a known router or peer):
config.insert_json5("connect/endpoints", &json!(["tcp/192.168.1.10:7447"]).to_string()).unwrap();

// Endpoints to listen on (so others can connect to us):
config.insert_json5("listen/endpoints", &json!(["tcp/0.0.0.0:0"]).to_string()).unwrap();

// --- Scouting / discovery (peer-to-peer, no router needed) ---
// Multicast UDP scouting (auto-discover peers on the LAN):
config.insert_json5("scouting/multicast/enabled", &json!(true).to_string()).unwrap();
config.insert_json5("scouting/multicast/address", &json!("224.0.0.224:7446").to_string()).unwrap();
// Gossip scouting (peers exchange the peers they know about):
config.insert_json5("scouting/gossip/enabled", &json!(true).to_string()).unwrap();
// Disable multicast scouting (e.g. rely on explicit connect endpoints):
// config.insert_json5("scouting/multicast/enabled", &json!(false).to_string()).unwrap();

// --- No router required ---
// In "peer" mode with gossip+multicast scouting, peers form a mesh directly; a router is
// optional. To force full-mesh peer connectivity (each peer connects to each other peer):
config.insert_json5("routing/peer/mode", &json!("linkstate").to_string()).unwrap();

// Enable timestamping (needed for AdvancedPublisher caching / transient_local, section 8):
config.insert_json5("timestamping/enabled", &json!(true).to_string()).unwrap();
```

Config methods (docs: link above):
- `Config::default() -> Config`
- `Config::from_file<P: AsRef<Path>>(path) -> ZResult<Config>`
- `Config::from_json5(&str) -> Result<Config, ...>`
- `config.insert_json5(key: &str, value: &str) -> ZResult<()>` — set any path.
- `config.get_json(key: &str) -> ZResult<String>` — read a path back.
- Typed accessors exist too, e.g. `config.timestamping.set_enabled(Some(ModeDependentValue::Unique(true)))` (used by the advanced-pub example).

`WhatAmI` (mode enum): `WhatAmI::Peer | Client | Router`.
Docs: <https://docs.rs/zenoh/latest/zenoh/config/enum.WhatAmI.html>

### Opening a session

```rust
// zenoh::open(config) -> OpenBuilder;  .await -> ZResult<Session>
let session: zenoh::Session = zenoh::open(config).await.unwrap();
```

- `zenoh::open(config)` — <https://docs.rs/zenoh/latest/zenoh/fn.open.html>
- `Session` — <https://docs.rs/zenoh/latest/zenoh/struct.Session.html>
- **The `Session` is the anchor.** It is internally `Arc`-based and `Clone`; cloning is cheap
  and shares the same underlying session. Keep at least one handle alive for the whole program.
- Close explicitly to flush & release: `session.close().await.unwrap();`
  (<https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.close>)

Logging init helper (call once at startup):
```rust
zenoh::init_log_from_env_or("error"); // reads RUST_LOG, defaults to "error"
```

---

## 3. Publisher / Subscriber

### Publisher — `declare_publisher`, `put` with encoding + attachment

Source: examples `z_pub.rs`, `z_pub_thr.rs`.

```rust
use zenoh::bytes::Encoding;

let publisher = session.declare_publisher("demo/example/topic").await.unwrap();

publisher
    .put(b"hello".to_vec())          // payload: anything Into<ZBytes>
    .encoding(Encoding::TEXT_PLAIN)  // optional encoding metadata
    .attachment(b"meta".to_vec())    // optional attachment (Into<ZBytes>)
    .await
    .unwrap();

// Delete (tombstone) on the publisher's key:
publisher.delete().await.unwrap();
```

- `Session::declare_publisher(key_expr) -> PublisherBuilder`
  <https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.declare_publisher>
- `Publisher` — <https://docs.rs/zenoh/latest/zenoh/pubsub/struct.Publisher.html>
- `Publisher::put(payload) -> PublisherPutBuilder` with `.encoding(..)`, `.attachment(..)`,
  and the QoS knobs in section 8.
- One-shot publish without a publisher: `session.put(key, payload).await` (`z_put.rs`) and
  `session.delete(key).await` (`z_delete.rs`).

### Subscriber — stream (`recv_async`) vs callback

**Stream style** (source `z_sub.rs`):

```rust
let subscriber = session.declare_subscriber("demo/example/**").await.unwrap();

while let Ok(sample) = subscriber.recv_async().await {   // also .recv() for sync
    let payload = sample.payload().try_to_string()
        .unwrap_or_else(|e| e.to_string().into());
    println!("{} on '{}': {}", sample.kind(), sample.key_expr().as_str(), payload);

    if let Some(att) = sample.attachment() {
        let att = att.try_to_string().unwrap_or_else(|e| e.to_string().into());
        println!("  attachment: {att}");
    }
}
```

**Callback style** (source `z_sub_thr.rs`): the callback runs on zenoh's I/O thread. Use
`.background()` so the subscriber keeps running without you holding the handle, or keep the
returned handle alive.

```rust
let _subscriber = session
    .declare_subscriber("demo/example/**")
    .callback(|sample| {
        // process sample here; keep it fast & non-blocking
    })
    // .callback_mut(move |sample| { ... })  // FnMut variant
    .background()   // detach: no need to hold the handle
    .await
    .unwrap();
```

`Sample` accessors (docs: <https://docs.rs/zenoh/latest/zenoh/sample/struct.Sample.html>):
- `sample.payload() -> &ZBytes`
- `sample.key_expr() -> &KeyExpr` (`.as_str()` for the string)
- `sample.kind() -> SampleKind` (`Put` / `Delete`)
- `sample.encoding() -> &Encoding`
- `sample.attachment() -> Option<&ZBytes>`
- `sample.timestamp() -> Option<&Timestamp>`
- `sample.priority()`, `sample.congestion_control()`, `sample.express()`, `sample.reliability()`

`Session::declare_subscriber` — <https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.declare_subscriber>
`Subscriber` — <https://docs.rs/zenoh/latest/zenoh/pubsub/struct.Subscriber.html>

---

## 4. Attachments (ZBytes)

Attachments are arbitrary side-band bytes carried next to the payload — ideal for ROS 2 RMW
metadata (GID, sequence number, source timestamp) that you don't want to bake into the payload.

```rust
use zenoh::bytes::ZBytes;

// Build an attachment from raw bytes:
let attachment = ZBytes::from(vec![1u8, 2, 3]);

publisher.put(payload).attachment(attachment).await.unwrap();

// Read it back on the subscriber:
if let Some(att) = sample.attachment() {           // &ZBytes
    let raw: std::borrow::Cow<[u8]> = att.to_bytes();  // infallible -> Cow<[u8]>
    let bytes: &[u8] = &raw;
    // ... parse your metadata from `bytes`
}
```

- On query too: `query.attachment() -> Option<&ZBytes>` and `session.get(sel).attachment(..)`.
- `ZBytes::to_bytes()` is **infallible** and returns `Cow<[u8]>` (borrow when contiguous, else copy).
- `ZBytes::try_to_string()` returns `Result<Cow<str>, _>` (fails on non-UTF8).

---

## 5. Key expressions

Types: `zenoh::key_expr::KeyExpr<'a>` (owned/borrowed, validated) and `keyexpr` (unsized
borrowed str-like). Docs: <https://docs.rs/zenoh/latest/zenoh/key_expr/struct.KeyExpr.html>

```rust
use zenoh::key_expr::{KeyExpr, keyexpr};

// Validated construction (fails on illegal keys):
let ke: KeyExpr = KeyExpr::try_from("demo/example/topic").unwrap();
let ke_static: KeyExpr<'static> = KeyExpr::try_from("a/b".to_string()).unwrap();

// Borrowed, const-checkable:
let k: &keyexpr = keyexpr::new("test/ping").unwrap();

// Building by concatenation / joining:
let joined: KeyExpr = ke.join("sub/leaf").unwrap();      // "demo/example/topic/sub/leaf"
let cat: KeyExpr = (&ke) / keyexpr::new("child").unwrap(); // Div operator also works

// Wildcards:
//   *   matches a single chunk  (a/*/c  matches a/b/c, not a/b/x/c)
//   **  matches zero or more chunks (a/**  matches a, a/b, a/b/c ...)
let wild: KeyExpr = KeyExpr::try_from("demo/example/**").unwrap();

// Matching / inclusion tests:
let intersects: bool = ke.intersects(&wild);      // do the two sets overlap?
let includes:   bool = wild.includes(&ke);        // does wild's set contain ke's set?
```

Most APIs accept `impl TryInto<KeyExpr>`, so you can pass a `&str` literal directly
(`session.declare_subscriber("a/**")`). Invalid keys surface as an error at declaration time.
Validation rules: no empty chunks, no leading/trailing `/`, `*`/`**` occupy a whole chunk.

Optionally **declare** a key expression once to get a compact numeric id used on the wire:
`let ke = session.declare_keyexpr("long/prefix/topic").await.unwrap();`
(<https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.declare_keyexpr>) — worthwhile
for hot ROS topics.

---

## 6. Queryable & Query (request/response = ROS services)

### Server side — declare a queryable and reply

Source: `z_queryable.rs`.

```rust
let queryable = session
    .declare_queryable("demo/example/service")
    .complete(true)   // this queryable fully covers the key expr (affects BestMatching target)
    .await
    .unwrap();

while let Ok(query) = queryable.recv_async().await {     // or .callback(...)
    // Inspect the request:
    let sel = query.selector();                 // Selector: key_expr + parameters
    let ke  = query.key_expr();                 // &KeyExpr
    let params = query.parameters();            // &Parameters (the "?a=b" part)
    if let Some(req) = query.payload() {         // Option<&ZBytes> request body
        let body = req.try_to_string().unwrap_or_else(|e| e.to_string().into());
        // ... decode request
    }
    if let Some(att) = query.attachment() {      // Option<&ZBytes> request metadata
        let _ = att.to_bytes();
    }

    // Reply with a value (multiple replies allowed; reply on a key matching the query):
    query
        .reply("demo/example/service", b"response".to_vec())
        .await
        .unwrap();

    // Reply an error instead:
    // query.reply_err(b"boom".to_vec()).await.unwrap();
    // Reply a delete:
    // query.reply_del("demo/example/service").await.unwrap();
}
```

- `Session::declare_queryable(key_expr) -> QueryableBuilder`
  <https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.declare_queryable>
- `Query` — <https://docs.rs/zenoh/latest/zenoh/query/struct.Query.html>
  (`payload()`, `attachment()`, `selector()`, `key_expr()`, `parameters()`, `encoding()`,
  `reply()`, `reply_err()`, `reply_del()`).
- `query.reply(key, payload)` also has a builder form with `.encoding(..)`, `.attachment(..)`,
  `.timestamp(..)`.

### Client side — `session.get(selector)` with payload/attachment, receive replies

Source: `z_get.rs`.

```rust
use zenoh::query::{QueryTarget, Selector};

let mut builder = session
    .get("demo/example/service")     // impl TryInto<Selector>; "key?params" also valid
    .target(QueryTarget::BestMatching) // or ::All / ::AllComplete
    .timeout(std::time::Duration::from_secs(5))
    .encoding(zenoh::bytes::Encoding::APPLICATION_JSON)
    .attachment(b"req-meta".to_vec());
// Attach a request payload (carry the request body to the queryable):
builder = builder.payload(b"request-body".to_vec());

let replies = builder.await.unwrap();   // returns a handler channel of Reply

while let Ok(reply) = replies.recv_async().await {
    match reply.result() {                 // Result<&Sample, &ReplyError>
        Ok(sample) => {
            let payload = sample.payload().try_to_string()
                .unwrap_or_else(|e| e.to_string().into());
            println!("reply on '{}': {}", sample.key_expr().as_str(), payload);
        }
        Err(err) => {
            let e = err.payload().try_to_string().unwrap_or_else(|e| e.to_string().into());
            println!("ERROR reply: {e}");
        }
    }
}
```

- `Session::get(selector) -> SessionGetBuilder` — <https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.get>
  Builder methods: `.target(..)`, `.timeout(..)`, `.payload(..)`, `.attachment(..)`,
  `.encoding(..)`, `.consolidation(..)`, `.with(handler)` (e.g. `RingChannel`).
- `QueryTarget` — <https://docs.rs/zenoh/latest/zenoh/query/enum.QueryTarget.html>
  `BestMatching` (one best queryable), `All`, `AllComplete`.
- `Reply` — <https://docs.rs/zenoh/latest/zenoh/query/struct.Reply.html> — `result()`,
  `replier_id()`.
- **Correlating replies to requests:** each `session.get(...)` yields its own `replies`
  channel, so replies are already scoped to that request — no manual correlation id needed for
  the basic case. For a ROS-2 service with concurrent in-flight calls you typically run one
  `get` per call. If you need extra correlation (e.g. matching to a ROS request id), put it in
  the query **attachment** and echo it back in the reply attachment. The channel closes when
  the query completes (timeout or all `complete` queryables answered).

---

## 7. Liveliness — this is how discovery works

`session.liveliness()` returns a `Liveliness` handle.
Docs: <https://docs.rs/zenoh/latest/zenoh/liveliness/struct.Liveliness.html>
(Requires the **`unstable`** feature.)

Model: an entity **declares a liveliness token** on some key expr. While the token (and its
session) is alive, that key "exists". Others **subscribe** to a key expr to get `Put` events
when tokens appear and `Delete` events when they vanish (including on ungraceful disconnect),
and can **query** to enumerate currently-alive tokens. This is exactly the primitive you build
ROS 2 discovery on: encode node/topic/type info into token key exprs.

### Declare a token (announce presence)

Source: `z_liveliness.rs`.

```rust
let token = session
    .liveliness()
    .declare_token("group1/node/my_node")
    .await
    .unwrap();

// Keep `token` alive for as long as the entity should be considered present.
// Dropping it (or session close / process crash) emits a Delete to subscribers.
token.undeclare().await.unwrap();   // explicit graceful removal
```

`Liveliness::declare_token(key_expr) -> LivelinessTokenBuilder` →
`LivelinessToken` (<https://docs.rs/zenoh/latest/zenoh/liveliness/struct.LivelinessToken.html>).

### Subscribe to token appear/disappear events

Source: `z_sub_liveliness.rs`.

```rust
use zenoh::sample::SampleKind;

let subscriber = session
    .liveliness()
    .declare_subscriber("group1/**")
    .history(true)   // also deliver Put events for tokens already alive at subscribe time
    .await
    .unwrap();

while let Ok(sample) = subscriber.recv_async().await {
    match sample.kind() {
        SampleKind::Put    => println!("NEW  token: {}", sample.key_expr().as_str()),
        SampleKind::Delete => println!("GONE token: {}", sample.key_expr().as_str()),
    }
}
```

`Liveliness::declare_subscriber(key_expr) -> LivelinessSubscriberBuilder` with `.history(bool)`
and `.callback(..)` / `.background()` like a normal subscriber. The samples are ordinary
`Sample`s; only `key_expr()` and `kind()` are meaningful.

### Query currently-alive tokens (enumerate on startup)

Source: `z_get_liveliness.rs`.

```rust
let replies = session
    .liveliness()
    .get("group1/**")
    .timeout(std::time::Duration::from_secs(10))
    .await
    .unwrap();

while let Ok(reply) = replies.recv_async().await {
    match reply.result() {
        Ok(sample) => println!("alive: {}", sample.key_expr().as_str()),
        Err(err)   => println!("error: {:?}", err.payload().to_bytes()),
    }
}
```

`Liveliness::get(key_expr) -> LivelinessGetBuilder` with `.timeout(..)`.

**Discovery pattern:** on startup, `get` the whole discovery keyspace to learn the current
graph, then keep a `declare_subscriber(..).history(true)` running to receive incremental
changes. Encode all discovery data in the token key expr (and/or a token attachment, if you
declare the token with metadata) so a plain liveliness subscriber reconstructs the ROS graph.

---

## 8. QoS-ish knobs & transient-local (durability)

### Per-publisher / per-put QoS

Source: `z_pub_thr.rs`, `z_ping.rs`. Types under `zenoh::qos`.

```rust
use zenoh::qos::{CongestionControl, Priority, Reliability};

let publisher = session
    .declare_publisher("demo/topic")
    .congestion_control(CongestionControl::Block)  // ::Drop (default) | ::Block
    .priority(Priority::RealTime)                   // 8 levels; ::DEFAULT == Data
    .express(true)                                  // true = don't batch, lower latency
    .reliability(Reliability::Reliable)             // ::Reliable | ::BestEffort  (unstable)
    .await
    .unwrap();

// The same knobs can be set per-put:
publisher.put(data)
    .congestion_control(CongestionControl::Drop)
    .priority(Priority::InteractiveHigh)
    .express(false)
    .await
    .unwrap();
```

- `CongestionControl` — <https://docs.rs/zenoh/latest/zenoh/qos/enum.CongestionControl.html>
  `Drop` (shed messages when queues fill — best-effort-ish) vs `Block` (apply backpressure).
- `Priority` — <https://docs.rs/zenoh/latest/zenoh/qos/enum.Priority.html>
  `RealTime`, `InteractiveHigh`, `InteractiveLow`, `DataHigh`, `Data` (`DEFAULT`), `DataLow`,
  `Background`. Convertible from `u8`.
- `express(bool)` — bypass batching for latency (at throughput cost).
- `Reliability` (on subscriber/publisher, **unstable**) — `Reliable` vs `BestEffort`.
  <https://docs.rs/zenoh/latest/zenoh/qos/enum.Reliability.html>

Map ROS 2 QoS roughly: reliable→`Reliability::Reliable`+`CongestionControl::Block`,
best-effort→`Reliability::BestEffort`+`CongestionControl::Drop`.

### Transient-local / durability → `zenoh-ext` Advanced pub/sub

Zenoh's core pub/sub is *volatile* (a late subscriber misses prior samples). ROS 2
`TRANSIENT_LOCAL` (and history depth) is provided by **`zenoh-ext`**, which in 1.x is the
`AdvancedPublisher` / `AdvancedSubscriber` API (it supersedes the older
`PublicationCache` / `QueryingSubscriber` names from earlier 1.x; those types may still exist
but the advanced builders are the recommended surface).

**Publisher side** (caches samples so late/recovering subscribers can fetch them). Source
`zenoh-ext/examples/examples/z_advanced_pub.rs`. **Requires `timestamping/enabled = true`** in
config.

```rust
use std::time::Duration;
use zenoh_ext::{AdvancedPublisherBuilderExt, CacheConfig, MissDetectionConfig};

let publisher = session
    .declare_publisher("demo/example/topic")
    .cache(CacheConfig::default().max_samples(10))   // keep last N (history depth)
    .sample_miss_detection(
        MissDetectionConfig::default().heartbeat(Duration::from_millis(500)))
    .publisher_detection()   // announce this publisher via liveliness (discoverable)
    .await
    .unwrap();

publisher.put(b"data".to_vec()).await.unwrap();
```

**Subscriber side** (queries history on join, recovers missed samples). Source
`z_advanced_sub.rs`.

```rust
use zenoh_ext::{AdvancedSubscriberBuilderExt, HistoryConfig, RecoveryConfig};

let subscriber = session
    .declare_subscriber("demo/example/**")
    .history(HistoryConfig::default().detect_late_publishers()) // pull cached history -> transient_local
    .recovery(RecoveryConfig::default().heartbeat())            // recover missed samples
    .subscriber_detection()                                     // discoverable via liveliness
    .await
    .unwrap();

// Optional: get notified of detected gaps
let miss_listener = subscriber.sample_miss_listener().await.unwrap();

loop {
    tokio::select! {
        s = subscriber.recv_async() => if let Ok(sample) = s { /* ... */ },
        m = miss_listener.recv_async() => if let Ok(miss) = m {
            println!("missed {} from {:?}", miss.nb(), miss.source());
        },
    }
}
```

- Crate: `zenoh-ext` (features `["unstable"]`). Docs: <https://docs.rs/zenoh-ext/latest/zenoh_ext/>
- The `AdvancedPublisherBuilderExt` / `AdvancedSubscriberBuilderExt` traits add `.cache()`,
  `.sample_miss_detection()`, `.publisher_detection()` (pub) and `.history()`, `.recovery()`,
  `.subscriber_detection()` (sub) onto the *normal* `declare_publisher` / `declare_subscriber`
  builders — bring the trait into scope with `use zenoh_ext::...`.
- `zenoh-ext` also provides `z_serialize` / `z_deserialize` for typed ZBytes (numbers, Vec,
  HashMap, tuples) — see section 9.

---

## 9. Encoding & ZBytes

### Encoding

`zenoh::bytes::Encoding` — metadata hint, not enforced. Docs:
<https://docs.rs/zenoh/latest/zenoh/bytes/struct.Encoding.html>

```rust
use zenoh::bytes::Encoding;
Encoding::ZENOH_BYTES;         // default, raw bytes
Encoding::ZENOH_STRING;        // utf-8 string
Encoding::TEXT_PLAIN;
Encoding::APPLICATION_JSON;
Encoding::APPLICATION_CBOR;
Encoding::APPLICATION_PROTOBUF;
Encoding::APPLICATION_OCTET_STREAM;
// Custom / with schema suffix:
let enc: Encoding = Encoding::from("application/ros2msg");
let enc2 = Encoding::APPLICATION_PROTOBUF.with_schema("my.Type");
```

### ZBytes construction and back to bytes

Type `zenoh::bytes::ZBytes`. Source `z_bytes.rs`. Docs:
<https://docs.rs/zenoh/latest/zenoh/bytes/struct.ZBytes.html>

```rust
use zenoh::bytes::ZBytes;

// From &[u8]  -> copies:
let a = ZBytes::from(b"raw bytes".as_slice());
// From Vec<u8> -> moves (no copy):
let b = ZBytes::from(vec![1u8, 2, 3]);
// From &str / String:
let c = ZBytes::from("text");
let d = ZBytes::from("text".to_string());
// From an iterator of u8:
let e: ZBytes = (0..256u32).map(|i| (i % 10) as u8).collect::<Vec<u8>>().into();

// Back to bytes (infallible, borrows when contiguous):
let raw: std::borrow::Cow<[u8]> = b.to_bytes();
let owned: Vec<u8> = b.to_bytes().into_owned();     // force a Vec<u8>
// Back to string (fallible):
let s: std::borrow::Cow<str> = c.try_to_string().unwrap();

// Streaming writer / reader for zero-ish-copy assembly:
use std::io::{Read, Write};
let mut w = ZBytes::writer();
w.write_all(&[0u8, 1]).unwrap();
w.append(ZBytes::from([2, 3]));
let zb = w.finish();                 // ZBytes == [0,1,2,3]
let mut r = zb.reader();
let mut buf = [0u8; 2];
r.read_exact(&mut buf).unwrap();
```

Typed serialization via `zenoh-ext` (length-prefixed, self-describing for the supported set):

```rust
use zenoh_ext::{z_serialize, z_deserialize};
let payload: ZBytes = z_serialize(&1234u32);
let n: u32 = z_deserialize(&payload).unwrap();
// also Vec<T>, HashMap<K,V>, tuples, arrays, and primitive numerics.
```

For ROS 2 you'll typically ship the **CDR-serialized message** as a raw `Vec<u8>` payload
(`ZBytes::from(cdr_vec)`) and set an appropriate `Encoding`, rather than using `z_serialize`.

---

## 10. Gotchas

1. **Runtime.** Zenoh runs its own internal tokio runtime for I/O, but the *async* API
   (`.await`) must be driven by *your* runtime — wrap `main` in `#[tokio::main]` or build a
   runtime. The blocking `.wait()` API (trait `zenoh::Wait`) works anywhere but can deadlock if
   called from *inside* a tokio worker; prefer `.await` when already async.

2. **Keep the `Session` alive.** `Session` is `Clone` (cheap `Arc`). If the last handle drops,
   the session closes and all its publishers/subscribers/queryables/tokens stop. Store it in
   your RMW context struct for the whole process lifetime. Call `session.close().await` on
   shutdown to flush.

3. **Declarations must be stored — or explicitly backgrounded.** `declare_publisher`,
   `declare_subscriber`, `declare_queryable`, and `declare_token` return handles whose `Drop`
   **undeclares** them. If you write `let _ = session.declare_subscriber(...)...;` it is dropped
   immediately and receives nothing. Either bind it to a named variable you keep, or call
   `.background()` on the builder to detach its lifetime from the handle. Same for
   `LivelinessToken`: dropping the token removes it (emits `Delete`).

4. **Undeclaring.** Prefer explicit `token.undeclare().await` / `subscriber.undeclare().await`
   for deterministic, graceful teardown (and to control ordering of `Delete` events) rather
   than relying on `Drop`, which runs synchronously and can't be `.await`ed.

5. **Callbacks run on zenoh threads.** A `.callback(..)` closure executes on zenoh's internal
   I/O thread. Keep it fast and non-blocking; do not block or call `.wait()` inside it. Offload
   heavy work to a channel. The stream (`recv_async`) style runs on *your* task instead.

6. **Volatile by default.** Plain pub/sub does not retain history; a subscriber that joins
   after a `put` misses it. Use `zenoh-ext` Advanced pub/sub (section 8) for
   transient-local/history, and enable `timestamping/enabled = true` in config (the cache needs
   timestamps).

7. **`unstable` feature.** Liveliness, matching listeners, `Reliability` selection, and the
   `zenoh-ext` advanced APIs are gated behind `features = ["unstable"]` on both `zenoh` and
   `zenoh-ext`. Missing it produces confusing "method not found" errors.

8. **KeyExpr validation is at declaration/parse time.** Passing a `&str` builds and validates a
   `KeyExpr` lazily; malformed keys (empty chunks, leading/trailing `/`, `*` not filling a
   chunk) fail there, not at compile time. Validate untrusted topic strings up front.

9. **Wildcard subtlety:** `*` is exactly one chunk, `**` is zero-or-more. `a/**` matches `a`
   itself. Use `intersects()` (overlap) vs `includes()` (superset) deliberately when routing.

10. **Reply key must match the query.** In a queryable, `query.reply(key, ..)` must use a key
    expression that the original query's selector matches, or the reply is dropped. Reusing the
    queryable's own key expr (as in `z_queryable.rs`) is the safe default.

---

## Quick source index

| Concern | Example file | Doc |
|---|---|---|
| Publish | `examples/examples/z_pub.rs`, `z_put.rs` | [Publisher](https://docs.rs/zenoh/latest/zenoh/pubsub/struct.Publisher.html) |
| Subscribe | `z_sub.rs`, `z_sub_thr.rs`, `z_pull.rs` | [Subscriber](https://docs.rs/zenoh/latest/zenoh/pubsub/struct.Subscriber.html) |
| Query client | `z_get.rs` | [Session::get](https://docs.rs/zenoh/latest/zenoh/struct.Session.html#method.get) |
| Queryable server | `z_queryable.rs` | [Query](https://docs.rs/zenoh/latest/zenoh/query/struct.Query.html) |
| Liveliness token | `z_liveliness.rs` | [Liveliness](https://docs.rs/zenoh/latest/zenoh/liveliness/struct.Liveliness.html) |
| Liveliness sub | `z_sub_liveliness.rs` | " |
| Liveliness get | `z_get_liveliness.rs` | " |
| QoS knobs | `z_pub_thr.rs`, `z_ping.rs` | [qos](https://docs.rs/zenoh/latest/zenoh/qos/index.html) |
| ZBytes/Encoding | `z_bytes.rs` | [bytes](https://docs.rs/zenoh/latest/zenoh/bytes/index.html) |
| Advanced pub/sub | `zenoh-ext/examples/examples/z_advanced_pub.rs`, `z_advanced_sub.rs` | [zenoh_ext](https://docs.rs/zenoh-ext/latest/zenoh_ext/) |
| Config helper | `examples/src/lib.rs` (`CommonArgs`) | [Config](https://docs.rs/zenoh/latest/zenoh/config/struct.Config.html) |

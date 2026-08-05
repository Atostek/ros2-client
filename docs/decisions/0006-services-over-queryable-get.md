# 6. Services over Zenoh queryable/get; `ServiceMapping` is a DDS-only concept

- Status: accepted
- Date: 2026-07-07

## Context

`ros2-client` services use request (`rq/…Request`) and reply (`rr/…Reply`) DDS
topics, with correlation via `RmwRequestId` (≡ RTPS `SampleIdentity` =
`GUID`+`SequenceNumber`). Three `ServiceMapping`s (Basic/Enhanced/Cyclone) encode
this differently on the wire; `Enhanced` (the default, used by parameter
services) relies on RTPS **inline-QoS `related_sample_identity`**, which has no
Zenoh equivalent.

`rmw_zenoh` implements services with a Zenoh **queryable** (server) and
**querier/get** (client): request/response ride the query/reply channel, and
correlation uses the message **attachment** `(seq, ts, client_gid)`. The server
keys pending queries by `hash(client_gid) → seq` and echoes seq+gid in the reply.

## Decision

Under `zenoh`, implement services with queryable/get exactly as `rmw_zenoh` does:

- `Server`: `declare_queryable(key).complete(true)`, a pending-query map keyed by
  `hash(client_gid) → sequence_number`, replies via `query.reply(...)` echoing
  the request's seq + client gid and a fresh timestamp.
- `Client`: `declare_querier(key)`/`get` with `target = ALL_COMPLETE`,
  `consolidation = None`; a per-client monotonic sequence number (from 1);
  attachment `(seq, ts, client_gid)`; correlate replies by the echoed seq.
- The public `Client`/`Server` API and `RmwRequestId` shape are preserved, but
  `writer_guid` becomes the client **entity GID** and `sequence_number` a
  client-local counter.
- `ServiceMapping` becomes a **no-op** under `zenoh` (Zenoh has exactly one
  mapping). The type is kept for API compatibility and documented as `dds`-only;
  the parameter services stop caring which mapping is requested.
- `get_result` action services get an effectively infinite querier timeout
  (key intersects `**/_action/get_result/**`).

## Consequences

- **Pro:** correct, interoperable services without RTPS inline QoS; one code path
  instead of three mappings; actions and parameters inherit it for free.
- **Con:** `ServiceMapping` selection is silently ignored under `zenoh` (a
  documented compromise). `RmwRequestId`'s field meaning changes across backends.
- Correlation now lives entirely in the attachment (payload-adjacent), which is
  simpler and matches the ground truth.

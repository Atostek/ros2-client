# Zenoh backend study for `ros2-client`

This folder is the technical study behind adding **Zenoh** support to
`ros2-client` (issue [#71](https://github.com/Atostek/ros2-client/issues/71)),
alongside the existing RustDDS backend, as a mutually-exclusive cargo feature.

## Executive summary

`ros2-client` is a generic, serde-based ROS 2 client on top of RustDDS. Adding a
Zenoh backend is feasible **without broad renames** by using compile-time backend
selection (`dds` default / `zenoh` opt-in) and introducing a small set of owned
public types where RustDDS currently leaks through the API. The message wire
format (CDR) and the ROS naming convention are shared and port directly; the
RTPS-specific machinery (GUID identity, `SampleIdentity` RPC correlation, DDS
discovery, DDS QoS) is rebuilt over Zenoh following the official `rmw_zenoh`
design:

- **Transport model:** one `zenoh::Session` per `Context` (peer mode; a `zenohd`
  router is required by default for discovery).
- **Topics → key expressions:** `<domain>/<name>/<type>/<type_hash>`.
- **Discovery → liveliness tokens** under `@ros2_lv/<domain>/**` + a graph cache.
- **Messages carry an attachment** `(seq, source_timestamp, source_gid)`.
- **Services → Zenoh queryable/get**; correlation via the attachment, not RTPS.
- **GID → XXH3-128** of the entity's liveliness key.
- **Actions → services + topics** (no special Zenoh handling).

The single biggest **interop risk** is the REP-2016 **type hash** in key
expressions: `ros2-client` doesn't compute one today. The MVP resolves this with
liberal wildcard-receive plus a known-types hash table, deferring full IDL-based
hashing (see ADR-0007).

## How to read this study

| Document | Purpose |
|----------|---------|
| [`feature_map.md`](feature_map.md) | The core: every `ros2-client` feature mapped to its ROS 2 concept, the `rmw_zenoh` Zenoh realisation, the mismatch, and the refactor it implies. |
| [`refactoring_plan.md`](refactoring_plan.md) | The abstraction design (compile-time backend selection), the dependency graph, and the ordered work items **E0–E10** that become GitHub issues. |
| [`test_plan.md`](test_plan.md) | Lean 3-tier test strategy (unit wire-format / in-process integration / real-ROS-2 interop) and the CI design under a < 30 min budget. |
| [`research/`](research/) | Raw ground-truth references gathered up front (appendices). |

### Research appendices
- [`research/rmw_zenoh.md`](research/rmw_zenoh.md) — the official ROS 2 ↔ Zenoh
  middleware: exact key-expression/liveliness/attachment/service/QoS/GID formats.
- [`research/zenoh_api.md`](research/zenoh_api.md) — the Zenoh 1.x Rust API
  (session/pub/sub/queryable/get/liveliness/attachments/`zenoh-ext`).
- [`research/ros2_client_internals.md`](research/ros2_client_internals.md) — a
  feature-by-feature map of the current crate and every RustDDS coupling point.
- [`research/workshop_use_cases.md`](research/workshop_use_cases.md) — the RosCon
  2025 workshop use cases → testable scenarios.

## Decision records

Design decisions and compromises are recorded as ADRs under
[`../decisions/`](../decisions/). Start at
[`0001-record-architecture-decisions.md`](../decisions/0001-record-architecture-decisions.md).

## Status

Study, plan, test plan, and decision records: **complete**. Implementation is
tracked by the E0–E10 GitHub issues generated from
[`refactoring_plan.md`](refactoring_plan.md).

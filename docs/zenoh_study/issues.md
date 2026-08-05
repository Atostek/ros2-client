# Planned GitHub issues (ready to create)

> These were meant to be created directly on `semio-ai/ros2-client`, but the
> session's GitHub access is currently **read-only** (issue creation and content
> writes both return `403 Resource not accessible by integration`, and
> `git push` returns `403` via the git proxy). They are captured here so they can
> be created verbatim once write access is granted (see the note at the bottom).
>
> Full detail for each item is in [`refactoring_plan.md`](refactoring_plan.md);
> acceptance/tests in [`test_plan.md`](test_plan.md).

Suggested labels: `zenoh`, `enhancement`. Suggested milestone: `Zenoh MVP`.

---

## Epic: Zenoh backend for ros2-client (issue #71)

Body: link the study (`docs/zenoh_study/`) and decisions (`docs/decisions/`),
the approach (ADR-0002), and the dependency graph below with checkboxes for
E0–E10. Reference upstream `Atostek/ros2-client#71`.

```
E0 scaffolding ──┬── E1 owned types ──┐
                 ├── E2 wire prims ────┼── E4 pub/sub ──┐
                 └── E3 session ───────┤                ├── E6 services ──┬── E7 actions
                                       └── E5 discovery ─┘                 ├── E8 params
                                              (E4 also feeds E9 rosout)    └── E9 rosout
```

## E0 — Feature scaffolding: `dds` (default) / `zenoh` (opt-in, exclusive)
- Depends on: —
- Add `dds`/`zenoh` features; `default=["dds"]`; `compile_error!` if both/neither;
  gate all `rustdds` deps/uses behind `dds`; add `zenoh`/`zenoh-ext` optional deps;
  both configs build (`cargo check --no-default-features --features {dds,zenoh}`);
  CI matrix builds both. Zenoh path may be `todo!()` stubs behind a clear boundary.

## E1 — Owned public types (decouple from RustDDS)
- Depends on: E0
- `qos::QosProfile`, owned timestamp, `error` enums, backend-neutral `NodeEvent`;
  `dds` `From/Into` RustDDS; keep `ros2::` aliases; DDS tests unchanged. (ADR-0004)

## E2 — Zenoh wire primitives: keyexpr / attachment / type-hash / gid / CDR
- Depends on: E0
- Data + liveliness key builders (`%` mangling, `<qos>` delta encoding);
  attachment `(i64 seq,i64 ts,[u8;16] gid)` via `zenoh_ext::z_serialize`;
  type-hash known-table + wildcard-receive; XXH3-128 gid; CDR encapsulation helper.
  Unit tests pin exact strings/bytes from `research/rmw_zenoh.md`. (ADR-0003/7/8)

## E3 — Zenoh session / Context + config
- Depends on: E0
- `ContextInner` opens a `zenoh::Session` (peer defaults ≈
  `DEFAULT_RMW_ZENOH_SESSION_CONFIG.json5`); honour `ZENOH_SESSION_CONFIG_URI`/
  `ZENOH_CONFIG_OVERRIDE`; retain `domain_id`; document router requirement. (ADR-0009)

## E4 — Pub/Sub over Zenoh
- Depends on: E1, E2, E3
- `Publisher`/`Subscription` zenoh internals; volatile put+attachment / subscriber
  stream; transient-local via `zenoh-ext` cache+history; `MessageInfo` from
  attachment; congestion control for keep_all+reliable.
- Interop (Tier C1–C3): `ros2 topic echo/pub/list` round-trips.

## E5 — Discovery: liveliness tokens + graph cache
- Depends on: E2, E3
- Per-entity liveliness tokens; graph-cache subscriber on `@ros2_lv/<domain>/**` +
  initial get; reimplement `wait_for_*` and `get_*_count` over the cache;
  backend-neutral events. (ADR-0005)
- Interop: `ros2 node list` shows the node; graph sees C++ talker.

## E6 — Services over queryable / get
- Depends on: E4, E5
- Server queryable(`complete`) + pending map `hash(gid)→seq`; client querier/get
  (`ALL_COMPLETE`, `consolidation=None`); attachment correlation; `RmwRequestId`
  provenance change; `ServiceMapping` no-op under zenoh; `get_result` long timeout. (ADR-0006)
- Interop (C4/C5): `add_two_ints` both directions.

## E7 — Actions over Zenoh
- Depends on: E6
- Verify actions via services+topics; add `**/_action/get_result/**` long timeout.
- Interop (C6/C7): `Fibonacci` both directions.

## E8 — Parameters over Zenoh
- Depends on: E6
- Param services + `parameter_events` topic with owned timestamp.
- Interop (C8): `ros2 param set/get`.

## E9 — rosout logging over Zenoh
- Depends on: E4
- `rt/rosout` key publisher with owned timestamp; optional inbound sub.
- Interop (C9): `ros2 topic echo /rosout`.

## E10 — Docs / examples / README / interop matrix
- Depends on: E4–E9 (incremental)
- README Zenoh section, router requirement, interop matrix; examples build under
  `zenoh`; finalise ADRs; migration notes.

## (Follow-up, post-MVP) — Compute RIHS01 type hashes from IDL in `msggen`
- Depends on: E2
- Full REP-2016 type-hash computation for arbitrary types (see ADR-0007c),
  removing the known-types-table limitation for the send direction.

---

### How to create these once write access is granted
The GitHub App installation for this session lacks `issues: write` /
`contents: write` / `pull_requests: write` on `semio-ai/ros2-client`. An admin can
grant it at <https://claude.ai/admin-settings> (Claude GitHub settings) or by
adjusting the GitHub App's repository permissions. Then these issues can be created
from this file (or directly from `refactoring_plan.md`).

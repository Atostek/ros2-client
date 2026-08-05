# 2. Dual backend via compile-time feature selection

- Status: accepted
- Date: 2026-07-07
- Updated: 2026-08-05
- Relates to: issue #71, ADR-0004, ADR-0010, `docs/zenoh_study/refactoring_plan.md`

## Context

`ros2-client` must support two middlewares — RustDDS (today) and Zenoh (new).
Issue #71 proposed a default `dds` feature and an opt-in `zenoh` feature. The
maintainer wants to avoid broad renames early on, keep churn small for the first
landing, and use **compile-time** gating for optional dependencies and backend
code.

A stricter reading treated the backends as **mutually exclusive forever** (never
in the same build). That is too tight: applications and tools should eventually
be able to use **both DDS and Zenoh from one library build** (e.g. bridge
processes, multi-transport nodes, or tests). Mutual exclusion may be a
convenient **interim** constraint while the Zenoh path and owned public types
mature, not a permanent product rule.

Design options considered:

- **A. Runtime trait-object abstraction** (`trait Middleware` with associated
  entity types; `Context`/`Node` generic over it).
- **B. Compile-time feature gating** via `#[cfg(feature = "dds")]` /
  `#[cfg(feature = "zenoh")]`, with shared owned public types (ADR-0004,
  ADR-0010) and backend-specific internals.
- **C. Permanent mutual exclusion** — at most one of `dds` / `zenoh` per build.

## Decision

Adopt **B, compile-time feature selection**, without locking in **C**.

- Features `dds` and `zenoh` gate optional dependencies (`rustdds` vs
  `zenoh`/`zenoh-ext`/…) and backend implementation modules.
- `default = ["dds", …]` for the familiar DDS-only experience; `zenoh` is
  opt-in.
- **Interim (MVP / current branch):** builds may still require exactly one
  backend (`compile_error!` if both or neither) so the first Zenoh landing
  stays simple and crate-root types do not collide.
- **Direction:** allow **both** `dds` and `zenoh` in the same build. When both
  are enabled:
  - Shared ROS-shaped types (`QosProfile`, owned time / errors / graph types,
    …) remain common.
  - Backend entity APIs (`Context`, `Node`, `Publisher`, …) are disambiguated
    (e.g. `ros2_client::dds::…` and `ros2_client::zenoh::…`, or another
    explicit scheme)—not by forbidding the feature combination.
- Prefer designing owned types and module layout so lifting the exclusivity
  `compile_error!` does not force another public-API rewrite (see ADR-0010).
- A full runtime `dyn Middleware` abstraction is **not** required for MVP; it
  remains optional later if dual-backend apps need a single type-erased handle.

## Consequences

- **Pro:** optional deps stay lean; zero cost when a backend is disabled;
  room for single-backend apps *and* future dual-backend binaries; aligns with
  API convergence (ADR-0010).
- **Con:** interim exclusivity means dual-backend apps wait; lifting it needs
  clear namespacing (or generics) for entity types; CI must grow from
  `{dds}` / `{zenoh}` to also cover `{dds,zenoh}` once allowed.
- Documentation must distinguish **current** feature rules from the **intended**
  “both allowed” end state so dependents do not treat exclusivity as permanent.

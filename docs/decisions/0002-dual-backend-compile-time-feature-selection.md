# 2. Dual backend via compile-time feature selection

- Status: accepted
- Date: 2026-07-07
- Relates to: issue #71, `docs/zenoh_study/refactoring_plan.md`

## Context

`ros2-client` must support two middlewares — RustDDS (today) and Zenoh (new) —
that are never used together in one build. Issue #71 proposes a default `dds`
feature and an opt-in, **mutually exclusive** `zenoh` feature. The maintainer
explicitly wants to avoid broad renames/refactoring and keep the change minimal.

Two designs were considered:

- **A. Runtime trait-object abstraction** (`trait Middleware` with associated
  entity types; `Context`/`Node` generic over it).
- **B. Compile-time selection** via `#[cfg(feature = "dds")]` /
  `#[cfg(feature = "zenoh")]`, one non-generic public API, backend-specific
  internals, plus a few owned public types.

## Decision

Adopt **B, compile-time feature selection**.

- `default = ["dds"]`; `zenoh` is opt-in. A `compile_error!` fires if both or
  neither backend feature is enabled.
- All `rustdds` dependencies and code are gated behind `dds`; `zenoh`/`zenoh-ext`
  are optional deps gated behind `zenoh`.
- The DDS code path stays byte-for-byte as-is; Zenoh support is additive
  (`#[cfg]` branches in `context.rs`/`node.rs`/`pubsub.rs`/`service/*` plus new
  `src/zenoh_backend/*` modules).

## Consequences

- **Pro:** matches the mutually-exclusive-feature reality; zero runtime cost; no
  generics threaded through the public API; the DDS path is untouched and its
  tests keep passing; smallest possible churn.
- **Con:** the two backends can't coexist in a single binary (acceptable —
  that's the requirement). `#[cfg]` duplication in a few files. Documentation and
  CI must cover both feature configurations (added to the CI matrix).
- Because the backends are exclusive, no `dyn`/vtable abstraction is warranted;
  shared behaviour is expressed as ordinary functions/types selected by `cfg`.

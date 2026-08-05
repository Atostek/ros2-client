# 1. Record architecture decisions

- Status: accepted
- Date: 2026-07-07
- Updated: 2026-08-05 (revision policy clarified)

## Context

Adding a Zenoh backend to `ros2-client` (issue #71) is a large, multi-phase
change with several irreversible design choices and compromises (feature
architecture, public-API changes, wire-format interop trade-offs). The
maintainer asked that "every compromise / design decision" be compiled into a
reviewable set of records.

## Decision

We keep lightweight Architecture Decision Records (ADRs) in `docs/decisions/`,
one file per decision, numbered sequentially, in a MADR-lite format
(Context / Decision / Consequences). Each substantive design choice or
compromise made while implementing the Zenoh backend gets an ADR. The technical
rationale lives in `docs/zenoh_study/`; the ADRs capture the *decisions* and
their trade-offs so they can be reviewed independently of the code.

## Consequences

- Reviewers can audit decisions without reading every diff.
- Numbering is global across the project, not per-feature.
- **While a decision is not yet implemented (or only on an experimental
  branch), ADRs may be revised in place** and stamped with an `Updated:` date.
  Once behaviour has shipped on `master` and dependents may rely on it, prefer
  append-only changes: a new ADR that supersedes the old one, rather than
  silently rewriting history.
- A later decision that reverses an implemented one adds a new record and marks
  the old one superseded.

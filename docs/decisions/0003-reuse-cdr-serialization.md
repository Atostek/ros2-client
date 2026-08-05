# 3. Reuse CDR serialization via `cdr-encoding`

- Status: accepted
- Date: 2026-07-07

## Context

Over DDS, messages are CDR-encoded through RustDDS writer/reader adapters. A
Zenoh backend has no DataWriter/Reader but must produce **byte-identical** CDR
payloads (including the 4-byte CDR encapsulation header) because `rmw_zenoh`
carries the same CDR bytes as the Zenoh payload.

## Decision

Serialize/deserialize messages for the Zenoh path with the standalone
[`cdr-encoding`](https://lib.rs/crates/cdr-encoding) crate
(`to_vec::<M, LittleEndian>()` / `from_bytes::<M, LittleEndian>()`), and prepend
the 4-byte encapsulation header (`00 01 00 00` for CDR_LE) via a small shared
helper. `cdr-encoding` is already a transitive dependency and is authored by the
RustDDS author, maximising the chance of byte parity.

A unit test (Tier A7) pins the encapsulation-header handling and a first live
pub/sub round-trip confirms header presence/duplication.

## Consequences

- **Pro:** no need to pull RustDDS under the `zenoh` feature; the same serde
  `Message` types work unchanged; minimal new code.
- **Con:** a dependency on `cdr-encoding` matching RustDDS's CDR output exactly —
  guarded by unit tests and interop tests. If a discrepancy appears (e.g. header
  emitted by `to_vec` vs added by us), the helper is the single place to fix it.
- XCDR2/`RIHS`-driven type support is out of scope; plain CDR (XCDR1) only, which
  is what the current DDS path and the interop targets use.

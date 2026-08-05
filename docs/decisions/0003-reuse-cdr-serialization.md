# 3. Reuse CDR serialization via `cdr-encoding`

- Status: accepted
- Date: 2026-07-07
- Updated: 2026-08-05
- Relates to: ADR-0004, RustDDS / `cdr-encoding` evolution

## Context

Over DDS, messages are CDR-encoded through RustDDS writer/reader adapters. A
Zenoh backend has no DataWriter/Reader but must produce **byte-identical** CDR
payloads (including the 4-byte CDR encapsulation header) because `rmw_zenoh`
carries the same CDR bytes as the Zenoh payload.

ROS 2 / DDS also define **XCDR2** (and DataRepresentation QoS). RustDDS is
gaining representation awareness; the standalone [`cdr-encoding`] crate may
grow XCDR2 support later. The Zenoh path should not paint itself into an
XCDR1-only corner in the public or helper API.

[`cdr-encoding`]: https://lib.rs/crates/cdr-encoding

## Decision

Serialize/deserialize messages for the Zenoh path with the standalone
`cdr-encoding` crate (`to_vec` / `from_bytes` with an explicit endianness), and
apply the encapsulation header via a small shared helper. Prefer the same
`cdr-encoding` lineage as RustDDS so byte parity stays realistic.

**MVP encoding:** plain CDR / **XCDR1** with CDR_LE encapsulation
(`00 01 00 00`), matching current DDS defaults and `rmw_zenoh` interop targets.

**Future room for XCDR2:**

- Keep representation choice out of call sites that can stay representation-
  agnostic; concentrate header + encode/decode in the helper (or a thin
  `Representation` / encoding selector).
- When `cdr-encoding` (and interop peers) support XCDR2, extend the helper and
  QoS/DataRepresentation mapping rather than replacing the whole stack.
- Do not hard-code “XCDR1 forever” into public types; document MVP as XCDR1
  without closing the door on XCDR2.

Unit tests (Tier A7) pin encapsulation-header handling; live pub/sub confirms
header presence/duplication for the MVP representation.

## Consequences

- **Pro:** no need to pull RustDDS under a zenoh-only build; the same serde
  `Message` types work unchanged; a single place to evolve representation
  support.
- **Con:** MVP is still XCDR1-only until toolchain and peers are ready; must
  track `cdr-encoding` / RustDDS representation APIs; tests must eventually
  cover more than one representation when XCDR2 lands.
- Interop with `rmw_zenoh` remains XCDR1 until that ecosystem advertises
  otherwise; enabling XCDR2 is a coordinated change, not an silent default flip.

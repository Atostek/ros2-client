# 8. GID via XXH3-128 and attachment byte parity

- Status: accepted
- Date: 2026-07-07

## Context

For Zenoh interop, two byte-level formats must match `rmw_zenoh` exactly:

1. **GID** — `rmw_zenoh` derives the 16-byte entity GID as the 128-bit XXH3 hash
   of the entity's liveliness key string: `gid[0..8] = low64`,
   `gid[8..16] = high64`, each in host (little-endian) order. It uses a
   self-contained "simplified" XXH3-128 for cross-version stability.
2. **Attachment** — every put/request/response carries a `zenoh-ext`-serialized
   tuple `(i64 sequence, i64 source_timestamp_ns, [u8;16] source_gid)`. The
   `zenoh-ext` serializer length-prefixes the array, so the layout is
   `8B seq | 8B ts | 1B len(=16) | 16B gid`, not a naive concatenation.

## Decision

- Compute the Zenoh GID as XXH3-128 (seed 0) of the liveliness key string using
  the [`xxhash-rust`](https://crates.io/crates/xxhash-rust) `xxh3` implementation,
  writing `low64` then `high64` as little-endian bytes. A unit test pins the
  result against a known vector derived from a sample key.
- Serialize/deserialize the attachment with `zenoh_ext::z_serialize` /
  `z_deserialize` (the Rust counterpart of zenoh-cpp `ext::Serializer`) to get the
  length-prefixed framing for free, rather than hand-rolling bytes. A unit test
  pins the exact byte layout.
- Both are validated by live interop (a `ros2-client` node and a C++ peer must see
  each other's GIDs and correlate service replies).

## Consequences

- **Pro:** byte-for-byte interop with `rmw_zenoh`; correlation and graph identity
  work across the C++/Rust boundary.
- **Con:** depends on `xxhash-rust`'s XXH3-128 matching `rmw_zenoh`'s "simplified"
  implementation and on `zenoh-ext`'s Rust/C++ serializers agreeing. Both are
  guarded by unit + interop tests; if a mismatch is found, the isolated
  `gid`/`attachment` modules are the single place to correct it.

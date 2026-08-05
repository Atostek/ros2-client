//! Zenoh-backend entity GID derivation.
//!
//! Per `docs/decisions/0008-gid-and-attachment-byte-parity.md` and
//! `rmw_zenoh` `liveliness_utils.cpp`, the 16-byte RMW GID of an entity is the
//! 128-bit XXH3 hash of that entity's liveliness key string, laid out as
//! `gid[0..8] = low64`, `gid[8..16] = high64`, each in little-endian order.
//!
//! `u128::to_le_bytes()` produces exactly that layout (least-significant 8
//! bytes = `low64` LE, next 8 = `high64` LE), so the mapping is a single call.
//!
//! Byte-for-byte parity with `rmw_zenoh`'s "simplified" XXH3-128 is asserted by
//! live interop; this module fixes the layout and determinism.

use xxhash_rust::xxh3::xxh3_128;

/// Number of bytes in a Zenoh-backend GID (RMW `RMW_GID_STORAGE_SIZE`).
pub const GID_LEN: usize = 16;

/// Compute the 16-byte entity GID from its liveliness key expression string.
pub fn gid_from_liveliness_key(liveliness_key: &str) -> [u8; GID_LEN] {
  xxh3_128(liveliness_key.as_bytes()).to_le_bytes()
}

/// Lowercase hex rendering of a GID, matching the `Gid` `Debug` format.
pub fn gid_hex(gid: &[u8; GID_LEN]) -> String {
  let mut s = String::with_capacity(GID_LEN * 2);
  for b in gid {
    s.push_str(&format!("{b:02x}"));
  }
  s
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn deterministic_and_16_bytes() {
    let key = "@ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/0/NN/%/%/listener";
    let a = gid_from_liveliness_key(key);
    let b = gid_from_liveliness_key(key);
    assert_eq!(a, b);
    assert_eq!(a.len(), 16);
    // Distinct keys yield distinct GIDs (overwhelmingly likely).
    assert_ne!(gid_from_liveliness_key("a"), gid_from_liveliness_key("b"));
  }

  #[test]
  fn byte_layout_is_low64_then_high64_le() {
    // Independently reconstruct the rmw layout (low64 LE || high64 LE) and
    // confirm it equals our single to_le_bytes() call.
    let key = "some/liveliness/key";
    let h = xxh3_128(key.as_bytes());
    let low = (h & 0xFFFF_FFFF_FFFF_FFFF) as u64;
    let high = (h >> 64) as u64;
    let mut expect = [0u8; 16];
    expect[0..8].copy_from_slice(&low.to_le_bytes());
    expect[8..16].copy_from_slice(&high.to_le_bytes());
    assert_eq!(gid_from_liveliness_key(key), expect);
  }

  #[test]
  fn hex_rendering() {
    let gid = [0x00, 0x01, 0x0a, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(gid_hex(&gid).starts_with("00010aff"));
    assert_eq!(gid_hex(&gid).len(), 32);
  }
}

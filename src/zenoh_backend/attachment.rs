//! Message / request / response attachment (E2).
//!
//! Every Zenoh publication, service request, and service reply carries an
//! attachment with three fields, in this order (see
//! `docs/zenoh_study/research/rmw_zenoh.md` §4 and
//! `docs/decisions/0008-gid-and-attachment-byte-parity.md`):
//!
//! 1. sequence number — `i64`
//! 2. source timestamp — `i64`, nanoseconds since the UNIX epoch
//! 3. source GID — 16-byte array
//!
//! The bytes are produced with the `zenoh-ext` serializer (the Rust counterpart
//! of zenoh-cpp's `ext::Serializer`), which follows the Zenoh serialization
//! RFC: integers are fixed little-endian, and the array is written with a
//! varint length prefix. For the 16-byte GID that prefix is the single byte
//! `0x10`, so the on-wire layout is:
//!
//! ```text
//! | seq: 8B LE | source_ts: 8B LE | len=0x10 | gid: 16B |   (33 bytes)
//! ```
//!
//! which is byte-for-byte what `rmw_zenoh` emits.

use zenoh::bytes::ZBytes;
use zenoh_ext::{ZDeserializer, ZSerializer};

/// The three attachment fields carried alongside every message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachmentData {
  /// Per-publisher / per-client monotonic sequence number.
  pub sequence_number: i64,
  /// Source timestamp, nanoseconds since the UNIX epoch.
  pub source_timestamp: i64,
  /// 16-byte GID of the originating publisher / client.
  pub source_gid: [u8; 16],
}

/// Failure to decode an [`AttachmentData`] from received bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentError;

impl std::fmt::Display for AttachmentError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "malformed ROS 2 / Zenoh message attachment")
  }
}
impl std::error::Error for AttachmentError {}

impl AttachmentData {
  /// Serialize to a Zenoh attachment `ZBytes`, byte-compatible with
  /// `rmw_zenoh`.
  pub fn to_zbytes(self) -> ZBytes {
    let mut s = ZSerializer::new();
    s.serialize(self.sequence_number);
    s.serialize(self.source_timestamp);
    s.serialize(self.source_gid);
    s.finish()
  }

  /// Decode from a received Zenoh attachment.
  pub fn from_zbytes(zbytes: &ZBytes) -> Result<Self, AttachmentError> {
    let mut d = ZDeserializer::new(zbytes);
    let sequence_number = d.deserialize::<i64>().map_err(|_| AttachmentError)?;
    let source_timestamp = d.deserialize::<i64>().map_err(|_| AttachmentError)?;
    let source_gid = d.deserialize::<[u8; 16]>().map_err(|_| AttachmentError)?;
    Ok(Self {
      sequence_number,
      source_timestamp,
      source_gid,
    })
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn wire_layout_matches_rmw_zenoh() {
    let a = AttachmentData {
      sequence_number: 7,
      source_timestamp: 0x0102_0304_0506_0708,
      source_gid: [0xAB; 16],
    };
    let zbytes = a.to_zbytes();
    let bytes = zbytes.to_bytes();

    let mut expected = Vec::new();
    expected.extend_from_slice(&7i64.to_le_bytes());
    expected.extend_from_slice(&0x0102_0304_0506_0708i64.to_le_bytes());
    expected.push(0x10); // varint length prefix of the 16-byte array
    expected.extend_from_slice(&[0xAB; 16]);

    assert_eq!(bytes.as_ref(), expected.as_slice());
    assert_eq!(bytes.len(), 33);
  }

  #[test]
  fn round_trips() {
    let a = AttachmentData {
      sequence_number: 42,
      source_timestamp: 1_700_000_000_000_000_000,
      source_gid: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    };
    let decoded = AttachmentData::from_zbytes(&a.to_zbytes()).unwrap();
    assert_eq!(a, decoded);
  }

  #[test]
  fn rejects_truncated() {
    let short = ZBytes::from(vec![1u8, 2, 3]);
    assert_eq!(AttachmentData::from_zbytes(&short), Err(AttachmentError));
  }
}

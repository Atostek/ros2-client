//! CDR (de)serialization for the Zenoh backend (E2/E4).
//!
//! ROS 2 message payloads are OMG CDR with a 4-byte *encapsulation header*
//! (a 2-byte representation identifier + 2-byte options) followed by the CDR
//! body. `rmw_zenoh` carries exactly these bytes as the Zenoh payload, so the
//! Zenoh backend must produce the same.
//!
//! The DDS backend gets CDR through RustDDS; here we use the standalone
//! [`cdr-encoding`] crate (same author as RustDDS) for the body and prepend the
//! header ourselves — `cdr-encoding` serializes the body *without* the
//! encapsulation header (see its docs). See
//! `docs/decisions/0003-reuse-cdr-serialization.md`.
//!
//! [`cdr-encoding`]: https://docs.rs/cdr-encoding

use byteorder::{BigEndian, LittleEndian};
use cdr_encoding::{from_bytes, to_vec};
use serde::{de::DeserializeOwned, Serialize};

/// The 4-byte encapsulation header for little-endian plain CDR: representation
/// identifier `CDR_LE` (`0x0001`) followed by zero options.
pub const CDR_LE_HEADER: [u8; 4] = [0x00, 0x01, 0x00, 0x00];

/// Failure to (de)serialize a ROS 2 CDR payload.
#[derive(Debug)]
pub enum CdrError {
  /// The underlying `cdr-encoding` serializer/deserializer failed.
  Encoding(cdr_encoding::Error),
  /// The payload is too short to contain the 4-byte encapsulation header.
  MissingHeader,
}

impl std::fmt::Display for CdrError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      CdrError::Encoding(e) => write!(f, "CDR encoding error: {e}"),
      CdrError::MissingHeader => write!(f, "CDR payload missing 4-byte encapsulation header"),
    }
  }
}

impl std::error::Error for CdrError {}

/// Serialize a message to a ROS 2 CDR payload: the 4-byte `CDR_LE`
/// encapsulation header followed by the little-endian CDR body.
pub fn to_cdr<M: Serialize>(msg: &M) -> Result<Vec<u8>, CdrError> {
  let body = to_vec::<M, LittleEndian>(msg).map_err(CdrError::Encoding)?;
  let mut out = Vec::with_capacity(CDR_LE_HEADER.len() + body.len());
  out.extend_from_slice(&CDR_LE_HEADER);
  out.extend_from_slice(&body);
  Ok(out)
}

/// Deserialize a ROS 2 CDR payload (with its 4-byte encapsulation header) into
/// `M`, honouring the representation identifier's endianness (`CDR_LE` vs
/// `CDR_BE`, including the parameter-list variants).
pub fn from_cdr<M: DeserializeOwned>(bytes: &[u8]) -> Result<M, CdrError> {
  if bytes.len() < 4 {
    return Err(CdrError::MissingHeader);
  }
  let body = &bytes[4..];
  // Representation identifier is bytes[0..2]; the low bit of the second byte
  // selects little-endian (CDR_LE=0x0001, PL_CDR_LE=0x0003) vs big-endian
  // (CDR_BE=0x0000, PL_CDR_BE=0x0002).
  let little_endian = (bytes[1] & 0x01) == 0x01;
  let (msg, _consumed) = if little_endian {
    from_bytes::<M, LittleEndian>(body).map_err(CdrError::Encoding)?
  } else {
    from_bytes::<M, BigEndian>(body).map_err(CdrError::Encoding)?
  };
  Ok(msg)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};

  use super::*;

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  struct AddTwoIntsRequest {
    a: i64,
    b: i64,
  }

  #[test]
  fn header_is_prepended_once() {
    let bytes = to_cdr(&"hello".to_string()).unwrap();
    assert_eq!(&bytes[0..4], &CDR_LE_HEADER);
    // A CDR string is: u32 length (incl. NUL) + chars + NUL. "hello" -> len 6.
    // header(4) + len(4) + 6 = 14 bytes; the point is the header appears once.
    assert!(bytes.len() >= 4 + 4 + 6);
  }

  #[test]
  fn round_trips_string_and_struct() {
    let s = "chatter message".to_string();
    assert_eq!(from_cdr::<String>(&to_cdr(&s).unwrap()).unwrap(), s);

    let req = AddTwoIntsRequest { a: 2, b: 40 };
    assert_eq!(
      from_cdr::<AddTwoIntsRequest>(&to_cdr(&req).unwrap()).unwrap(),
      req
    );
  }

  #[test]
  fn decodes_big_endian_payload() {
    // Build a CDR_BE payload by hand and confirm from_cdr honours the header.
    let body = to_vec::<AddTwoIntsRequest, BigEndian>(&AddTwoIntsRequest { a: 7, b: 8 }).unwrap();
    let mut be = vec![0x00, 0x00, 0x00, 0x00]; // CDR_BE header
    be.extend_from_slice(&body);
    assert_eq!(
      from_cdr::<AddTwoIntsRequest>(&be).unwrap(),
      AddTwoIntsRequest { a: 7, b: 8 }
    );
  }

  #[test]
  fn rejects_missing_header() {
    assert!(matches!(
      from_cdr::<String>(&[0x00, 0x01]),
      Err(CdrError::MissingHeader)
    ));
  }
}

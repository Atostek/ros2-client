//! ROS 2 global identifier ([`Gid`]).
//!
//! Backend-neutral 16- or 24-byte entity id (distro-gated). DDS maps to/from
//! RustDDS [`GUID`](rustdds::GUID); Zenoh uses the first 16 bytes of the
//! liveliness-key XXH3 hash (ADR-0008).

use std::fmt;

use serde::{Deserialize, Serialize};

// The Gid definition changed between Humble and Iron, so `iron` (which is
// enabled by every newer distribution feature) is the threshold: iron-or-newer
// uses the 16-byte format, galactic/humble use the older 24-byte format.
#[cfg(feature = "iron")]
pub const GID_LENGTH: usize = 16;
#[cfg(not(feature = "iron"))]
pub const GID_LENGTH: usize = 24;

/// ROS 2 equivalent for DDS GUID / rmw GID.
///
/// See <https://github.com/ros2/rmw_dds_common/blob/master/rmw_dds_common/msg/Gid.msg>
///
/// Size is selected by the distribution feature chain (see Cargo.toml):
/// `galactic`/`humble` → 24 bytes; `iron` or newer → 16 bytes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "dds", derive(rustdds::dds::key::CdrEncodingSize))]
pub struct Gid([u8; GID_LENGTH]);

impl Gid {
  /// All-zero GID.
  pub const ZERO: Self = Self([0u8; GID_LENGTH]);

  /// Raw bytes (length is [`GID_LENGTH`]).
  pub fn as_bytes(&self) -> &[u8; GID_LENGTH] {
    &self.0
  }

  /// First 16 bytes for Zenoh / modern rmw GIDs.
  ///
  /// On 24-byte (pre-Iron) builds, only the leading 16 bytes are returned; the
  /// trailing 8 padding bytes used on the wire for older ROS distros are
  /// dropped at this boundary.
  pub fn to_bytes16(self) -> [u8; 16] {
    let mut out = [0u8; 16];
    let n = GID_LENGTH.min(16);
    out[..n].copy_from_slice(&self.0[..n]);
    out
  }
}

impl Default for Gid {
  fn default() -> Self {
    Self::ZERO
  }
}

impl fmt::Debug for Gid {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for b in self.0.iter() {
      write!(f, "{b:02x}")?;
    }
    Ok(())
  }
}

impl From<[u8; 16]> for Gid {
  fn from(bytes: [u8; 16]) -> Self {
    let mut arr = [0u8; GID_LENGTH];
    let n = GID_LENGTH.min(16);
    arr[..n].copy_from_slice(&bytes[..n]);
    Gid(arr)
  }
}

#[cfg(feature = "dds")]
mod dds_conv {
  use rustdds::{GUID, dds::key::Key};

  use super::{GID_LENGTH, Gid};

  impl From<GUID> for Gid {
    fn from(guid: GUID) -> Self {
      Gid(std::array::from_fn(|i| {
        *guid.to_bytes().as_ref().get(i).unwrap_or(&0)
      }))
    }
  }

  impl From<Gid> for GUID {
    fn from(gid: Gid) -> GUID {
      GUID::from_bytes(std::array::from_fn(|i| {
        if i < GID_LENGTH { gid.0[i] } else { 0 }
      }))
    }
  }

  impl Key for Gid {}
}

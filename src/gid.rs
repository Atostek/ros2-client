use std::fmt;

use serde::{Deserialize, Serialize};
use rustdds::{dds::key::CdrEncodingSize, *};

// The Gid definition changed between Humble and Iron, so `iron` (which is
// enabled by every newer distribution feature) is the threshold: iron-or-newer
// uses the 16-byte format, galactic/humble use the older 24-byte format.
#[cfg(feature = "iron")]
pub const GID_LENGTH: usize = 16;
#[cfg(not(feature = "iron"))]
pub const GID_LENGTH: usize = 24;

/// ROS2 equivalent for DDS GUID
///
/// See https://github.com/ros2/rmw_dds_common/blob/master/rmw_dds_common/msg/Gid.msg
///
/// Gid definition has changed in ROS 2 from 24 bytes to 16 bytes in Jan 2023
/// https://github.com/ros2/rmw_dds_common/commit/5ab4f5944e4442fe0188e15b10cf11377fb45801
///             
/// This is between Humble (May 2022) and Iron (May 2023)
///
/// The size is selected by the distribution feature chain (see Cargo.toml):
/// build against `galactic` or `humble` for the old 24-byte format, or any
/// `iron`-or-newer distribution for the 16-byte format.           
#[derive(
  Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, CdrEncodingSize,
)]
pub struct Gid([u8; GID_LENGTH]);

impl fmt::Debug for Gid {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for b in self.0.iter() {
      write!(f, "{b:02x}")?;
    }
    Ok(())
  }
}

impl From<GUID> for Gid {
  fn from(guid: GUID) -> Self {
    Gid(std::array::from_fn(|i| {
      *guid.to_bytes().as_ref().get(i).unwrap_or(&0)
    }))
  }
}

impl From<Gid> for GUID {
  fn from(gid: Gid) -> GUID {
    GUID::from_bytes(std::array::from_fn(|i| gid.0[i]))
  }
}

impl Key for Gid {}

//! Owned ROS 2 request identity ([`RmwRequestId`]).
//!
//! Backend-neutral replacement for DDS `SampleIdentity` / Zenoh
//! `(client_gid, seq)` pairs (ADR-0010 Phase 2).

use serde::{Deserialize, Serialize};

use crate::gid::Gid;

/// Identifies a service request: the client's GID plus its sequence number.
///
/// [rmw_request_id_t](https://docs.ros2.org/foxy/api/rmw/structrmw__request__id__t.html)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RmwRequestId {
  /// GID of the requesting client (historically `writer_guid` on DDS).
  pub writer_gid: Gid,
  /// Client-assigned request sequence number.
  pub sequence_number: i64,
}

#[cfg(feature = "dds")]
mod dds_conv {
  use rustdds::{rpc::SampleIdentity, SequenceNumber, GUID};

  use super::RmwRequestId;
  use crate::gid::Gid;

  impl From<RmwRequestId> for SampleIdentity {
    fn from(
      RmwRequestId {
        writer_gid,
        sequence_number,
      }: RmwRequestId,
    ) -> SampleIdentity {
      SampleIdentity {
        writer_guid: GUID::from(writer_gid),
        sequence_number: SequenceNumber::from(sequence_number),
      }
    }
  }

  impl From<SampleIdentity> for RmwRequestId {
    fn from(
      SampleIdentity {
        writer_guid,
        sequence_number,
      }: SampleIdentity,
    ) -> RmwRequestId {
      RmwRequestId {
        writer_gid: Gid::from(writer_guid),
        sequence_number: i64::from(sequence_number),
      }
    }
  }
}

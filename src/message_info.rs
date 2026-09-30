//! Metadata for received `Message`s (source time, sequence, publisher GID).
//!
//! Backend-neutral (ADR-0010 Phase 2). DDS and Zenoh adapters populate the
//! same type; fields that a backend cannot supply are `None`.

use crate::{gid::Gid, request_id::RmwRequestId, ros_time::ROSTime};

/// Message metadata attached to a received sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInfo {
  received_timestamp: Option<ROSTime>,
  source_timestamp: Option<ROSTime>,
  sequence_number: i64,
  publisher_gid: Gid,
  related_request_id: Option<RmwRequestId>,
}

impl MessageInfo {
  /// Construct from owned fields (used by backend adapters).
  pub fn new(
    received_timestamp: Option<ROSTime>,
    source_timestamp: Option<ROSTime>,
    sequence_number: i64,
    publisher_gid: Gid,
    related_request_id: Option<RmwRequestId>,
  ) -> Self {
    Self {
      received_timestamp,
      source_timestamp,
      sequence_number,
      publisher_gid,
      related_request_id,
    }
  }

  /// When this process received the sample, if known.
  pub fn received_timestamp(&self) -> Option<ROSTime> {
    self.received_timestamp
  }

  /// Source timestamp set by the publisher, if present.
  pub fn source_timestamp(&self) -> Option<ROSTime> {
    self.source_timestamp
  }

  /// Per-publisher sequence number.
  pub fn sequence_number(&self) -> i64 {
    self.sequence_number
  }

  /// Publishing entity GID.
  pub fn publisher_gid(&self) -> Gid {
    self.publisher_gid
  }

  /// Related request id (e.g. Enhanced service mapping / RPC correlation).
  pub fn related_request_id(&self) -> Option<RmwRequestId> {
    self.related_request_id
  }
}

#[cfg(feature = "dds")]
mod dds_conv {
  use std::convert::TryFrom;

  use rustdds::*;

  use super::MessageInfo;
  use crate::{gid::Gid, request_id::RmwRequestId, ros_time::ROSTime};

  // `Timestamp::ZERO` is the Unix epoch, a valid time; only `INVALID` means
  // "no timestamp".
  fn timestamp_to_ros(ts: Timestamp) -> Option<ROSTime> {
    TryFrom::try_from(ts).ok()
  }

  impl From<&SampleInfo> for MessageInfo {
    fn from(sample_info: &SampleInfo) -> MessageInfo {
      MessageInfo {
        // received time not available from SampleInfo today
        received_timestamp: None,
        source_timestamp: sample_info.source_timestamp().and_then(timestamp_to_ros),
        sequence_number: i64::from(sample_info.sample_identity().sequence_number),
        publisher_gid: Gid::from(sample_info.publication_handle()),
        related_request_id: sample_info
          .related_sample_identity()
          .map(RmwRequestId::from),
      }
    }
  }

  impl<M> From<&rustdds::no_key::DeserializedCacheChange<M>> for MessageInfo {
    fn from(dcc: &rustdds::no_key::DeserializedCacheChange<M>) -> MessageInfo {
      MessageInfo {
        // `receive_instant` is the local reception time RustDDS stamps on each
        // incoming sample.
        received_timestamp: timestamp_to_ros(dcc.receive_instant),
        source_timestamp: dcc.source_timestamp().and_then(timestamp_to_ros),
        sequence_number: i64::from(dcc.sequence_number),
        publisher_gid: Gid::from(dcc.writer_guid()),
        related_request_id: dcc.related_sample_identity().map(RmwRequestId::from),
      }
    }
  }

  // sample_identity helper removed — use RmwRequestId fields directly

  #[cfg(test)]
  mod tests {
    use rustdds::Timestamp;

    use super::timestamp_to_ros;
    use crate::ros_time::ROSTime;

    #[test]
    fn epoch_is_a_valid_timestamp() {
      assert_eq!(timestamp_to_ros(Timestamp::ZERO), Some(ROSTime::UNIX_EPOCH));
    }

    #[test]
    fn invalid_and_infinite_are_missing() {
      assert_eq!(timestamp_to_ros(Timestamp::INVALID), None);
      assert_eq!(timestamp_to_ros(Timestamp::INFINITE), None);
    }
  }
}

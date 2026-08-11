//! Metadata for received `Message`s, such as `Timestamp`s and publisher id.
use rustdds::{rpc::SampleIdentity, *};

/// Message metadata
#[derive(Debug, Clone)]
pub struct MessageInfo {
  received_timestamp: Timestamp,
  source_timestamp: Option<Timestamp>,
  sequence_number: SequenceNumber,
  publisher: GUID,
  related_sample_identity: Option<SampleIdentity>,
}

impl MessageInfo {
  pub fn received_timestamp(&self) -> Timestamp {
    self.received_timestamp
  }

  pub fn source_timestamp(&self) -> Option<Timestamp> {
    self.source_timestamp
  }

  pub fn writer_guid(&self) -> GUID {
    self.publisher
  }

  pub fn sample_identity(&self) -> rustdds::rpc::SampleIdentity {
    rustdds::rpc::SampleIdentity {
      writer_guid: self.writer_guid(),
      sequence_number: self.sequence_number,
    }
  }

  pub fn related_sample_identity(&self) -> Option<SampleIdentity> {
    self.related_sample_identity
  }
}

impl<M> From<&rustdds::no_key::DeserializedCacheChange<M>> for MessageInfo {
  fn from(dcc: &rustdds::no_key::DeserializedCacheChange<M>) -> MessageInfo {
    MessageInfo {
      // `receive_instant` is the local reception time RustDDS stamps on each
      // incoming sample.
      received_timestamp: dcc.receive_instant,
      source_timestamp: dcc.source_timestamp(),
      sequence_number: dcc.sequence_number,
      publisher: dcc.writer_guid(),
      related_sample_identity: dcc.related_sample_identity(),
    }
  }
}

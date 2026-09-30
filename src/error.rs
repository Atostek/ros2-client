//! Owned operation errors shared by DDS and Zenoh backends (ADR-0010 Phase 3).
//!
//! Public APIs return these types (and the [`CreateResult`] / [`ReadResult`] /
//! [`WriteResult`] / [`WaitResult`] aliases) instead of `rustdds::dds::*` or
//! `zenoh::Result`. Middleware errors are mapped at the adapter boundary.

use std::{error::Error as StdError, fmt, io};

/// Failure to create a ROS 2 entity (`Context`, `Node`, topic endpoints, …).
#[derive(Debug)]
pub enum CreateError {
  /// A required resource was already dropped.
  ResourceDropped { reason: String },
  /// Synchronization / background worker failure.
  Poisoned { reason: String },
  /// I/O failure during creation.
  Io(io::Error),
  /// Invalid argument or configuration.
  BadParameter { reason: String },
  /// Allocation or capacity limit.
  OutOfResources { reason: String },
  /// Unexpected internal failure.
  Internal { reason: String },
  /// Unclassified middleware / backend failure.
  Middleware { reason: String },
}

impl fmt::Display for CreateError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::ResourceDropped { reason } => {
        write!(f, "create failed, resource dropped: {reason}")
      }
      Self::Poisoned { reason } => write!(f, "create failed, poisoned: {reason}"),
      Self::Io(e) => write!(f, "create I/O error: {e}"),
      Self::BadParameter { reason } => write!(f, "create bad parameter: {reason}"),
      Self::OutOfResources { reason } => write!(f, "create out of resources: {reason}"),
      Self::Internal { reason } => write!(f, "create internal error: {reason}"),
      Self::Middleware { reason } => write!(f, "create middleware error: {reason}"),
    }
  }
}

impl StdError for CreateError {
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    match self {
      Self::Io(e) => Some(e),
      _ => None,
    }
  }
}

impl From<io::Error> for CreateError {
  fn from(e: io::Error) -> Self {
    Self::Io(e)
  }
}

/// Result of entity-creation operations.
pub type CreateResult<T> = Result<T, CreateError>;

/// Failure to read / take a sample.
#[derive(Debug)]
pub enum ReadError {
  /// Payload could not be decoded.
  Deserialization { reason: String },
  /// Synchronization / background worker failure.
  Poisoned { reason: String },
  /// Unexpected internal failure.
  Internal { reason: String },
  /// Receive channel / reader closed.
  Closed,
  /// Message metadata (e.g. attachment) was malformed.
  Malformed,
  /// Unclassified middleware / backend failure.
  Middleware { reason: String },
}

impl fmt::Display for ReadError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Deserialization { reason } => write!(f, "read deserialization error: {reason}"),
      Self::Poisoned { reason } => write!(f, "read poisoned: {reason}"),
      Self::Internal { reason } => write!(f, "read internal error: {reason}"),
      Self::Closed => write!(f, "read: channel closed"),
      Self::Malformed => write!(f, "read: malformed sample metadata"),
      Self::Middleware { reason } => write!(f, "read middleware error: {reason}"),
    }
  }
}

impl StdError for ReadError {}

/// Result of read / take operations.
pub type ReadResult<T> = Result<T, ReadError>;

/// Failure to write / publish a sample.
///
/// Some variants retain the payload `D` so callers can retry (e.g.
/// [`WouldBlock`](WriteError::WouldBlock)).
#[derive(Debug)]
pub enum WriteError<D> {
  /// Serialization of the payload failed.
  Serialization { reason: String, data: D },
  /// Synchronization / background worker failure.
  Poisoned { reason: String, data: D },
  /// I/O failure during write.
  Io(io::Error),
  /// Would block / timed out while writing.
  WouldBlock { data: D },
  /// Unexpected internal failure.
  Internal { reason: String },
  /// Unclassified middleware / backend failure.
  Middleware { reason: String, data: D },
}

impl<D> fmt::Display for WriteError<D> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Serialization { reason, .. } => write!(f, "write serialization error: {reason}"),
      Self::Poisoned { reason, .. } => write!(f, "write poisoned: {reason}"),
      Self::Io(e) => write!(f, "write I/O error: {e}"),
      Self::WouldBlock { .. } => write!(f, "write would block / timed out"),
      Self::Internal { reason } => write!(f, "write internal error: {reason}"),
      Self::Middleware { reason, .. } => write!(f, "write middleware error: {reason}"),
    }
  }
}

impl<D: fmt::Debug> StdError for WriteError<D> {
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    match self {
      Self::Io(e) => Some(e),
      _ => None,
    }
  }
}

impl From<io::Error> for WriteError<()> {
  fn from(e: io::Error) -> Self {
    Self::Io(e)
  }
}

impl<D> WriteError<D> {
  /// Drop retained payload data (useful when `D` is not needed).
  pub fn forget_data(self) -> WriteError<()> {
    match self {
      Self::Serialization { reason, data: _ } => WriteError::Serialization { reason, data: () },
      Self::Poisoned { reason, data: _ } => WriteError::Poisoned { reason, data: () },
      Self::Io(e) => WriteError::Io(e),
      Self::WouldBlock { data: _ } => WriteError::WouldBlock { data: () },
      Self::Internal { reason } => WriteError::Internal { reason },
      Self::Middleware { reason, data: _ } => WriteError::Middleware { reason, data: () },
    }
  }
}

/// Result of write / publish operations.
pub type WriteResult<T, D> = Result<T, WriteError<D>>;

/// Failure while waiting for a condition.
#[derive(Debug)]
pub enum WaitError {
  /// The wait timed out.
  Timeout,
  /// Unclassified middleware / backend failure.
  Middleware { reason: String },
}

impl fmt::Display for WaitError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Timeout => write!(f, "wait timed out"),
      Self::Middleware { reason } => write!(f, "wait middleware error: {reason}"),
    }
  }
}

impl StdError for WaitError {}

/// Result of wait operations.
pub type WaitResult<T> = Result<T, WaitError>;

/// Failure in service call / response handling (Zenoh services; shared shape).
#[derive(Debug)]
pub enum ServiceError {
  /// CDR (de)serialization failed.
  Cdr { reason: String },
  /// Unclassified middleware / backend failure.
  Middleware { reason: String },
  /// Request/reply lacked expected payload or attachment.
  Malformed,
  /// Client received no reply.
  NoReply,
  /// `send_response` referenced an unknown request id.
  UnknownRequest,
}

impl fmt::Display for ServiceError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Cdr { reason } => write!(f, "service CDR error: {reason}"),
      Self::Middleware { reason } => write!(f, "service middleware error: {reason}"),
      Self::Malformed => write!(f, "service: malformed request/reply"),
      Self::NoReply => write!(f, "service: no reply received"),
      Self::UnknownRequest => write!(f, "service: unknown request id"),
    }
  }
}

impl StdError for ServiceError {}

/// Result of service call / response operations.
pub type ServiceResult<T> = Result<T, ServiceError>;

// ---------------------------------------------------------------------------
// DDS adapters
// ---------------------------------------------------------------------------

#[cfg(feature = "dds")]
mod dds_conv {
  use super::*;

  impl From<rustdds::dds::CreateError> for CreateError {
    fn from(e: rustdds::dds::CreateError) -> Self {
      use rustdds::dds::CreateError as R;
      match e {
        R::ResourceDropped { reason } => Self::ResourceDropped { reason },
        R::Poisoned { reason } => Self::Poisoned { reason },
        R::Io(io) => Self::Io(io),
        R::TopicKind(kind) => Self::BadParameter {
          reason: format!("wrong topic kind, expected {kind:?}"),
        },
        R::Internal { reason } => Self::Internal { reason },
        R::BadParameter { reason } => Self::BadParameter { reason },
        R::OutOfResources { reason } => Self::OutOfResources { reason },
        #[cfg(feature = "security")]
        R::NotAllowedBySecurity { reason } => Self::Middleware {
          reason: format!("not allowed by security: {reason}"),
        },
      }
    }
  }

  impl From<rustdds::dds::ReadError> for ReadError {
    fn from(e: rustdds::dds::ReadError) -> Self {
      use rustdds::dds::ReadError as R;
      match e {
        R::Deserialization { reason } => Self::Deserialization { reason },
        R::UnknownKey { details } => Self::Deserialization {
          reason: format!("unknown key: {details}"),
        },
        R::Poisoned { reason } => Self::Poisoned { reason },
        R::Internal { reason } => Self::Internal { reason },
      }
    }
  }

  impl<D> From<rustdds::dds::WriteError<D>> for WriteError<D> {
    fn from(e: rustdds::dds::WriteError<D>) -> Self {
      use rustdds::dds::WriteError as R;
      match e {
        R::Serialization { reason, data } => Self::Serialization { reason, data },
        R::Poisoned { reason, data } => Self::Poisoned { reason, data },
        R::Io(io) => Self::Io(io),
        R::WouldBlock { data } => Self::WouldBlock { data },
        R::Internal { reason } => Self::Internal { reason },
      }
    }
  }

  impl From<rustdds::dds::WaitError> for WaitError {
    fn from(e: rustdds::dds::WaitError) -> Self {
      match e {
        rustdds::dds::WaitError::Timeout => Self::Timeout,
      }
    }
  }

  // RustDDS's CDR (de)serialization error, surfaced e.g. by
  // `rustdds::serialization::{deserialize_from_cdr_with_rep_id,
  // to_writer_with_rep_id}` used directly by `crate::service::wrappers`.
  impl From<rustdds::serialization::Error> for ReadError {
    fn from(e: rustdds::serialization::Error) -> Self {
      Self::Deserialization {
        reason: e.to_string(),
      }
    }
  }

  impl From<rustdds::serialization::Error> for WriteError<()> {
    fn from(e: rustdds::serialization::Error) -> Self {
      Self::Serialization {
        reason: e.to_string(),
        data: (),
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Zenoh adapters
// ---------------------------------------------------------------------------

#[cfg(feature = "zenoh")]
mod zenoh_conv {
  use super::*;
  use crate::zenoh_backend::cdr::CdrError;

  impl From<zenoh::Error> for CreateError {
    fn from(e: zenoh::Error) -> Self {
      Self::Middleware {
        reason: e.to_string(),
      }
    }
  }

  impl From<zenoh::Error> for ReadError {
    fn from(e: zenoh::Error) -> Self {
      Self::Middleware {
        reason: e.to_string(),
      }
    }
  }

  impl From<zenoh::Error> for WriteError<()> {
    fn from(e: zenoh::Error) -> Self {
      Self::Middleware {
        reason: e.to_string(),
        data: (),
      }
    }
  }

  impl From<zenoh::Error> for ServiceError {
    fn from(e: zenoh::Error) -> Self {
      Self::Middleware {
        reason: e.to_string(),
      }
    }
  }

  impl From<CdrError> for ReadError {
    fn from(e: CdrError) -> Self {
      Self::Deserialization {
        reason: e.to_string(),
      }
    }
  }

  impl From<CdrError> for WriteError<()> {
    fn from(e: CdrError) -> Self {
      Self::Serialization {
        reason: e.to_string(),
        data: (),
      }
    }
  }

  impl From<CdrError> for ServiceError {
    fn from(e: CdrError) -> Self {
      Self::Cdr {
        reason: e.to_string(),
      }
    }
  }

  /// Map a Zenoh write failure while retaining the message payload.
  pub(crate) fn write_middleware<D>(reason: impl Into<String>, data: D) -> WriteError<D> {
    WriteError::Middleware {
      reason: reason.into(),
      data,
    }
  }

  /// Map a CDR serialization failure while retaining the message payload.
  pub(crate) fn write_serialization<D>(reason: impl Into<String>, data: D) -> WriteError<D> {
    WriteError::Serialization {
      reason: reason.into(),
      data,
    }
  }
}

#[cfg(feature = "zenoh")]
pub(crate) use zenoh_conv::{write_middleware, write_serialization};

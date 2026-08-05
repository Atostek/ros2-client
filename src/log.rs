//! `rosout` logging data types

#[cfg(feature = "dds")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "dds")]
use rustdds::*;

/// Log message structure, communicated over the rosout Topic.
///
/// [Log](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/Log.msg)
///
/// To write log messages, use the [`rosout`](crate::rosout!) macro.
///
/// Currently only available on the `dds` backend (uses a RustDDS `Timestamp`);
/// the `zenoh` backend's rosout support lands with an owned timestamp (E9).
#[cfg(feature = "dds")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
  pub timestamp: Timestamp,
  pub level: u8,
  pub name: String,
  pub msg: String,
  pub file: String,
  pub function: String,
  pub line: u32,
}

#[cfg(feature = "dds")]
impl Log {
  /// ROS2 logging severity level
  pub const DEBUG: u8 = 10;
  pub const INFO: u8 = 20;
  pub const WARN: u8 = 30;
  pub const ERROR: u8 = 40;
  pub const FATAL: u8 = 50;

  /// Timestamp when rosout message was sent
  pub fn get_timestamp(&self) -> &Timestamp {
    &self.timestamp
  }

  /// Rosout level
  pub fn get_level(&self) -> u8 {
    self.level
  }

  /// Name of the rosout message
  pub fn name(&self) -> &str {
    &self.name
  }

  /// Actual message
  pub fn get_msg(&self) -> &str {
    &self.msg
  }

  pub fn get_file(&self) -> &str {
    &self.file
  }

  pub fn get_function(&self) -> &str {
    &self.function
  }

  pub fn get_line(&self) -> u32 {
    self.line
  }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
  Fatal = 50,
  Error = 40,
  Warn = 30,
  Info = 20,
  Debug = 10,
}

//impl From<u8> for Level

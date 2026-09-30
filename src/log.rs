//! `rosout` logging data types
//!
//! Shared by DDS and Zenoh backends. Wire field layout matches
//! [`rcl_interfaces/msg/Log`](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/Log.msg)
//! (`stamp` is [`builtin_interfaces::Time`](crate::builtin_interfaces::Time)).

use serde::{Deserialize, Serialize};

use crate::builtin_interfaces::Time;

/// Log message structure, communicated over the rosout Topic.
///
/// [Log](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/Log.msg)
///
/// To write log messages, use the [`rosout`](crate::rosout!) macro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
  /// When the record was produced (`builtin_interfaces/Time stamp`).
  pub stamp: Time,
  pub level: u8,
  pub name: String,
  pub msg: String,
  pub file: String,
  pub function: String,
  pub line: u32,
}

impl Log {
  /// ROS2 logging severity level
  pub const DEBUG: u8 = 10;
  pub const INFO: u8 = 20;
  pub const WARN: u8 = 30;
  pub const ERROR: u8 = 40;
  pub const FATAL: u8 = 50;

  /// Timestamp when rosout message was sent
  pub fn get_stamp(&self) -> &Time {
    &self.stamp
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

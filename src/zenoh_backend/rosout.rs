//! Zenoh rosout logging (E9).
//!
//! ROS 2 nodes publish log records to the global `/rosout` topic
//! (`rcl_interfaces/msg/Log`). This module provides a [`Logger`] that `put`s
//! those records on the corresponding Zenoh key, plus an optional inbound
//! [`Subscription`] for reading `/rosout` (the `read_rosout` capability).
//!
//! The record carries an owned [`builtin_interfaces::Time`] timestamp, keeping
//! the Zenoh backend independent of RustDDS (ADR-0004); the DDS backend's
//! [`crate::log::Log`] uses `rustdds::Timestamp` instead. The field layout (and
//! thus the CDR wire format) matches `rcl_interfaces/msg/Log`, so
//! `ros2 topic echo /rosout` shows records logged by a `ros2-client` node.

use serde::{Deserialize, Serialize};

use super::pubsub::Publisher;
use crate::{builtin_interfaces::Time, log::LogLevel};

/// A `rcl_interfaces/msg/Log` record with an owned
/// [`builtin_interfaces::Time`] timestamp.
///
/// Field order matches ROS 2's `Log.msg`: `stamp`, `level`, `name`, `msg`,
/// `file`, `function`, `line`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
  /// When the record was produced.
  pub stamp: Time,
  /// Severity level (see [`LogLevel`]).
  pub level: u8,
  /// Name of the logger (usually the node name).
  pub name: String,
  /// The log message.
  pub msg: String,
  /// Source file the record came from.
  pub file: String,
  /// Source function the record came from.
  pub function: String,
  /// Source line the record came from.
  pub line: u32,
}

/// A rosout logger: publishes [`Log`] records to the global `/rosout` topic.
///
/// Create one with [`Node::create_logger`](crate::Node::create_logger). The
/// `name` field of each record defaults to the node's base name. Use the
/// [`rosout!`](crate::rosout!) macro to capture source location automatically,
/// or call [`log_at`](Self::log_at) / the level helpers directly.
pub struct Logger {
  node_name: String,
  publisher: Publisher<Log>,
}

impl Logger {
  pub(crate) fn new(node_name: String, publisher: Publisher<Log>) -> Self {
    Self {
      node_name,
      publisher,
    }
  }

  /// The logger name (the node's base name) stamped into each record.
  pub fn name(&self) -> &str {
    &self.node_name
  }

  /// Publish a log record, specifying the source location explicitly.
  ///
  /// Prefer the [`rosout!`](crate::rosout!) macro, which fills `file`/`line`
  /// from the call site.
  pub fn log_at(&self, level: LogLevel, msg: &str, file: &str, function: &str, line: u32) {
    let record = Log {
      stamp: Time::now(),
      level: level as u8,
      name: self.node_name.clone(),
      msg: msg.to_string(),
      file: file.to_string(),
      function: function.to_string(),
      line,
    };
    if let Err(e) = self.publisher.publish(record) {
      log::warn!("rosout publish failed: {e}");
    }
  }

  /// Log at [`LogLevel::Debug`] (no source location).
  pub fn debug(&self, msg: &str) {
    self.log_at(LogLevel::Debug, msg, "", "", 0);
  }
  /// Log at [`LogLevel::Info`] (no source location).
  pub fn info(&self, msg: &str) {
    self.log_at(LogLevel::Info, msg, "", "", 0);
  }
  /// Log at [`LogLevel::Warn`] (no source location).
  pub fn warn(&self, msg: &str) {
    self.log_at(LogLevel::Warn, msg, "", "", 0);
  }
  /// Log at [`LogLevel::Error`] (no source location).
  pub fn error(&self, msg: &str) {
    self.log_at(LogLevel::Error, msg, "", "", 0);
  }
  /// Log at [`LogLevel::Fatal`] (no source location).
  pub fn fatal(&self, msg: &str) {
    self.log_at(LogLevel::Fatal, msg, "", "", 0);
  }
}

/// Write a record to the `/rosout` topic via a [`Logger`], capturing the call
/// site's file and line.
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "zenoh")]
/// # fn demo(logger: &ros2_client::Logger) {
/// use ros2_client::ros2::LogLevel;
/// let kind = "silly";
/// ros2_client::rosout!(logger, LogLevel::Info, "A {} event was seen.", kind);
/// # }
/// ```
#[cfg(feature = "zenoh")]
#[macro_export]
macro_rules! rosout {
  ($logger:expr, $lvl:expr, $($arg:tt)+) => (
    $crate::Logger::log_at(
      &$logger,
      $lvl,
      &std::format!($($arg)+),
      std::file!(),
      "<unknown_func>",
      std::line!(),
    )
  );
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use zenoh::Config;

  use crate::{ros2::LogLevel, Context, ContextOptions, NodeName, NodeOptions};

  fn make_config(listen_port: u16, connect_port: Option<u16>) -> Config {
    let mut c = Config::default();
    c.insert_json5("mode", "\"peer\"").unwrap();
    c.insert_json5("scouting/multicast/enabled", "false")
      .unwrap();
    c.insert_json5(
      "listen/endpoints",
      &format!("[\"tcp/127.0.0.1:{listen_port}\"]"),
    )
    .unwrap();
    if let Some(p) = connect_port {
      c.insert_json5("connect/endpoints", &format!("[\"tcp/127.0.0.1:{p}\"]"))
        .unwrap();
    }
    c
  }

  #[test]
  fn rosout_publish_and_read() {
    let pub_port = 17525;
    let sub_port = 17526;
    let sub_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(sub_port, None)))
        .unwrap();
    let pub_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(pub_port, Some(sub_port))),
    )
    .unwrap();

    let sub_node = sub_ctx.new_node(NodeName::new("/", "listener").unwrap(), NodeOptions::new());
    let pub_node = pub_ctx.new_node(NodeName::new("/", "talker").unwrap(), NodeOptions::new());

    let reader = sub_node.read_rosout().unwrap();
    let logger = pub_node.create_logger().unwrap();

    // Publish repeatedly until the peers connect and a record arrives.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut got = None;
    while Instant::now() < deadline {
      rosout!(logger, LogLevel::Warn, "hello from {}", "talker");
      if let Some((record, _info)) = reader.try_take().unwrap() {
        got = Some(record);
        break;
      }
      std::thread::sleep(Duration::from_millis(100));
    }

    let record = got.expect("no rosout record received within timeout");
    assert_eq!(record.level, LogLevel::Warn as u8);
    assert_eq!(record.name, "talker");
    assert_eq!(record.msg, "hello from talker");
    assert!(record.file.ends_with("rosout.rs"));
    assert!(record.line > 0);
  }
}

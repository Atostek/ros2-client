use std::sync::Arc;

use crate::{
  log::{Log, LogLevel},
  ros2::Timestamp,
  Publisher,
};

use super::Node;

pub trait RosoutRaw {
  /// Returns the `rosout` writer from `self`.
  fn rosout_writer(&self) -> Arc<Option<Publisher<Log>>>;

  /// Grabs the "base name" of the node.
  fn base_name(&self) -> &str;

  /// Logs to `rosout`.
  #[expect(clippy::too_many_arguments)]
  #[tracing::instrument(skip_all)]
  fn rosout_raw(
    &self,
    timestamp: Timestamp,
    level: impl Into<LogLevel>,
    log_name: &str,
    log_msg: &str,
    source_file: &str,
    source_function: &str,
    source_line: u32,
  ) {
    // get the writer
    let Some(ref rosout_writer) = *self.rosout_writer() else {
      tracing::debug!("Tried to log to `rosout`, but it's not turned on. msg: {log_msg}");
      return;
    };

    // convert log level
    let log_level = Into::<LogLevel>::into(level);

    // publish a log
    _ = rosout_writer
      .publish(Log {
        timestamp,
        level: log_level as u8,
        name: log_name.into(),
        msg: log_msg.into(),
        file: source_file.into(),
        function: source_function.into(),
        line: source_line,
      })
      .inspect_err(|e| tracing::warn!("Failed to publish message to `rosout`! err: {e}"));

    // also output a tracing message
    let tracing_msg = format!("[rosout] ({log_name}) {log_msg}");
    let span = tracing::span!(tracing::Level::ERROR, "rosout_raw");
    let _guard = span.enter();

    match log_level {
      LogLevel::Fatal | LogLevel::Error => tracing::error!("{tracing_msg}"),
      LogLevel::Warn => tracing::warn!("{tracing_msg}"),
      LogLevel::Info => tracing::info!("{tracing_msg}"),
      LogLevel::Debug => tracing::debug!("{tracing_msg}"),
    };
  }
}

/// A "handle" to log from any node without moving it between threads.
pub struct NodeLoggingHandle {
  /// Writes to the `rosout` topic. Shared with all other instances of the
  /// parent node.
  pub(super) rosout_writer: Arc<Option<Publisher<Log>>>,

  /// The base name of the represented node.
  pub base_name: String,
}

impl RosoutRaw for NodeLoggingHandle {
  fn rosout_writer(&self) -> Arc<Option<Publisher<Log>>> {
    Arc::clone(&self.rosout_writer)
  }

  fn base_name(&self) -> &str {
    &self.base_name
  }
}

impl RosoutRaw for Node {
  fn rosout_writer(&self) -> Arc<Option<Publisher<Log>>> {
    Arc::clone(&self.rosout_writer)
  }

  fn base_name(&self) -> &str {
    self.node_name.base_name()
  }
}

#[cfg(test)]
mod tests {
  use crate::{log::LogLevel, rosout, Context, NodeName};

  mod new_namespace {
    use crate::{log::LogLevel, rosout, Context, NodeName};

    #[test]
    fn logging_works_without_import() {
      let ctx = Context::new().unwrap();

      let node = ctx
        .new_node(
          NodeName::new("/", "logging_works_without_import_node").unwrap(),
          Default::default(),
        )
        .unwrap();

      rosout!(node, LogLevel::Warn, "log call works!");
    }

    #[test]
    fn logging_handle_works_without_import() {
      let ctx = Context::new().unwrap();

      let node = ctx
        .new_node(
          NodeName::new("/", "logging_works_without_import_node").unwrap(),
          Default::default(),
        )
        .unwrap();
      let node_logging_handle = node.logging_handle();

      rosout!(node, LogLevel::Warn, "log call works!");

      std::thread::spawn(move || {
        rosout!(node_logging_handle, LogLevel::Debug, "works here too");
      })
      .join()
      .unwrap();
    }
  }

  #[test]
  fn log_across_threads() {
    let ctx = Context::new().unwrap();

    let node = ctx
      .new_node(
        NodeName::new("/", "log_across_threads_node").unwrap(),
        Default::default(),
      )
      .unwrap();

    // log some stuff on two threads at once
    let node_logging_handle = node.logging_handle();
    let handle = std::thread::spawn(move || {
      for i in 0..50 {
        rosout!(
          node_logging_handle,
          LogLevel::Debug,
          "hey! system thread on {i}"
        );
      }
    });

    for j in 0..50 {
      rosout!(node, LogLevel::Debug, "hi! main thread on {j}");
    }

    handle.join().unwrap();
  }
}

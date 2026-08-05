//! Self-contained demo of the **Zenoh** backend of `ros2-client`.
//!
//! Build & run with the `zenoh` backend (the `dds` default off):
//!
//! ```console
//! cargo run --no-default-features --features zenoh --example zenoh_demo
//! ```
//!
//! It starts a talker and a listener in one process, connected directly over
//! IPv4 loopback (Zenoh *peer* mode, multicast disabled, explicit endpoints),
//! so it works without a running Zenoh router. In a real multi-process or
//! multi-host deployment you would instead run a Zenoh router (`zenohd`) — as
//! `rmw_zenoh` does — or configure peer `connect`/`listen` endpoints; see
//! `docs/decisions/0009-zenoh-router-and-config.md`.
//!
//! The listener also reads `/rosout`, and the talker logs there, to show
//! logging on the Zenoh backend.

use std::time::{Duration, Instant};

use ros2_client::{
  ros2::LogLevel, rosout, Context, ContextOptions, MessageTypeName, Name, NodeName, NodeOptions,
  QosProfile,
};
use zenoh::Config;

/// A peer config on IPv4 loopback with multicast off. Pinning `listen` and
/// (optionally) `connect` ports lets two in-process peers connect directly, no
/// router required.
fn loopback_config(listen_port: u16, connect_port: Option<u16>) -> Config {
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

fn main() {
  let talker_port = 17600;
  let listener_port = 17601;

  // Two independent ROS contexts (each wraps its own Zenoh session).
  let talker_ctx =
    Context::with_options(ContextOptions::new().zenoh_config(loopback_config(talker_port, None)))
      .expect("open talker context");
  let listener_ctx = Context::with_options(
    ContextOptions::new().zenoh_config(loopback_config(listener_port, Some(talker_port))),
  )
  .expect("open listener context");

  let talker = talker_ctx.new_node(NodeName::new("/", "talker").unwrap(), NodeOptions::new());
  let listener = listener_ctx.new_node(NodeName::new("/", "listener").unwrap(), NodeOptions::new());

  // A `/chatter` topic carrying `std_msgs/String`.
  let chatter = |node: &ros2_client::Node| {
    node.create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &QosProfile::default(),
    )
  };
  let publisher = talker
    .create_publisher::<String>(&chatter(&talker), None)
    .expect("create publisher");
  let subscription = listener
    .create_subscription::<String>(&chatter(&listener), None)
    .expect("create subscription");

  // rosout: the talker logs, the listener reads `/rosout`.
  let logger = talker.create_logger().expect("create logger");
  let rosout_reader = listener.read_rosout().expect("read rosout");

  println!("Publishing on /chatter (Zenoh backend). Ctrl-C to stop.");

  let mut count = 0;
  let mut received = 0;
  let deadline = Instant::now() + Duration::from_secs(10);
  while Instant::now() < deadline {
    count += 1;
    let msg = format!("Hello ROS 2 over Zenoh #{count}");
    publisher.publish(msg.clone()).expect("publish");
    rosout!(logger, LogLevel::Info, "published: {msg}");

    // Drain whatever has arrived.
    while let Ok(Some((data, info))) = subscription.try_take() {
      received += 1;
      println!(
        "chatter: {data:?}  (seq {}, gid {:02x?}…)",
        info.sequence_number(),
        &info.source_gid()[..4]
      );
    }
    while let Ok(Some((log, _))) = rosout_reader.try_take() {
      println!("/rosout [{}] {}: {}", log.level, log.name, log.msg);
    }

    std::thread::sleep(Duration::from_millis(500));
  }

  println!("Done: sent {count} messages, received {received} on /chatter.");
}

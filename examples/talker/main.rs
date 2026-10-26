use std::time::{Duration, Instant};

use log::error;
use rustdds::mio1::{Events, Poll};
use ros2_client::{
  Context, MessageTypeName, Name, Node, NodeName, NodeOptions, QosProfile,
  qos::{Durability, History, WhenFull},
};
//use core::cmp::min;

// Simple demo program.
// Test this against ROS2 "listener" demo node.
//
// Listener start in ROS2:
// % ros2 run demo_nodes_py listener
//
// Then run this example program to talk to the listener.

fn main() {
  // Here is a fixed path, so this example must be started from
  // RustDDS main directory
  log4rs::init_file("examples/talker/log4rs.yaml", Default::default()).unwrap();

  let mut node = create_node();
  let topic_qos = create_qos();

  // The counterpart message type depends on the ROS 2 distribution: older
  // demo_nodes_cpp use std_msgs/String, while newer ones (>= Lyrical) publish
  // example_interfaces/String. Both are structurally identical (a single
  // `string data` field); DDS matches by type name, so the name must agree.
  // Selected via the distribution feature chain (see Cargo.toml).
  let (type_pkg, type_name) = chatter_type();
  let chatter_topic = node
    .create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new(type_pkg, type_name),
      &topic_qos,
    )
    .unwrap();
  let chatter_publisher = node
    .create_publisher::<String>(&chatter_topic, None)
    .unwrap();

  // Nothing is registered: the poll timeout is the talk timer.
  let talk_period = Duration::from_secs(2);
  let mut next_talk = Instant::now() + talk_period;

  let mut poll = Poll::new().unwrap();
  let mut events = Events::with_capacity(8);
  let mut count = 0;

  let filler: String =
    "All work and no play makes ROS a dull boy. All play and no work makes RTPS a mere toy. "
      .repeat(2);
  // Change repeat count to e.g. 100 to test data fragmentation.

  loop {
    let timeout = next_talk.saturating_duration_since(Instant::now());
    poll.poll(&mut events, Some(timeout)).unwrap();

    if Instant::now() >= next_talk {
      count += 1;
      let message = format!("count={count} {filler}");
      println!("Talking, count={} len={}", count, message.len());
      chatter_publisher
        .publish(message)
        .unwrap_or_else(|e| error!("publish failed: {e:?}"));
      next_talk += talk_period;
    }
  } // loop
} // main

// The /chatter message type of the matching demo_nodes_cpp counterpart:
// example_interfaces/String on Lyrical or newer, std_msgs/String before that.
fn chatter_type() -> (&'static str, &'static str) {
  if cfg!(feature = "lyrical") {
    ("example_interfaces", "String")
  } else {
    ("std_msgs", "String")
  }
}

fn create_qos() -> QosProfile {
  QosProfile::publisher_default()
    .reliability_reliable(WhenFull::DEFAULT)
    .durability(Durability::Volatile)
    .history(History::KeepLast { depth: 10 })
}

fn create_node() -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "talker").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

use ros2_client::{ros2, ros2::policy, Context, MessageTypeName, Name, NodeName, NodeOptions};
use async_io::Timer;

// This test program is like "async_talker", but it also tracks the amount of
// subscribers.

fn main() {
  // Here is a fixed path, so this example must be started from
  // RustDDS main directory
  log4rs::init_file(
    "examples/async_smart_talker/log4rs.yaml",
    Default::default(),
  )
  .unwrap();

  let context = Context::new().unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", "talker").unwrap(),
      NodeOptions::default(),
    )
    .unwrap();

  let reliable_qos = ros2::QosPolicyBuilder::new()
    .history(policy::History::KeepLast { depth: 10 })
    .reliability(policy::Reliability::Reliable {
      max_blocking_time: ros2::Duration::from_millis(100),
    })
    .durability(policy::Durability::TransientLocal)
    .build();

  let chatter_topic = node
    .create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &reliable_qos,
    )
    .unwrap();

  let chatter_publisher = node
    .create_publisher::<String>(&chatter_topic, None)
    .unwrap();
  let mut count = 0;

  let filler: String =
    "All work and no play makes ROS a dull boy. All play and no work makes RTPS a mere toy. "
      .repeat(2);

  // We must run Spinner to have all the Node functions
  smol::spawn(node.spinner().unwrap().spin()).detach();

  // Dump all status events to console
  //
  // use futures::StreamExt;
  // smol::spawn(node.status_receiver().for_each(|event| async move {
  //   println!("{:?}", event);
  // })).detach();

  smol::block_on(async {
    let mut sub_count = 0;
    loop {
      println!("Waiting for subscribers to appear...");
      chatter_publisher.wait_for_subscription(&node).await;
      loop {
        count += 1;
        let message = format!("count={count} {filler}");
        println!("Talking, count={} len={}", count, message.len());
        let _ = chatter_publisher.async_publish(message).await;

        let new_sub_count = chatter_publisher.get_subscription_count(&node);
        if new_sub_count != sub_count {
          println!("Subscriber count change from {sub_count} to {new_sub_count}");
        }
        sub_count = new_sub_count;

        if sub_count == 0 {
          println!("Stopping publishing.");
          break;
          // Note: This stopping logic is just to test subscriber detection API.
          //
          // A reasonable DDS implementation will stop the actual sending of
          // data to the network if it detects that there are not subscribers
          // left.
        }
        Timer::after(std::time::Duration::from_secs(2)).await;
      }
    }
  });
}

use futures::StreamExt;
use ros2_client::{ros2::policy, *};

pub fn main() {
  // Here is a fixed path, so this example must be started from
  // RustDDS main directory
  log4rs::init_file("examples/async_listener/log4rs.yaml", Default::default()).unwrap();

  let context = Context::new().unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", "rustdds_listener").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap();

  let reliable_qos = ros2::QosPolicyBuilder::new()
    .history(policy::History::KeepLast { depth: 10 })
    .reliability(policy::Reliability::Reliable {
      max_blocking_time: ros2::Duration::from_millis(100),
    })
    //.durability(policy::Durability::TransientLocal)
    .build();

  let chatter_topic = node
    .create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &ros2_client::DEFAULT_SUBSCRIPTION_QOS,
    )
    .unwrap();
  let chatter_subscription = node
    .create_subscription::<String>(&chatter_topic, Some(reliable_qos))
    .unwrap();

  let subscription_stream = chatter_subscription
    .async_stream()
    .for_each(|result| async {
      match result {
        Ok((msg, _)) => println!("I heard: {msg}"),
        Err(e) => eprintln!("Receive request error: {e:?}"),
      }
    });

  // Since we enabled rosout, let's log something
  rosout!(
    node,
    ros2::LogLevel::Info,
    "wow. very listening. such topics. much subscribe."
  );

  smol::block_on(subscription_stream);
}

use smol::future::{self, FutureExt};
use async_ctrlc::CtrlC;
use ros2_client::{ros2, ros2::policy, Context, MessageTypeName, Name, NodeName, NodeOptions};
use async_io::Timer;

fn main() {
  // Here is a fixed path, so this example must be started from
  // RustDDS main directory
  log4rs::init_file("examples/async_talker/log4rs.yaml", Default::default()).unwrap();

  let ctrl_c_signal = CtrlC::new().expect("cannot create Ctrl+C handler?");

  let context = Context::new().unwrap();

  let unique_node_name = format!("talker_{}", std::process::id());
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", &unique_node_name).unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap();

  // Do not use the global executor, but create a local one insted.
  // This drops the executor, and therefore spawned tasks, including spinner,
  // before the end of the program. This gives a chance to run .drop() in
  // `context` and therefore DomainParticipant inside it. This allows
  // RustDDS to send RTPS messages indicating that it is exiting.
  let executor = smol::Executor::new();
  executor.spawn(node.spinner().unwrap().spin()).detach();
  //smol::spawn(node.spinner().unwrap().spin()).detach();

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

  let filler = "All work and no play makes ROS a dull boy.";
  //" All play and no work makes RTPS a mere toy. ";

  future::block_on(executor.run(ctrl_c_signal.race(async {
    loop {
      count += 1;
      let message = format!("count={count} {filler}");
      println!("Talking, count={} len={}", count, message.len());
      let _ = chatter_publisher.async_publish(message).await;
      Timer::after(std::time::Duration::from_secs(2)).await;
    }
  })));
}

use futures::StreamExt;
use smol::future::{self, FutureExt};
use async_ctrlc::CtrlC;
use ros2_client::{ros2::policy, *};
use rustdds::DomainParticipantStatusEvent;

pub fn main() {
  // Here is a fixed path, so this example must be started from
  // RustDDS main directory
  log4rs::init_file("examples/async_listener/log4rs.yaml", Default::default()).unwrap();

  // We need to set a Ctrl-C-handler to prevent immediate stopping of the process
  // at signal. If this is omitted, then destructors cannot run and RustDDS is
  // unable to notify its peers over the network that it is exiting.
  //
  // If this is not done, the program will still run, but the ROS2 Node will just
  // vanish from the network on exit. This causes the other ROS2 nodes to think it
  // is present for some time afterwards.
  let ctrl_c_signal = CtrlC::new().expect("cannot create Ctrl+C handler?");

  let context = Context::new().unwrap();
  let unique_node_name = format!("listener_{}", std::process::id());
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", &unique_node_name).unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap();

  // Use a local excutor instead of global. This allows executor and its tasks,
  // notably the Spinner to be dropped before we drop the Context and
  // DomainParticipant. This again allows DomainParticipant .drop() to run
  // and RustDDS can notify its peers about shutdown.
  let executor = smol::Executor::new();
  executor.spawn(node.spinner().unwrap().spin()).detach();

  let status_event_stream = node.status_receiver().for_each(|event| async move {
    match event {
      NodeEvent::DDS(DomainParticipantStatusEvent::RemoteWriterMatched {
        remote_writer, ..
      }) if remote_writer.entity_id.kind().is_user_defined() => {
        println!("Matched remote writer {remote_writer:?}");
      }
      NodeEvent::DDS(DomainParticipantStatusEvent::WriterLost { guid, reason }) => {
        println!("Lost remote writer {guid:?}: {reason:?}");
      }
      _ => {}
    }
  });
  executor.spawn(status_event_stream).detach();

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

  future::block_on(executor.run(ctrl_c_signal.race(subscription_stream)));
}

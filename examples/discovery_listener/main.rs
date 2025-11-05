use futures::StreamExt;
use smol::future::{self, FutureExt};
use async_ctrlc::CtrlC;
use ros2_client::{Context, NodeName, NodeOptions};

pub fn main() {
  let ctrl_c_signal = CtrlC::new().expect("cannot create Ctrl+C handler?");

  let context = Context::new().unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", "discovery_listener").unwrap(),
      NodeOptions::default(),
    )
    .unwrap();

  let executor = smol::Executor::new();
  executor.spawn(node.spinner().unwrap().spin()).detach();

  let status_event_stream = node.status_receiver().for_each(|event| async move {
    println!("{event:?}");
  });

  future::block_on(executor.run(ctrl_c_signal.race(status_event_stream)));
}

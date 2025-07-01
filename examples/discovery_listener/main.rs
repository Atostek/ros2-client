use futures::StreamExt;
use ros2_client::{Context, NodeName, NodeOptions};

pub fn main() {
  let context = Context::new().unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/rustdds", "discovery_listener").unwrap(),
      NodeOptions::default(),
    )
    .unwrap();

  smol::spawn(node.spinner().unwrap().spin()).detach();

  let status_event_stream = node.status_receiver().for_each(|event| async move {
    println!("{event:?}");
  });

  smol::block_on(status_event_stream);
}

//! `Publisher::get_subscription_count` and `Subscription::get_publisher_count`
//! between two nodes.
#![cfg(feature = "dds")]

use std::time::{Duration, Instant};

use ros2_client::{
  MessageTypeName, Name, NodeName, NodeOptions,
  dds::{Context, ContextOptions, DEFAULT_PUBLISHER_QOS, Node, Publisher, Subscription},
};

const TIMEOUT: Duration = Duration::from_secs(10);

/// A context on the domain that `ROS_DOMAIN_ID` names, so that the test can
/// be moved off a domain other systems on the network use. Domain 0 if it is
/// not set, like the other tests.
fn context() -> Context {
  let domain_id = std::env::var("ROS_DOMAIN_ID")
    .ok()
    .and_then(|id| id.trim().parse().ok())
    .unwrap_or(0);
  Context::with_options(ContextOptions::new().domain_id(domain_id)).unwrap()
}

/// A node `name` with a publisher and a subscription type for `topic`.
fn node(context: &Context, name: &str) -> Node {
  context
    .new_node(NodeName::new("/", name).unwrap(), NodeOptions::new())
    .unwrap()
}

fn publisher(node: &mut Node, topic: &str) -> Publisher<String> {
  let topic = node
    .create_topic(
      &Name::new("/", topic).unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &DEFAULT_PUBLISHER_QOS,
    )
    .unwrap();
  node.create_publisher(&topic, None).unwrap()
}

fn subscription(node: &mut Node, topic: &str) -> Subscription<String> {
  let topic = node
    .create_topic(
      &Name::new("/", topic).unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &DEFAULT_PUBLISHER_QOS,
    )
    .unwrap();
  node.create_subscription(&topic, None).unwrap()
}

/// Polls `condition` until it holds, for at most [`TIMEOUT`].
fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
  let deadline = Instant::now() + TIMEOUT;
  loop {
    if condition() {
      return true;
    }
    if Instant::now() >= deadline {
      return false;
    }
    std::thread::sleep(Duration::from_millis(20));
  }
}

// Neither node runs a Spinner here: the counts are those of the DDS endpoints
// and do not depend on anything processing discovery events.
#[test]
fn counts_follow_a_match_without_a_spinner() {
  let (talker_context, listener_context) = (context(), context());
  let mut talker = node(&talker_context, "count_match_talker");
  let publisher = publisher(&mut talker, "count_match_topic");
  assert_eq!(publisher.get_subscription_count(&talker), 0);

  let mut listener = node(&listener_context, "count_match_listener");
  let subscription = subscription(&mut listener, "count_match_topic");

  assert!(
    wait_for(|| publisher.get_subscription_count(&talker) == 1),
    "the publisher never counted the subscription"
  );
  assert!(
    wait_for(|| subscription.get_publisher_count(&listener) == 1),
    "the subscription never counted the publisher"
  );
}

#[test]
fn subscription_count_drops_when_the_subscription_is_gone() {
  let (talker_context, listener_context) = (context(), context());
  let mut talker = node(&talker_context, "count_drop_talker");
  let publisher = publisher(&mut talker, "count_drop_topic");
  let mut listener = node(&listener_context, "count_drop_listener");
  let subscription = subscription(&mut listener, "count_drop_topic");
  assert!(
    wait_for(|| publisher.get_subscription_count(&talker) == 1),
    "the publisher never counted the subscription"
  );

  drop(subscription);
  assert!(
    wait_for(|| publisher.get_subscription_count(&talker) == 0),
    "the publisher still counts the subscription that is gone"
  );
}

// The same counts, for code that holds the Publisher or the Subscription but
// not the Node.
#[test]
fn matched_counts_need_no_node() {
  let (talker_context, listener_context) = (context(), context());
  let mut talker = node(&talker_context, "count_no_node_talker");
  let publisher = publisher(&mut talker, "count_no_node_topic");
  let mut listener = node(&listener_context, "count_no_node_listener");
  let subscription = subscription(&mut listener, "count_no_node_topic");
  assert_eq!(publisher.matched_subscription_count(), 0);

  assert!(
    wait_for(|| publisher.matched_subscription_count() == 1),
    "the publisher never counted the subscription"
  );
  assert!(
    wait_for(|| subscription.matched_publisher_count() == 1),
    "the subscription never counted the publisher"
  );
}

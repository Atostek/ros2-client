//! `Publisher::get_subscription_count` and `Subscription::get_publisher_count`
//! between two nodes, and what `Publisher::publish` does with nobody matched.
#![cfg(feature = "dds")]

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{Duration, Instant},
};

use serde::{Serialize, Serializer};
use ros2_client::{
  MessageTypeName, Name, NodeName, NodeOptions,
  dds::{Context, ContextOptions, DEFAULT_PUBLISHER_QOS, Node, Publisher, Subscription},
  qos::{Durability, History, Liveliness, QosProfile, WhenFull},
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

/// A `std_msgs/UInt32` that counts how often it is serialized.
#[derive(Debug)]
struct Counted {
  data: u32,
  serialized: Arc<AtomicUsize>,
}

impl Serialize for Counted {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    self.serialized.fetch_add(1, Ordering::Relaxed);
    self.data.serialize(serializer)
  }
}

/// A publisher of [`Counted`] messages with `qos`, and what makes one: `data`
/// and the count of serializations so far.
struct CountedPublisher {
  publisher: Publisher<Counted>,
  serialized: Arc<AtomicUsize>,
}

impl CountedPublisher {
  fn new(node: &mut Node, topic: &str, qos: &QosProfile) -> Self {
    let topic = node
      .create_topic(
        &Name::new("/", topic).unwrap(),
        MessageTypeName::new("std_msgs", "UInt32"),
        qos,
      )
      .unwrap();
    Self {
      publisher: node.create_publisher(&topic, Some(qos.clone())).unwrap(),
      serialized: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn message(&self, data: u32) -> Counted {
    Counted {
      data,
      serialized: Arc::clone(&self.serialized),
    }
  }

  fn serialized(&self) -> usize {
    self.serialized.load(Ordering::Relaxed)
  }
}

fn uint32_subscription(node: &mut Node, topic: &str, qos: &QosProfile) -> Subscription<u32> {
  let topic = node
    .create_topic(
      &Name::new("/", topic).unwrap(),
      MessageTypeName::new("std_msgs", "UInt32"),
      qos,
    )
    .unwrap();
  node.create_subscription(&topic, Some(qos.clone())).unwrap()
}

// A message for a volatile topic that no subscription is matched on would
// reach nobody, so it is not written, and with that not serialized. Once a
// subscription is matched, it is.
#[test]
fn publish_writes_nothing_while_no_subscription_is_matched() {
  let (talker_context, listener_context) = (context(), context());
  let mut talker = node(&talker_context, "skip_talker");
  let talker = CountedPublisher::new(&mut talker, "skip_topic", &QosProfile::publisher_default());

  for data in 0..5 {
    talker.publisher.publish(talker.message(data)).unwrap();
  }
  assert_eq!(talker.serialized(), 0, "serialized for nobody");

  let mut listener = node(&listener_context, "skip_listener");
  let subscription = uint32_subscription(
    &mut listener,
    "skip_topic",
    &QosProfile::subscription_default().reliability_reliable(WhenFull::DEFAULT),
  );
  assert!(
    wait_for(|| talker.publisher.matched_subscription_count() == 1),
    "the publisher never counted the subscription"
  );

  let mut received = None;
  assert!(
    wait_for(|| {
      talker.publisher.publish(talker.message(7)).unwrap();
      received = subscription.take().unwrap().map(|(data, _)| data);
      received.is_some()
    }),
    "the subscription never received a message"
  );
  assert_eq!(received, Some(7));
  assert!(talker.serialized() > 0);
}

#[tokio::test]
async fn async_publish_writes_nothing_while_no_subscription_is_matched() {
  let talker_context = context();
  let mut talker = node(&talker_context, "async_skip_talker");
  let talker = CountedPublisher::new(
    &mut talker,
    "async_skip_topic",
    &QosProfile::publisher_default(),
  );

  talker
    .publisher
    .async_publish(talker.message(1))
    .await
    .unwrap();
  assert_eq!(talker.serialized(), 0, "serialized for nobody");
}

// A transient-local topic keeps its messages for subscriptions that come
// later, so they are written with nobody matched.
#[test]
fn publish_writes_for_late_joiners_on_a_transient_local_topic() {
  let (talker_context, listener_context) = (context(), context());
  let latched = QosProfile::publisher_default()
    .durability(Durability::TransientLocal)
    .history(History::KeepLast { depth: 10 });
  let mut talker = node(&talker_context, "latched_talker");
  let talker = CountedPublisher::new(&mut talker, "latched_topic", &latched);

  talker.publisher.publish(talker.message(42)).unwrap();

  let mut listener = node(&listener_context, "latched_listener");
  let subscription = uint32_subscription(&mut listener, "latched_topic", &latched);
  let mut received = None;
  assert!(
    wait_for(|| {
      received = subscription.take().unwrap().map(|(data, _)| data);
      received.is_some()
    }),
    "the late subscription never received the message published before it"
  );
  assert_eq!(received, Some(42));
}

// Writing is what asserts manual liveliness, so such a publisher writes even
// with nobody matched.
#[test]
fn publish_writes_with_manual_liveliness() {
  let talker_context = context();
  let mut talker = node(&talker_context, "manual_liveliness_talker");
  let talker = CountedPublisher::new(
    &mut talker,
    "manual_liveliness_topic",
    &QosProfile::publisher_default().liveliness(Liveliness::ManualByTopic),
  );

  talker.publisher.publish(talker.message(1)).unwrap();
  assert!(talker.serialized() > 0);
}

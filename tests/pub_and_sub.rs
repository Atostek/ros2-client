//! Ensures that two `ros2_client` nodes can speak to each other.

// Test case contributed by @onkoe, https://github.com/onkoe

use std::{pin::pin, time::Duration};

use futures::{future::select, StreamExt};
use ros2_client::{
  Context, MessageTypeName, Name, NodeName, NodeOptions, Publisher, Subscription,
  DEFAULT_PUBLISHER_QOS,
};

#[tokio::test]
async fn pub_and_sub() {
  select(pin!(make_publisher()), pin!(make_subscriber())).await;
}

async fn make_subscriber() {
  // make context
  let ctx = Context::new().unwrap();

  // node, topic, subscriber
  let mut node = ctx
    .new_node(
      NodeName::new("/", "topic_test_sub_node").unwrap(),
      NodeOptions::new(),
    )
    .unwrap();
  let topic = node
    .create_topic(
      &Name::new("/", "pub_sub_topic").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &DEFAULT_PUBLISHER_QOS.clone(),
    )
    .unwrap();
  let subscriber: Subscription<String> = node.create_subscription(&topic, None).unwrap();

  // spin node in background
  tokio::task::spawn(node.spinner().unwrap().spin());

  // start listening.
  //
  // if we don't get a message within five seconds, fail the test.
  let (msg, _msg_info) = tokio::time::timeout(
    Duration::from_secs(5),
    Box::pin(subscriber.async_stream()).next(),
  )
  .await
  .expect("Test timed out - publisher never sent anything!")
  .expect("we should've got a message.")
  .inspect(|m| println!("Subscriber received message {m:?}"))
  .expect("message should've sent correctly!");

  assert_eq!(msg, "hello subscriber!");
}

async fn make_publisher() {
  let ctx = Context::new().unwrap();

  let mut node = ctx
    .new_node(
      NodeName::new("/", "topic_test_pub_node").unwrap(),
      NodeOptions::new(),
    )
    .unwrap();

  let topic = node
    .create_topic(
      &Name::new("/", "pub_sub_topic").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &DEFAULT_PUBLISHER_QOS.clone(),
    )
    .unwrap();

  let publisher: Publisher<String> = node.create_publisher(&topic, None).unwrap();

  tokio::task::spawn(node.spinner().unwrap().spin());

  // send messages every 0.25 seconds
  loop {
    publisher
      .async_publish("hello subscriber!".into())
      .await
      .unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;
  }
}

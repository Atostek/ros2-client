//! Check that late-joining subscriber gets messages in Reliable topic.

// Test idea:
//
// Set up a Reliable Publisher to publish messages 0,1,2,3,4.
// After publishing, the Publisher task waits idle for some time, so that a
// late-joining Subscriber can get the data. After the wait the Publisher
// exists.
//
// Subscriber task first waits, and only after that sets up subscription.
// (It is late-joining.) Then it starts subscribing messages.
//
// Now, subscriber should be able to receive 0,1,2,3,4, from start to finish,
// and in order, as long as the Publisher is still alive. If the Publisher exits
// before Subscriber can receive all the sent data, then some obviously goes
// missing, as there is no-one holding the data. (If the underlying DDS would
// implement higher levels of QoS policy Persistence than TransientLocal,
// then we could exit the Publisher immediately, and the messages would be
// buffered by the DDS service "somewhere else".)
//
// Additional twist: After receiving the data correctly, the Subscriber signals
// the Publisher that it can stop waiting and exit immediately. This is not
// really part of the test, but just to speed up test case execution.
//
// In order to make this work, we need proper QoS settings both at Publisher and
// Subscriber. See comments at QoS setup below.

use std::{pin::pin, str::FromStr, time::Duration};

use futures::{
  future::{join, select},
  StreamExt,
};
use tokio::sync::oneshot;
use ros2_client::{
  ros2::{policy::*, QosPolicyBuilder},
  Context, MessageTypeName, Name, NodeName, NodeOptions, Publisher, Subscription,
  DEFAULT_PUBLISHER_QOS, DEFAULT_SUBSCRIPTION_QOS,
};

#[tokio::test]
async fn late_joiner() {
  let (tx, rx) = oneshot::channel();
  join(pin!(make_publisher(rx)), pin!(make_subscriber(tx))).await;
}

async fn make_subscriber(tx: oneshot::Sender<()>) {
  // make context
  let ctx = Context::new().unwrap();

  // node, topic, subscriber
  let mut node = ctx
    .new_node(
      NodeName::new("/", "late_join_sub_node").unwrap(),
      NodeOptions::new(),
    )
    .unwrap();
  let sub_policy = DEFAULT_SUBSCRIPTION_QOS.modify_by(
    &QosPolicyBuilder::new()
      // Subscriber must be made Reliable, or it will not request past data.
      .reliability(Reliability::Reliable {
        max_blocking_time: Duration::from_millis(1000).into(),
      })
      .build(),
  );
  let topic = node
    .create_topic(
      &Name::new("/", "late_topic").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &sub_policy,
    )
    .unwrap();

  // spin node in background
  tokio::task::spawn(node.spinner().unwrap().spin());

  eprintln!("Waiting to subscribe...");
  // this is late subscriber, so we delay
  tokio::time::sleep(Duration::from_millis(2000)).await;
  eprintln!("Subscribing");
  let subscriber: Subscription<String> = node.create_subscription(&topic, None).unwrap();

  // start listening.
  //
  // if we don't get a message within five seconds, fail the test.
  tokio::time::timeout(Duration::from_secs(8), async {
    let mut s = Box::pin(subscriber.async_stream());
    let mut i = 0;
    eprintln!("Waiting for data");
    while let Some(value) = s.next().await {
      let m = value.expect("Was expecting a message, got {value:?}").0;
      eprintln!("Received {m}");
      let n = i32::from_str(&m).expect("Expected number, got {m:?}");
      assert_eq!(i, n);
      i += 1;
      if i == 5 {
        break;
      }
    }
    eprintln!("Receive loop done");
  })
  .await
  .expect("Test timed out!");

  // Tell the publisher that it can stop.
  tx.send(()).expect("Cannot signal publisher to stop");
  eprintln!("Subscriber done");
}

async fn make_publisher(rx: oneshot::Receiver<()>) {
  let ctx = Context::new().unwrap();

  let mut node = ctx
    .new_node(
      NodeName::new("/", "late_join_pub_node").unwrap(),
      NodeOptions::new(),
    )
    .unwrap();
  let topic = node
    .create_topic(
      &Name::new("/", "late_topic").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &DEFAULT_PUBLISHER_QOS.clone(),
    )
    .unwrap();
  let pub_policy = DEFAULT_PUBLISHER_QOS.modify_by(
    &QosPolicyBuilder::new()
      // TransientLocal is necessary. Otherwise Durablity = Volatile and published data
      // is forgotten immediately after publishing.
      .durability(Durability::TransientLocal)
      // Reliablility is obviously necessary.
      .reliability(Reliability::Reliable {
        max_blocking_time: Duration::from_millis(100).into(),
      })
      // History buffer must be deep enough to hold all the data we wish to
      // provide for late joiners. In this case, >= 5 .
      .history(History::KeepLast { depth: 10 })
      .build(),
  );

  let publisher: Publisher<String> = node.create_publisher(&topic, Some(pub_policy)).unwrap();
  tokio::task::spawn(node.spinner().unwrap().spin());

  // send messages every 0.25 seconds
  for i in 0..5 {
    eprintln!("Publish {i}");
    publisher.async_publish(i.to_string()).await.unwrap();

    tokio::time::sleep(Duration::from_millis(400)).await;
  }

  // keep publisher alive for a few seconds, or until signalled
  select(pin!(tokio::time::sleep(Duration::from_millis(15000))), rx).await;
  eprintln!("Publisher exiting");
}

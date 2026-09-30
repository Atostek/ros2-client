//! Zenoh `Publisher` and `Subscription` (E4).
//!
//! A publisher `put`s a CDR payload (see [`super::cdr`]) with an
//! [`AttachmentData`] carrying `(sequence, source_timestamp, source_gid)` — the
//! `rmw_zenoh` message shape. A subscription declares a Zenoh subscriber with a
//! wildcard type-hash (liberal receive, ADR-0007) and yields decoded messages
//! plus their [`MessageInfo`].

use std::{
  future::Future,
  marker::PhantomData,
  sync::atomic::{AtomicI64, Ordering},
  time::{SystemTime, UNIX_EPOCH},
};

use futures::stream::{self, FusedStream, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use zenoh::{
  handlers::FifoChannelHandler,
  liveliness::LivelinessToken,
  pubsub::{Publisher as ZenohPublisher, Subscriber},
  sample::Sample,
  Wait,
};

use super::{attachment::AttachmentData, cdr, node::Node};
use crate::{
  error::{write_middleware, write_serialization, ReadError, ReadResult, WriteResult},
  gid::Gid,
  message_info::MessageInfo,
  ros_time::ROSTime,
};

fn now_nanos() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_nanos() as i64)
    .unwrap_or(0)
}

/// A ROS 2 publisher over Zenoh.
pub struct Publisher<M> {
  zenoh_publisher: ZenohPublisher<'static>,
  seq: AtomicI64,
  source_gid: [u8; 16],
  // Kept alive so the entity stays discoverable; dropped => token undeclared.
  _liveliness_token: Option<LivelinessToken>,
  // The topic's fully-qualified name, kept for the `Node`-scoped count/wait
  // helpers below (mirrors the DDS backend, which looks these up via GUID on
  // the owning `Node` instead).
  topic_fqn: String,
  phantom: PhantomData<fn() -> M>,
}

impl<M: Serialize> Publisher<M> {
  pub(crate) fn new(
    zenoh_publisher: ZenohPublisher<'static>,
    source_gid: [u8; 16],
    liveliness_token: Option<LivelinessToken>,
    topic_fqn: String,
  ) -> Self {
    Self {
      zenoh_publisher,
      seq: AtomicI64::new(0),
      source_gid,
      _liveliness_token: liveliness_token,
      topic_fqn,
      phantom: PhantomData,
    }
  }

  fn encode(&self, msg: &M) -> Result<(Vec<u8>, zenoh::bytes::ZBytes), cdr::CdrError> {
    let payload = cdr::to_cdr(msg)?;
    let sequence_number = self.seq.fetch_add(1, Ordering::Relaxed) + 1; // start
                                                                        // at 1
    let attachment = AttachmentData {
      sequence_number,
      source_timestamp: now_nanos(),
      source_gid: self.source_gid,
    }
    .to_zbytes();
    Ok((payload, attachment))
  }

  /// Publish a message (async).
  pub async fn async_publish(&self, msg: M) -> WriteResult<(), M> {
    let (payload, attachment) = match self.encode(&msg) {
      Ok(encoded) => encoded,
      Err(e) => return Err(write_serialization(e.to_string(), msg)),
    };
    self
      .zenoh_publisher
      .put(payload)
      .attachment(attachment)
      .await
      .map_err(|e| write_middleware(e.to_string(), msg))
  }

  /// Publish a message (blocking).
  pub fn publish(&self, msg: M) -> WriteResult<(), M> {
    let (payload, attachment) = match self.encode(&msg) {
      Ok(encoded) => encoded,
      Err(e) => return Err(write_serialization(e.to_string(), msg)),
    };
    self
      .zenoh_publisher
      .put(payload)
      .attachment(attachment)
      .wait()
      .map_err(|e| write_middleware(e.to_string(), msg))
  }

  /// This publisher's source [`Gid`].
  pub fn gid(&self) -> Gid {
    Gid::from(self.source_gid)
  }

  /// Returns the count of currently discovered subscriptions on this
  /// publisher's topic.
  ///
  /// `node` must be the [`Node`] that created this Publisher (or at least
  /// share its [`Context`](crate::Context)), or the result is undefined.
  pub fn get_subscription_count(&self, node: &Node) -> usize {
    node.subscription_count(&self.topic_fqn)
  }

  /// Waits until there is at least one matched subscription on this topic,
  /// possibly forever.
  ///
  /// `node` must be the [`Node`] that created this Publisher (or at least
  /// share its [`Context`](crate::Context)), or the length of the wait is
  /// undefined.
  pub fn wait_for_subscription<'a>(&'a self, node: &'a Node) -> impl Future<Output = ()> + 'a {
    node.wait_for_subscription(&self.topic_fqn)
  }
}

/// A ROS 2 subscription over Zenoh.
pub struct Subscription<M> {
  zenoh_subscriber: Subscriber<FifoChannelHandler<Sample>>,
  // Kept alive so the entity stays discoverable; dropped => token undeclared.
  _liveliness_token: Option<LivelinessToken>,
  // The topic's fully-qualified name; see `Publisher::topic_fqn`.
  topic_fqn: String,
  phantom: PhantomData<fn() -> M>,
}

impl<M: DeserializeOwned> Subscription<M> {
  pub(crate) fn new(
    zenoh_subscriber: Subscriber<FifoChannelHandler<Sample>>,
    liveliness_token: Option<LivelinessToken>,
    topic_fqn: String,
  ) -> Self {
    Self {
      zenoh_subscriber,
      _liveliness_token: liveliness_token,
      topic_fqn,
      phantom: PhantomData,
    }
  }

  fn decode(sample: &Sample) -> ReadResult<(M, MessageInfo)> {
    let payload = sample.payload().to_bytes();
    let msg = cdr::from_cdr::<M>(&payload)?;
    let info = match sample.attachment() {
      Some(zbytes) => {
        let a = AttachmentData::from_zbytes(zbytes).map_err(|_| ReadError::Malformed)?;
        MessageInfo::new(
          None,
          Some(ROSTime::from_nanos(a.source_timestamp)),
          a.sequence_number,
          Gid::from(a.source_gid),
          None,
        )
      }
      None => MessageInfo::new(None, None, 0, Gid::default(), None),
    };
    Ok((msg, info))
  }

  /// Await the next message and its metadata.
  pub async fn async_take(&self) -> ReadResult<(M, MessageInfo)> {
    let sample = self
      .zenoh_subscriber
      .recv_async()
      .await
      .map_err(|_| ReadError::Closed)?;
    Self::decode(&sample)
  }

  /// Take a message if one is immediately available (non-blocking).
  pub fn try_take(&self) -> ReadResult<Option<(M, MessageInfo)>> {
    match self.zenoh_subscriber.try_recv() {
      Ok(Some(sample)) => Self::decode(&sample).map(Some),
      Ok(None) => Ok(None),
      Err(_) => Err(ReadError::Closed),
    }
  }

  /// An async `Stream` of messages with `MessageInfo` metadata (mirrors the
  /// DDS backend's `Subscription::async_stream`), built by repeatedly calling
  /// [`Self::async_take`]. The stream never ends on its own.
  pub fn async_stream(&self) -> impl FusedStream<Item = ReadResult<(M, MessageInfo)>> + '_ {
    stream::unfold(self, |sub| async move {
      let item = sub.async_take().await;
      Some((item, sub))
    })
    .fuse()
  }

  /// Returns the count of currently discovered publishers on this
  /// subscription's topic.
  ///
  /// `node` must be the [`Node`] that created this Subscription (or at least
  /// share its [`Context`](crate::Context)), or the result is undefined.
  pub fn get_publisher_count(&self, node: &Node) -> usize {
    node.publisher_count(&self.topic_fqn)
  }

  /// Waits until there is at least one matched publisher on this topic,
  /// possibly forever.
  ///
  /// `node` must be the [`Node`] that created this Subscription (or at least
  /// share its [`Context`](crate::Context)), or the length of the wait is
  /// undefined.
  pub fn wait_for_publisher<'a>(&'a self, node: &'a Node) -> impl Future<Output = ()> + 'a {
    node.wait_for_publisher(&self.topic_fqn)
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use zenoh::Config;

  use super::{Publisher, Subscription};
  use crate::{
    zenoh_backend::{
      context::{Context, ContextOptions},
      node::Node,
    },
    Gid, MessageTypeName, Name, NodeName, NodeOptions, QosProfile,
  };

  // Build a peer config on IPv4 loopback with multicast off. `listen`/`connect`
  // pin explicit ports so two in-process peers connect directly — no router
  // (matches Tier B in docs/zenoh_study/test_plan.md).
  fn make_config(listen_port: u16, connect_port: Option<u16>) -> Config {
    let mut c = Config::default();
    c.insert_json5("mode", "\"peer\"").unwrap();
    c.insert_json5("scouting/multicast/enabled", "false")
      .unwrap();
    c.insert_json5(
      "listen/endpoints",
      &format!("[\"tcp/127.0.0.1:{listen_port}\"]"),
    )
    .unwrap();
    if let Some(p) = connect_port {
      c.insert_json5("connect/endpoints", &format!("[\"tcp/127.0.0.1:{p}\"]"))
        .unwrap();
    }
    c
  }

  #[test]
  fn pub_sub_roundtrip_in_process() {
    // Distinct fixed ports (CI runs zenoh tests with --test-threads=1).
    let sub_port = 17513;
    let pub_port = 17514;

    let sub_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(sub_port, None)))
        .expect("open subscriber context");
    let pub_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(pub_port, Some(sub_port))),
    )
    .expect("open publisher context");

    let sub_node = sub_ctx
      .new_node(NodeName::new("/", "test_sub").unwrap(), NodeOptions::new())
      .unwrap();
    let pub_node = pub_ctx
      .new_node(NodeName::new("/", "test_pub").unwrap(), NodeOptions::new())
      .unwrap();

    let make_topic = |n: &Node| {
      n.create_topic(
        &Name::new("/", "chatter").unwrap(),
        MessageTypeName::new("std_msgs", "String"),
        &QosProfile::default(),
      )
    };
    let sub: Subscription<String> = sub_node
      .create_subscription(&make_topic(&sub_node), None)
      .expect("create subscription");
    let publisher: Publisher<String> = pub_node
      .create_publisher(&make_topic(&pub_node), None)
      .expect("create publisher");

    // Publish repeatedly until the peers have connected and a sample arrives.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut got = None;
    while Instant::now() < deadline {
      publisher
        .publish("hello zenoh!".to_string())
        .expect("publish");
      if let Some(m) = sub.try_take().expect("try_take") {
        got = Some(m);
        break;
      }
      std::thread::sleep(Duration::from_millis(100));
    }

    let (msg, info) = got.expect("no message received within timeout");
    assert_eq!(msg, "hello zenoh!");
    assert!(info.sequence_number() >= 1);
    assert_ne!(info.publisher_gid(), Gid::default());
  }
}

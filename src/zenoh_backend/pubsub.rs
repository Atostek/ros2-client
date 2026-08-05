//! Zenoh `Publisher` and `Subscription` (E4).
//!
//! A publisher `put`s a CDR payload (see [`super::cdr`]) with an
//! [`AttachmentData`] carrying `(sequence, source_timestamp, source_gid)` — the
//! `rmw_zenoh` message shape. A subscription declares a Zenoh subscriber with a
//! wildcard type-hash (liberal receive, ADR-0007) and yields decoded messages
//! plus their [`MessageInfo`].

use std::{
  marker::PhantomData,
  sync::atomic::{AtomicI64, Ordering},
  time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Serialize};
use zenoh::{
  handlers::FifoChannelHandler,
  liveliness::LivelinessToken,
  pubsub::{Publisher as ZenohPublisher, Subscriber},
  sample::Sample,
  Wait,
};

use super::{attachment::AttachmentData, cdr};

/// Metadata about a received message, extracted from its Zenoh attachment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageInfo {
  source_timestamp_nanos: i64,
  sequence_number: i64,
  source_gid: [u8; 16],
}

impl MessageInfo {
  /// Source timestamp (ns since UNIX epoch) set by the publisher, or 0 if the
  /// message carried no ROS attachment.
  pub fn source_timestamp_nanos(&self) -> i64 {
    self.source_timestamp_nanos
  }

  /// Per-publisher sequence number of this message.
  pub fn sequence_number(&self) -> i64 {
    self.sequence_number
  }

  /// 16-byte GID of the publishing entity.
  pub fn source_gid(&self) -> [u8; 16] {
    self.source_gid
  }
}

/// Failure to publish a message.
#[derive(Debug)]
pub enum PublishError {
  /// CDR serialization of the message failed.
  Cdr(cdr::CdrError),
  /// The Zenoh `put` failed.
  Zenoh(zenoh::Error),
}

impl std::fmt::Display for PublishError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PublishError::Cdr(e) => write!(f, "publish: {e}"),
      PublishError::Zenoh(e) => write!(f, "publish: zenoh error: {e}"),
    }
  }
}
impl std::error::Error for PublishError {}

/// Failure to receive/decode a message.
#[derive(Debug)]
pub enum TakeError {
  /// CDR deserialization failed.
  Cdr(cdr::CdrError),
  /// The message attachment was malformed.
  Attachment,
  /// The subscriber channel was closed.
  Closed,
}

impl std::fmt::Display for TakeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      TakeError::Cdr(e) => write!(f, "take: {e}"),
      TakeError::Attachment => write!(f, "take: malformed attachment"),
      TakeError::Closed => write!(f, "take: subscriber closed"),
    }
  }
}
impl std::error::Error for TakeError {}

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
  phantom: PhantomData<fn() -> M>,
}

impl<M: Serialize> Publisher<M> {
  pub(crate) fn new(
    zenoh_publisher: ZenohPublisher<'static>,
    source_gid: [u8; 16],
    liveliness_token: Option<LivelinessToken>,
  ) -> Self {
    Self {
      zenoh_publisher,
      seq: AtomicI64::new(0),
      source_gid,
      _liveliness_token: liveliness_token,
      phantom: PhantomData,
    }
  }

  fn encode(&self, msg: &M) -> Result<(Vec<u8>, zenoh::bytes::ZBytes), PublishError> {
    let payload = cdr::to_cdr(msg).map_err(PublishError::Cdr)?;
    let sequence_number = self.seq.fetch_add(1, Ordering::Relaxed) + 1; // start at 1
    let attachment = AttachmentData {
      sequence_number,
      source_timestamp: now_nanos(),
      source_gid: self.source_gid,
    }
    .to_zbytes();
    Ok((payload, attachment))
  }

  /// Publish a message (async).
  pub async fn async_publish(&self, msg: M) -> Result<(), PublishError> {
    let (payload, attachment) = self.encode(&msg)?;
    self
      .zenoh_publisher
      .put(payload)
      .attachment(attachment)
      .await
      .map_err(PublishError::Zenoh)
  }

  /// Publish a message (blocking).
  pub fn publish(&self, msg: M) -> Result<(), PublishError> {
    let (payload, attachment) = self.encode(&msg)?;
    self
      .zenoh_publisher
      .put(payload)
      .attachment(attachment)
      .wait()
      .map_err(PublishError::Zenoh)
  }

  /// This publisher's 16-byte source GID.
  pub fn gid(&self) -> [u8; 16] {
    self.source_gid
  }
}

/// A ROS 2 subscription over Zenoh.
pub struct Subscription<M> {
  zenoh_subscriber: Subscriber<FifoChannelHandler<Sample>>,
  // Kept alive so the entity stays discoverable; dropped => token undeclared.
  _liveliness_token: Option<LivelinessToken>,
  phantom: PhantomData<fn() -> M>,
}

impl<M: DeserializeOwned> Subscription<M> {
  pub(crate) fn new(
    zenoh_subscriber: Subscriber<FifoChannelHandler<Sample>>,
    liveliness_token: Option<LivelinessToken>,
  ) -> Self {
    Self {
      zenoh_subscriber,
      _liveliness_token: liveliness_token,
      phantom: PhantomData,
    }
  }

  fn decode(sample: &Sample) -> Result<(M, MessageInfo), TakeError> {
    let payload = sample.payload().to_bytes();
    let msg = cdr::from_cdr::<M>(&payload).map_err(TakeError::Cdr)?;
    let info = match sample.attachment() {
      Some(zbytes) => {
        let a = AttachmentData::from_zbytes(zbytes).map_err(|_| TakeError::Attachment)?;
        MessageInfo {
          source_timestamp_nanos: a.source_timestamp,
          sequence_number: a.sequence_number,
          source_gid: a.source_gid,
        }
      }
      None => MessageInfo::default(),
    };
    Ok((msg, info))
  }

  /// Await the next message and its metadata.
  pub async fn async_take(&self) -> Result<(M, MessageInfo), TakeError> {
    let sample = self
      .zenoh_subscriber
      .recv_async()
      .await
      .map_err(|_| TakeError::Closed)?;
    Self::decode(&sample)
  }

  /// Take a message if one is immediately available (non-blocking).
  pub fn try_take(&self) -> Result<Option<(M, MessageInfo)>, TakeError> {
    match self.zenoh_subscriber.try_recv() {
      Ok(Some(sample)) => Self::decode(&sample).map(Some),
      Ok(None) => Ok(None),
      Err(_) => Err(TakeError::Closed),
    }
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use zenoh::Config;

  use super::{Publisher, Subscription};
  use crate::{Context, ContextOptions, MessageTypeName, Name, NodeName, NodeOptions, QosProfile};

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

    let sub_node = sub_ctx.new_node(NodeName::new("/", "test_sub").unwrap(), NodeOptions::new());
    let pub_node = pub_ctx.new_node(NodeName::new("/", "test_pub").unwrap(), NodeOptions::new());

    let make_topic = |n: &crate::Node| {
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
    assert_ne!(info.source_gid(), [0u8; 16]);
  }
}

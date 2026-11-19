#[cfg(any(feature = "mio_06", feature = "mio_1"))]
use std::io;
use std::marker::PhantomData;

#[cfg(feature = "mio_06")]
use rustdds::mio::{Evented, Poll, PollOpt, Ready, Token};
#[cfg(feature = "mio_1")]
use rustdds::mio1;
use rustdds::{serialization::CdrDeserializeSeedDecoder, *};
use futures::{
  Future, pin_mut,
  stream::{FusedStream, StreamExt},
};
use serde::{Serialize, de::DeserializeOwned};

use super::{gid::Gid, message_info::MessageInfo, node::Node};
use crate::error::{ReadError, ReadResult, WriteResult};

/// A ROS2 Publisher
///
/// Corresponds to a simplified [`DataWriter`](rustdds::no_key::DataWriter)in
/// DDS
pub struct Publisher<M: Serialize> {
  datawriter: no_key::DataWriterCdr<M>,
  // Whether a message may be left unwritten while no subscription is matched.
  // See `publish`.
  skip_unmatched: bool,
}

impl<M: Serialize> Publisher<M> {
  // These must be created from Node
  pub(crate) fn new(datawriter: no_key::DataWriterCdr<M>) -> Publisher<M> {
    let qos = rustdds::qos::HasQoSPolicy::qos(&datawriter);
    // A volatile topic keeps nothing for subscriptions that match later, so a
    // message written while none is matched reaches nobody. Any other
    // durability does keep it. And a write is what asserts manual liveliness,
    // so a publisher with that always writes.
    let volatile = matches!(qos.durability(), None | Some(policy::Durability::Volatile));
    let automatic_liveliness = matches!(
      qos.liveliness(),
      None | Some(policy::Liveliness::Automatic { .. })
    );
    Publisher {
      datawriter,
      skip_unmatched: volatile && automatic_liveliness,
    }
  }

  /// Publishes `message`.
  ///
  /// A message that would reach nobody is not written: while no subscription
  /// is matched, a message for a volatile topic is dropped here, before it is
  /// serialized, and the call returns `Ok`. A transient-local topic keeps its
  /// messages for later subscriptions, so they are always written, and so are
  /// those of a publisher with manual liveliness.
  pub fn publish(&self, message: M) -> WriteResult<(), M> {
    if self.nobody_would_receive() {
      return Ok(());
    }
    self
      .datawriter
      .write(message, Some(Timestamp::now()))
      .map_err(Into::into)
  }

  // True if a message published now would reach nobody. See `publish`.
  fn nobody_would_receive(&self) -> bool {
    self.skip_unmatched && self.matched_subscription_count() == 0
  }

  pub fn assert_liveliness(&self) -> WriteResult<(), ()> {
    self.datawriter.assert_liveliness().map_err(Into::into)
  }

  pub fn guid(&self) -> rustdds::GUID {
    self.datawriter.guid()
  }

  pub fn gid(&self) -> Gid {
    self.guid().into()
  }

  /// Returns the count of currently matched subscribers.
  ///
  /// This is the count the underlying DataWriter keeps itself, so it does not
  /// depend on a [`Spinner`](crate::dds::Spinner) processing discovery events.
  /// `my_node` is not used. The parameter is kept for compatibility, and
  /// because the Zenoh backend needs it. See
  /// [`matched_subscription_count`](Self::matched_subscription_count) for the
  /// same count without it.
  pub fn get_subscription_count(&self, _my_node: &Node) -> usize {
    self.matched_subscription_count()
  }

  /// Returns the count of currently matched subscribers, like
  /// [`get_subscription_count`](Self::get_subscription_count), without
  /// needing the Node.
  ///
  /// It takes no lock, so it is cheap enough to call before every `publish`,
  /// for example to not build a message that nobody would receive.
  pub fn matched_subscription_count(&self) -> usize {
    let matched = self.datawriter.get_publication_matched_status();
    usize::try_from(matched.current.count()).unwrap_or(0)
  }

  /// Waits until there is at least one matched subscription on this topic,
  /// possibly forever.
  ///
  /// `my_node` must be the Node that created this Subscription, or the length
  /// of the wait is undefined.
  pub fn wait_for_subscription(&self, my_node: &Node) -> impl Future<Output = ()> + Send {
    my_node.wait_for_reader(self.guid())
  }

  /// Publishes `message`, like [`publish`](Self::publish), and waits if the
  /// DataWriter cannot take it at once.
  pub async fn async_publish(&self, message: M) -> WriteResult<(), M> {
    if self.nobody_would_receive() {
      return Ok(());
    }
    self
      .datawriter
      .async_write(message, Some(Timestamp::now()))
      .await
      .map_err(Into::into)
  }
}
// ----------------------------------------------------
// ----------------------------------------------------
// ----------------------------------------------------
// ----------------------------------------------------
// ----------------------------------------------------

/// A ROS2 Subscription
///
/// Corresponds to a (simplified) [`DataReader`](rustdds::no_key::DataReader) in
/// DDS
pub struct Subscription<M> {
  datareader: no_key::SimpleDataReaderCdr<M>,
}

impl<M> Subscription<M>
where
  M: 'static,
{
  // These must be created from Node
  pub(crate) fn new(datareader: no_key::SimpleDataReaderCdr<M>) -> Subscription<M> {
    Subscription { datareader }
  }

  pub fn take_seed<S>(&self, seed: S) -> ReadResult<Option<(M, MessageInfo)>>
  where
    S: for<'de> serde::de::DeserializeSeed<'de, Value = M> + Clone,
    M: 'static,
  {
    self.datareader.drain_read_notifications();
    let decoder = CdrDeserializeSeedDecoder::new(seed, PhantomData::<()>);
    let ds: Option<no_key::DeserializedCacheChange<M>> =
      self.datareader.try_take_one_with(decoder)?;
    Ok(ds.map(dcc_to_value_and_messageinfo))
  }

  // Returns an async Stream of messages with MessageInfo metadata
  pub fn async_stream_seed<'a, S>(
    &'a self,
    seed: S,
  ) -> impl FusedStream<Item = ReadResult<(M, MessageInfo)>> + 'a
  where
    S: for<'de> serde::de::DeserializeSeed<'de, Value = M> + Clone + 'a,
    M: 'static,
  {
    let decoder = CdrDeserializeSeedDecoder::new(seed, PhantomData::<()>);
    self
      .datareader
      .as_async_stream_with(decoder)
      .map(|result| result.map_err(Into::into).map(dcc_to_value_and_messageinfo))
  }
}

impl<M: 'static + DeserializeOwned> Subscription<M> {
  pub fn take(&self) -> ReadResult<Option<(M, MessageInfo)>> {
    self.datareader.drain_read_notifications();
    let ds: Option<no_key::DeserializedCacheChange<M>> = self.datareader.try_take_one()?;
    Ok(ds.map(dcc_to_value_and_messageinfo))
  }

  pub async fn async_take(&self) -> ReadResult<(M, MessageInfo)> {
    let async_stream = self.datareader.as_async_stream();
    pin_mut!(async_stream);
    match async_stream.next().await {
      Some(Err(e)) => Err(e.into()),
      Some(Ok(ds)) => Ok(dcc_to_value_and_messageinfo(ds)),
      // Stream from SimpleDataReader is not supposed to ever end.
      None => {
        read_error_internal!("async_take(): SimpleDataReader value stream unexpectedly ended!")
      }
    }
  }

  // Returns an async Stream of messages with MessageInfo metadata
  pub fn async_stream(&self) -> impl FusedStream<Item = ReadResult<(M, MessageInfo)>> + '_ {
    self
      .datareader
      .as_async_stream()
      .map(|result| result.map_err(Into::into).map(dcc_to_value_and_messageinfo))
  }
}

impl<M> Subscription<M>
where
  M: 'static,
{
  pub fn guid(&self) -> rustdds::GUID {
    self.datareader.guid()
  }

  pub fn gid(&self) -> Gid {
    self.guid().into()
  }

  /// Returns the count of currently matched Publishers.
  ///
  /// This is the count the underlying DataReader keeps itself, so it does not
  /// depend on a [`Spinner`](crate::dds::Spinner) processing discovery events.
  /// `my_node` is not used. The parameter is kept for compatibility, and
  /// because the Zenoh backend needs it. See
  /// [`matched_publisher_count`](Self::matched_publisher_count) for the same
  /// count without it.
  pub fn get_publisher_count(&self, _my_node: &Node) -> usize {
    self.matched_publisher_count()
  }

  /// Returns the count of currently matched Publishers, like
  /// [`get_publisher_count`](Self::get_publisher_count), without needing the
  /// Node.
  pub fn matched_publisher_count(&self) -> usize {
    let matched = self.datareader.get_subscription_matched_status();
    usize::try_from(matched.current.count()).unwrap_or(0)
  }

  /// Waits until there is at least one matched publisher on this topic,
  /// possibly forever.
  ///
  /// `my_node` must be the Node that created this Subscription, or the length
  /// of the wait is undefined.
  pub fn wait_for_publisher(&self, my_node: &Node) -> impl Future<Output = ()> + Send {
    my_node.wait_for_writer(self.guid())
  }
}

// helper
#[inline]
fn dcc_to_value_and_messageinfo<M>(dcc: no_key::DeserializedCacheChange<M>) -> (M, MessageInfo) {
  let mi = MessageInfo::from(&dcc);
  (dcc.into_value(), mi)
}

#[cfg(feature = "mio_06")]
impl<D> Evented for Subscription<D>
where
  D: DeserializeOwned,
{
  // We just delegate all the operations to datareader, since it
  // already implements Evented
  fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
    self.datareader.register(poll, token, interest, opts)
  }

  fn reregister(
    &self,
    poll: &Poll,
    token: Token,
    interest: Ready,
    opts: PollOpt,
  ) -> io::Result<()> {
    self.datareader.reregister(poll, token, interest, opts)
  }

  fn deregister(&self, poll: &Poll) -> io::Result<()> {
    self.datareader.deregister(poll)
  }
}

#[cfg(feature = "mio_1")]
impl<D> mio1::event::Source for Subscription<D>
where
  D: DeserializeOwned,
{
  fn register(
    &mut self,
    registry: &mio1::Registry,
    token: mio1::Token,
    interests: mio1::Interest,
  ) -> io::Result<()> {
    mio1::event::Source::register(&mut self.datareader, registry, token, interests)
  }

  fn reregister(
    &mut self,
    registry: &mio1::Registry,
    token: mio1::Token,
    interests: mio1::Interest,
  ) -> io::Result<()> {
    mio1::event::Source::reregister(&mut self.datareader, registry, token, interests)
  }

  fn deregister(&mut self, registry: &mio1::Registry) -> io::Result<()> {
    mio1::event::Source::deregister(&mut self.datareader, registry)
  }
}

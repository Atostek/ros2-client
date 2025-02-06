use std::io;

use mio::{Evented, Poll, PollOpt, Ready, Token};
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use futures::{pin_mut, stream::FusedStream, StreamExt};
use rustdds::{
  dds::{CreateResult, ReadError, ReadResult, WriteResult},
  rpc::*,
  *,
};

use crate::{message_info::MessageInfo, node::Node, service::*};

// --------------------------------------------
// --------------------------------------------
/// Server end of a ROS2 Service
pub struct Server<S>
where
  S: Service,
  S::Request: Message,
  S::Response: Message,
{
  service_mapping: ServiceMapping,
  request_receiver: SimpleDataReaderR<RequestWrapper<S::Request>>,
  response_sender: DataWriterR<ResponseWrapper<S::Response>>,
}

impl<S> Server<S>
where
  S: 'static + Service,
{
  pub(crate) fn new(
    service_mapping: ServiceMapping,
    node: &mut Node,
    request_topic: &Topic,
    response_topic: &Topic,
    qos_request: Option<QosPolicies>,
    qos_response: Option<QosPolicies>,
  ) -> CreateResult<Self> {
    let request_receiver =
      node.create_simpledatareader
      ::<RequestWrapper<S::Request>, ServiceDeserializerAdapter<RequestWrapper<S::Request>>>(
        request_topic, qos_request)?;
    let response_sender =
      node.create_datawriter
      ::<ResponseWrapper<S::Response>, ServiceSerializerAdapter<ResponseWrapper<S::Response>>>(
        response_topic, qos_response)?;

    debug!(
      "Created new Server: requests={} response={}",
      request_topic.name(),
      response_topic.name()
    );

    Ok(Server::<S> {
      service_mapping,
      request_receiver,
      response_sender,
    })
  }

  /// Receive a request from Client.
  /// Returns `Ok(None)` if no new requests have arrived.
  pub fn receive_request(&self) -> ReadResult<Option<(RmwRequestId, S::Request)>> {
    self.request_receiver.drain_read_notifications();
    let dcc_rw: Option<no_key::DeserializedCacheChange<RequestWrapper<S::Request>>> =
      self.request_receiver.try_take_one()?;

    match dcc_rw {
      None => Ok(None),
      Some(dcc) => {
        let mi = MessageInfo::from(&dcc);
        let req_wrapper = dcc.into_value();
        let (ri, req) = req_wrapper.unwrap(self.service_mapping, &mi)?;
        Ok(Some((ri, req)))
      }
    } // match
  }

  /// Send response to request by Client.
  /// rmw_req_id identifies request being responded.
  pub fn send_response(
    &self,
    rmw_req_id: RmwRequestId,
    response: S::Response,
  ) -> WriteResult<(), ()> {
    let resp_wrapper = ResponseWrapper::<S::Response>::new(
      self.service_mapping,
      rmw_req_id,
      RepresentationIdentifier::CDR_LE,
      response,
    )?;
    let write_opts = WriteOptionsBuilder::new()
      .source_timestamp(Timestamp::now()) // always add source timestamp
      .related_sample_identity(SampleIdentity::from(rmw_req_id))
      // TODO: Check if this is right. Cyclone mapping does not send
      // Related Sample Identity in
      // WriteOptions (QoS ParameterList), but within data payload.
      // But maybe it is not harmful to send it in both?
      .build();
    self
      .response_sender
      .write_with_options(resp_wrapper, write_opts)
      .map(|_| ())
      .map_err(|e| e.forget_data()) // lose SampleIdentity result
  }

  /// The request_id must be sent back with the response to identify which
  /// request and response belong together.
  pub async fn async_receive_request(&self) -> ReadResult<(RmwRequestId, S::Request)> {
    let dcc_stream = self.request_receiver.as_async_stream();
    pin_mut!(dcc_stream);

    match dcc_stream.next().await {
      Some(Err(e)) => Err(e),
      Some(Ok(dcc)) => {
        let mi = MessageInfo::from(&dcc);
        let req_wrapper = dcc.into_value();
        let (ri, req) = req_wrapper.unwrap(self.service_mapping, &mi)?;
        debug!("async_receive_request: {ri:?}");
        Ok((ri, req))
      }
      // This should never occur, because topic do not "end".
      None => read_error_internal!("SimpleDataReader value stream unexpectedly ended!"),
    } // match
  }

  /// Returns a never-ending stream of (request_id, request)
  /// The request_id must be sent back with the response to identify which
  /// request and response belong together.
  pub fn receive_request_stream(
    &self,
  ) -> impl FusedStream<Item = ReadResult<(RmwRequestId, S::Request)>> + '_ {
    Box::pin(self.request_receiver.as_async_stream().then(
      move |dcc_r| async move {
        match dcc_r {
          Err(e) => Err(e),
          Ok(dcc) => {
            let mi = MessageInfo::from(&dcc);
            let req_wrapper = dcc.into_value();
            debug!("receive_request_stream: messageinfo={mi:?}");
            req_wrapper.unwrap(self.service_mapping, &mi)
          }
        } // match
      }, // async
    ))
  }

  /// Asynchronous response sending
  pub async fn async_send_response(
    &self,
    rmw_req_id: RmwRequestId,
    response: S::Response,
  ) -> dds::WriteResult<(), ()> {
    let resp_wrapper = ResponseWrapper::<S::Response>::new(
      self.service_mapping,
      rmw_req_id,
      RepresentationIdentifier::CDR_LE,
      response,
    )?;
    debug!("async_send_response: rmw_req_id = {rmw_req_id:?}");
    debug!(
      "async_send_response: related_sample_identity = {:?}",
      SampleIdentity::from(rmw_req_id)
    );
    let write_opts = WriteOptionsBuilder::new()
      .source_timestamp(Timestamp::now()) // always add source timestamp
      .related_sample_identity(SampleIdentity::from(rmw_req_id))
      // TODO: Check if this is right. Cyclone mapping does not send
      // Related Sample Identity in
      // WriteOptions (QoS ParameterList), but within data payload.
      // But maybe it is not harmful to send it in both?
      .build();
    self
      .response_sender
      .async_write_with_options(resp_wrapper, write_opts)
      .await
      .map(|_| ())
      .map_err(|e| e.forget_data()) // lose SampleIdentity result
  }
}

impl<S> Evented for Server<S>
where
  S: 'static + Service,
{
  fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
    self.request_receiver.register(poll, token, interest, opts)
  }

  fn reregister(
    &self,
    poll: &Poll,
    token: Token,
    interest: Ready,
    opts: PollOpt,
  ) -> io::Result<()> {
    self
      .request_receiver
      .reregister(poll, token, interest, opts)
  }

  fn deregister(&self, poll: &Poll) -> io::Result<()> {
    self.request_receiver.deregister(poll)
  }
}

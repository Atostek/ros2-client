use std::io;

use rustdds::{
  mio::{Evented, Poll, PollOpt, Ready, Token},
  rpc::*,
  *,
};
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use futures::{pin_mut, stream::FusedStream, StreamExt};

use crate::{
  error::{CreateResult, ReadError, ReadResult, WriteError, WriteResult},
  message::Message,
  message_info::MessageInfo,
  node::Node,
  qos::QosProfile,
  service::*,
};

// --------------------------------------------
// --------------------------------------------
/// Server end of a ROS2 Service
pub struct Server<Req, Resp>
where
  Req: Message,
  Resp: Message,
{
  service_mapping: ServiceMapping,
  request_receiver: SimpleDataReaderR<RequestWrapper<Req>>,
  response_sender: DataWriterR<ResponseWrapper<Resp>>,
}

impl<Req, Resp> Server<Req, Resp>
where
  Req: Message + 'static,
  Resp: Message + 'static,
{
  pub(crate) fn new(
    service_mapping: ServiceMapping,
    node: &mut Node,
    request_topic: &Topic,
    response_topic: &Topic,
    qos_request: Option<QosProfile>,
    qos_response: Option<QosProfile>,
  ) -> CreateResult<Self> {
    let request_receiver =
      node.create_simpledatareader
      ::<RequestWrapper<Req>, ServiceDeserializerAdapter<RequestWrapper<Req>>>(
        request_topic, qos_request)?;
    let response_sender = node
      .create_datawriter::<ResponseWrapper<Resp>, ServiceSerializerAdapter<ResponseWrapper<Resp>>>(
        response_topic,
        qos_response,
      )?;

    debug!(
      "Created new Server: requests={} response={}",
      request_topic.name(),
      response_topic.name()
    );

    Ok(Server::<Req, Resp> {
      service_mapping,
      request_receiver,
      response_sender,
    })
  }

  /// Receive a request from Client.
  /// Returns `Ok(None)` if no new requests have arrived.
  pub fn receive_request(&self) -> ReadResult<Option<(RmwRequestId, Req)>> {
    self.request_receiver.drain_read_notifications();
    let dcc_rw: Option<no_key::DeserializedCacheChange<RequestWrapper<Req>>> =
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
  pub fn send_response(&self, rmw_req_id: RmwRequestId, response: Resp) -> WriteResult<(), ()> {
    let resp_wrapper = ResponseWrapper::<Resp>::new(
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
      .map_err(|e| WriteError::from(e.forget_data())) // lose SampleIdentity
                                                      // result
  }

  /// The request_id must be sent back with the response to identify which
  /// request and response belong together.
  pub async fn async_receive_request(&self) -> ReadResult<(RmwRequestId, Req)> {
    let dcc_stream = self.request_receiver.as_async_stream();
    pin_mut!(dcc_stream);

    match dcc_stream.next().await {
      Some(Err(e)) => Err(e.into()),
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
  ) -> impl FusedStream<Item = ReadResult<(RmwRequestId, Req)>> + '_ {
    Box::pin(self.request_receiver.as_async_stream().then(
      move |dcc_r| async move {
        match dcc_r {
          Err(e) => Err(e.into()),
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
    response: Resp,
  ) -> WriteResult<(), ()> {
    let resp_wrapper = ResponseWrapper::<Resp>::new(
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
      .map_err(|e| WriteError::from(e.forget_data())) // lose SampleIdentity
                                                      // result
  }
}

impl<Req, Resp> Evented for Server<Req, Resp>
where
  Req: Message + 'static,
  Resp: Message + 'static,
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

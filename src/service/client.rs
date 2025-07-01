use std::{io, sync::atomic};

use mio::{Evented, Poll, PollOpt, Ready, Token};
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use futures::{join, pin_mut, StreamExt};
use rustdds::{
  dds::{CreateResult, ReadError, ReadResult, WriteError, WriteResult},
  rpc::*,
  *,
};

use crate::{message_info::MessageInfo, node::Node, service::*};

/// Client end of a ROS2 Service
pub struct Client<S>
where
  S: Service,
  S::Request: Message,
  S::Response: Message,
{
  service_mapping: ServiceMapping,
  request_sender: DataWriterR<RequestWrapper<S::Request>>,
  response_receiver: SimpleDataReaderR<ResponseWrapper<S::Response>>,
  sequence_number_gen: atomic::AtomicI64, // used by basic and cyclone
  client_guid: GUID,                      // used by the Cyclone ServiceMapping
}

impl<S> Client<S>
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
    let request_sender =
      node.create_datawriter
      ::<RequestWrapper<S::Request>, ServiceSerializerAdapter<RequestWrapper<S::Request>>>(
        request_topic, qos_request)?;
    let response_receiver =
      node.create_simpledatareader
      ::<ResponseWrapper<S::Response>, ServiceDeserializerAdapter<ResponseWrapper<S::Response>>>(
        response_topic, qos_response)?;

    debug!(
      "Created new Client: request={} response={}",
      request_topic.name(),
      response_topic.name()
    );
    let client_guid = request_sender.guid();
    Ok(Client::<S> {
      service_mapping,
      request_sender,
      response_receiver,
      sequence_number_gen: atomic::AtomicI64::new(SequenceNumber::default().into()),
      client_guid,
    })
  }

  /// Send a request to Service Server.
  /// The returned `RmwRequestId` is a token to identify the correct response.
  pub fn send_request(&self, request: S::Request) -> WriteResult<RmwRequestId, ()> {
    self.increment_sequence_number();
    let gen_rmw_req_id = RmwRequestId {
      writer_guid: self.client_guid,
      sequence_number: self.sequence_number(),
    };
    let req_wrapper = RequestWrapper::<S::Request>::new(
      self.service_mapping,
      gen_rmw_req_id,
      RepresentationIdentifier::CDR_LE,
      request,
    )?;
    let write_opts_builder = WriteOptionsBuilder::new().source_timestamp(Timestamp::now()); // always add source timestamp

    let write_opts_builder = if self.service_mapping == ServiceMapping::Enhanced {
      write_opts_builder
    } else {
      write_opts_builder.related_sample_identity(SampleIdentity::from(gen_rmw_req_id))
    };
    let sent_rmw_req_id = self
      .request_sender
      .write_with_options(req_wrapper, write_opts_builder.build())
      .map(RmwRequestId::from)
      .map_err(|e| e.forget_data())?;

    match self.service_mapping {
      ServiceMapping::Enhanced => Ok(sent_rmw_req_id),
      ServiceMapping::Basic | ServiceMapping::Cyclone => Ok(gen_rmw_req_id),
    }
  }

  /// Receive a response from Server
  /// Returns `Ok(None)` if no new responses have arrived.
  /// Note: The response may to someone else's request. Check received
  /// `RmWRequestId` against the one you got when sending request to identify
  /// the correct response. In case you receive someone else's response,
  /// please do receive again.
  pub fn receive_response(&self) -> ReadResult<Option<(RmwRequestId, S::Response)>> {
    self.response_receiver.drain_read_notifications();
    let dcc_rw: Option<no_key::DeserializedCacheChange<ResponseWrapper<S::Response>>> =
      self.response_receiver.try_take_one()?;

    match dcc_rw {
      None => Ok(None),
      Some(dcc) => {
        let mi = MessageInfo::from(&dcc);
        let res_wrapper = dcc.into_value();
        let (ri, res) = res_wrapper.unwrap(self.service_mapping, mi, self.client_guid)?;
        Ok(Some((ri, res)))
      }
    } // match
  }

  /// Send a request to Service Server asynchronously.
  /// The returned `RmwRequestId` is a token to identify the correct response.
  pub async fn async_send_request(&self, request: S::Request) -> WriteResult<RmwRequestId, ()> {
    let gen_rmw_req_id =
      // we do the req_id generation in an async block so that we do not generate
      // multiple sequence numbers if there are multiple polls to this function
      async {
        self.increment_sequence_number();
         RmwRequestId {
          writer_guid: self.client_guid,
          sequence_number: self.sequence_number(),
        }
      }.await;

    let req_wrapper = RequestWrapper::<S::Request>::new(
      self.service_mapping,
      gen_rmw_req_id,
      RepresentationIdentifier::CDR_LE,
      request,
    )?;
    let write_opts_builder = WriteOptionsBuilder::new().source_timestamp(Timestamp::now()); // always add source timestamp

    let write_opts_builder = if self.service_mapping == ServiceMapping::Enhanced {
      write_opts_builder
    } else {
      write_opts_builder.related_sample_identity(SampleIdentity::from(gen_rmw_req_id))
    };
    let sent_rmw_req_id = self
      .request_sender
      .async_write_with_options(req_wrapper, write_opts_builder.build())
      .await
      .map(RmwRequestId::from)
      .map_err(|e| e.forget_data())?;

    let req_id = match self.service_mapping {
      ServiceMapping::Enhanced => sent_rmw_req_id,
      ServiceMapping::Basic | ServiceMapping::Cyclone => gen_rmw_req_id,
    };
    debug!(
      "Sent Request {:?} to {:?}",
      req_id,
      self.request_sender.topic().name()
    );
    Ok(req_id)
  }

  /// Receive a response from Server
  /// The returned Future does not complete until the response has been
  /// received.
  pub async fn async_receive_response(&self, request_id: RmwRequestId) -> ReadResult<S::Response> {
    let dcc_stream = self.response_receiver.as_async_stream();
    pin_mut!(dcc_stream);

    loop {
      match dcc_stream.next().await {
        Some(Err(e)) => return Err(e),
        Some(Ok(dcc)) => {
          let mi = MessageInfo::from(&dcc);
          let (req_id, response) =
            dcc
              .into_value()
              .unwrap(self.service_mapping, mi, self.client_guid)?;
          if req_id == request_id {
            return Ok(response);
          } else {
            debug!(
              "Received response for someone else. expected={request_id:?}  received={req_id:?}"
            );
            continue; //
          }
        }
        // This should never occur, because topic do not "end".
        None => return read_error_internal!("SimpleDataReader value stream unexpectedly ended!"),
      }
    } // loop
  }

  pub async fn async_call_service(
    &self,
    request: S::Request,
  ) -> Result<S::Response, CallServiceError<()>> {
    let req_id = self.async_send_request(request).await?;
    self
      .async_receive_response(req_id)
      .await
      .map_err(CallServiceError::from)
  }

  /// Wait for a Server to be connected to the Request and Response topics.
  ///
  /// This does not distinguish between diagnostinc tools and actual servers.
  /// It is enough that someone has subscribed the Requests, and someone is
  /// a publisher for Responses.
  ///
  /// May panic, if the Node does not havea background Spinner running.
  pub async fn wait_for_service(&self, my_node: &Node) {
    join!(
      my_node.wait_for_reader(self.request_sender.guid()),
      my_node.wait_for_writer(self.response_receiver.guid())
    );
  }

  fn increment_sequence_number(&self) {
    self
      .sequence_number_gen
      .fetch_add(1, atomic::Ordering::Acquire);
  }

  fn sequence_number(&self) -> request_id::SequenceNumber {
    self
      .sequence_number_gen
      .load(atomic::Ordering::Acquire)
      .into()
  }
}

#[derive(Debug)]
pub enum CallServiceError<T> {
  WriteError(WriteError<T>),
  ReadError(ReadError),
}
impl<T> From<WriteError<T>> for CallServiceError<T> {
  fn from(value: WriteError<T>) -> Self {
    CallServiceError::WriteError(value)
  }
}
impl<T> From<ReadError> for CallServiceError<T> {
  fn from(value: ReadError) -> Self {
    CallServiceError::ReadError(value)
  }
}

impl<S> Evented for Client<S>
where
  S: 'static + Service,
{
  fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
    self.response_receiver.register(poll, token, interest, opts)
  }

  fn reregister(
    &self,
    poll: &Poll,
    token: Token,
    interest: Ready,
    opts: PollOpt,
  ) -> io::Result<()> {
    self
      .response_receiver
      .reregister(poll, token, interest, opts)
  }

  fn deregister(&self, poll: &Poll) -> io::Result<()> {
    self.response_receiver.deregister(poll)
  }
}

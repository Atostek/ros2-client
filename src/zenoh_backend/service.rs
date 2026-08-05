//! Zenoh services: `Client` and `Server` over queryable/get (E6).
//!
//! Mirrors `rmw_zenoh` (see `docs/zenoh_study/research/rmw_zenoh.md` §5 and
//! `docs/decisions/0006-services-over-queryable-get.md`):
//!
//! * A **server** declares a `complete` queryable on the service key and, for
//!   each incoming query, reads the request (CDR payload) and its attachment
//!   `(seq, ts, client_gid)`. It stores the pending [`zenoh::query::Query`]
//!   keyed by `(client_gid, seq)` and, when the response is ready, `reply`s
//!   with the CDR response plus an attachment echoing the client's seq + gid.
//! * A **client** issues a `get` on the service key (wildcard type-hash,
//!   liberal receive) carrying the request payload + attachment, and decodes
//!   the first OK reply.

use std::{
  collections::HashMap,
  marker::PhantomData,
  sync::{
    atomic::{AtomicI64, Ordering},
    Mutex,
  },
  time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Serialize};
use zenoh::{
  handlers::FifoChannelHandler,
  liveliness::LivelinessToken,
  query::{ConsolidationMode, Query, QueryTarget, Queryable},
  Session, Wait,
};

use super::{attachment::AttachmentData, cdr};

fn now_nanos() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_nanos() as i64)
    .unwrap_or(0)
}

/// Identifies a service request: the client's GID plus its sequence number.
/// (The Zenoh analogue of the DDS `RmwRequestId`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RmwRequestId {
  /// GID of the requesting client entity.
  pub writer_gid: [u8; 16],
  /// Client-assigned request sequence number.
  pub sequence_number: i64,
}

/// Errors from service calls.
#[derive(Debug)]
pub enum ServiceError {
  /// CDR (de)serialization failed.
  Cdr(cdr::CdrError),
  /// Underlying Zenoh error.
  Zenoh(zenoh::Error),
  /// A received request/reply lacked the expected payload or attachment.
  Malformed,
  /// The client received no reply.
  NoReply,
  /// `send_response` referenced an unknown request id.
  UnknownRequest,
}

impl std::fmt::Display for ServiceError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ServiceError::Cdr(e) => write!(f, "service: {e}"),
      ServiceError::Zenoh(e) => write!(f, "service: zenoh error: {e}"),
      ServiceError::Malformed => write!(f, "service: malformed request/reply"),
      ServiceError::NoReply => write!(f, "service: no reply received"),
      ServiceError::UnknownRequest => write!(f, "service: unknown request id"),
    }
  }
}
impl std::error::Error for ServiceError {}

fn gid_key(gid: [u8; 16]) -> u128 {
  u128::from_le_bytes(gid)
}

/// A ROS 2 service client over Zenoh.
pub struct Client<Req, Resp> {
  session: Session,
  /// The `get` selector (concrete service key).
  selector: String,
  seq: AtomicI64,
  client_gid: [u8; 16],
  /// Optional `get` timeout override (actions' `get_result` uses a long one).
  timeout: Option<std::time::Duration>,
  _liveliness_token: Option<LivelinessToken>,
  phantom: PhantomData<fn(Req) -> Resp>,
}

impl<Req: Serialize, Resp: DeserializeOwned> Client<Req, Resp> {
  pub(crate) fn new(
    session: Session,
    selector: String,
    client_gid: [u8; 16],
    liveliness_token: Option<LivelinessToken>,
  ) -> Self {
    Self::new_with_timeout(session, selector, client_gid, liveliness_token, None)
  }

  pub(crate) fn new_with_timeout(
    session: Session,
    selector: String,
    client_gid: [u8; 16],
    liveliness_token: Option<LivelinessToken>,
    timeout: Option<std::time::Duration>,
  ) -> Self {
    Self {
      session,
      selector,
      seq: AtomicI64::new(0),
      client_gid,
      timeout,
      _liveliness_token: liveliness_token,
      phantom: PhantomData,
    }
  }

  fn request_bytes(&self, req: &Req) -> Result<(Vec<u8>, zenoh::bytes::ZBytes), ServiceError> {
    let payload = cdr::to_cdr(req).map_err(ServiceError::Cdr)?;
    let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let attachment = AttachmentData {
      sequence_number: seq,
      source_timestamp: now_nanos(),
      source_gid: self.client_gid,
    }
    .to_zbytes();
    Ok((payload, attachment))
  }

  fn decode_reply(reply: &zenoh::query::Reply) -> Option<Result<Resp, ServiceError>> {
    let sample = reply.result().ok()?;
    let bytes = sample.payload().to_bytes();
    Some(cdr::from_cdr::<Resp>(&bytes).map_err(ServiceError::Cdr))
  }

  /// Call the service and wait (blocking) for the first reply.
  pub fn call(&self, req: Req) -> Result<Resp, ServiceError> {
    let (payload, attachment) = self.request_bytes(&req)?;
    let mut builder = self
      .session
      .get(&self.selector)
      .payload(payload)
      .attachment(attachment)
      .target(QueryTarget::AllComplete)
      .consolidation(ConsolidationMode::None);
    if let Some(t) = self.timeout {
      builder = builder.timeout(t);
    }
    let replies = builder.wait().map_err(ServiceError::Zenoh)?;
    while let Ok(reply) = replies.recv() {
      if let Some(res) = Self::decode_reply(&reply) {
        return res;
      }
    }
    Err(ServiceError::NoReply)
  }

  /// Call the service and await the first reply.
  pub async fn async_call(&self, req: Req) -> Result<Resp, ServiceError> {
    let (payload, attachment) = self.request_bytes(&req)?;
    let mut builder = self
      .session
      .get(&self.selector)
      .payload(payload)
      .attachment(attachment)
      .target(QueryTarget::AllComplete)
      .consolidation(ConsolidationMode::None);
    if let Some(t) = self.timeout {
      builder = builder.timeout(t);
    }
    let replies = builder.await.map_err(ServiceError::Zenoh)?;
    while let Ok(reply) = replies.recv_async().await {
      if let Some(res) = Self::decode_reply(&reply) {
        return res;
      }
    }
    Err(ServiceError::NoReply)
  }
}

/// A ROS 2 service server over Zenoh.
pub struct Server<Req, Resp> {
  queryable: Queryable<FifoChannelHandler<Query>>,
  pending: Mutex<HashMap<(u128, i64), Query>>,
  _liveliness_token: Option<LivelinessToken>,
  phantom: PhantomData<fn(Req) -> Resp>,
}

impl<Req: DeserializeOwned, Resp: Serialize> Server<Req, Resp> {
  pub(crate) fn new(
    queryable: Queryable<FifoChannelHandler<Query>>,
    liveliness_token: Option<LivelinessToken>,
  ) -> Self {
    Self {
      queryable,
      pending: Mutex::new(HashMap::new()),
      _liveliness_token: liveliness_token,
      phantom: PhantomData,
    }
  }

  fn accept(&self, query: Query) -> Result<(RmwRequestId, Req), ServiceError> {
    let payload = query.payload().ok_or(ServiceError::Malformed)?;
    let req = cdr::from_cdr::<Req>(&payload.to_bytes()).map_err(ServiceError::Cdr)?;
    let attachment = query.attachment().ok_or(ServiceError::Malformed)?;
    let a = AttachmentData::from_zbytes(attachment).map_err(|_| ServiceError::Malformed)?;
    let id = RmwRequestId {
      writer_gid: a.source_gid,
      sequence_number: a.sequence_number,
    };
    self
      .pending
      .lock()
      .unwrap()
      .insert((gid_key(a.source_gid), a.sequence_number), query);
    Ok((id, req))
  }

  /// Take the next pending request if one is immediately available.
  pub fn try_receive_request(&self) -> Result<Option<(RmwRequestId, Req)>, ServiceError> {
    match self.queryable.try_recv() {
      Ok(Some(query)) => self.accept(query).map(Some),
      Ok(None) => Ok(None),
      Err(_) => Ok(None),
    }
  }

  /// Await the next request.
  pub async fn async_receive_request(&self) -> Result<(RmwRequestId, Req), ServiceError> {
    let query = self
      .queryable
      .recv_async()
      .await
      .map_err(|_| ServiceError::NoReply)?;
    self.accept(query)
  }

  /// Send the response for a previously received request.
  pub fn send_response(&self, id: RmwRequestId, resp: Resp) -> Result<(), ServiceError> {
    let query = self
      .pending
      .lock()
      .unwrap()
      .remove(&(gid_key(id.writer_gid), id.sequence_number))
      .ok_or(ServiceError::UnknownRequest)?;
    let payload = cdr::to_cdr(&resp).map_err(ServiceError::Cdr)?;
    // Echo the request's seq + client gid; fresh reply timestamp.
    let attachment = AttachmentData {
      sequence_number: id.sequence_number,
      source_timestamp: now_nanos(),
      source_gid: id.writer_gid,
    }
    .to_zbytes();
    query
      .reply(query.key_expr().clone(), payload)
      .attachment(attachment)
      .wait()
      .map_err(ServiceError::Zenoh)?;
    Ok(())
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::{
    sync::{
      atomic::{AtomicBool, Ordering},
      Arc,
    },
    time::{Duration, Instant},
  };

  use serde::{Deserialize, Serialize};
  use zenoh::Config;

  use super::{Client, Server};
  use crate::{Context, ContextOptions, Name, NodeName, NodeOptions, ServiceTypeName};

  #[derive(Serialize, Deserialize)]
  struct AddTwoIntsRequest {
    a: i64,
    b: i64,
  }
  #[derive(Serialize, Deserialize)]
  struct AddTwoIntsResponse {
    sum: i64,
  }

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
  fn add_two_ints_roundtrip() {
    let srv_port = 17519;
    let cli_port = 17520;
    let srv_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(srv_port, None)))
        .unwrap();
    let cli_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(cli_port, Some(srv_port))),
    )
    .unwrap();

    let srv_node = srv_ctx.new_node(
      NodeName::new("/", "add_server").unwrap(),
      NodeOptions::new(),
    );
    let cli_node = cli_ctx.new_node(
      NodeName::new("/", "add_client").unwrap(),
      NodeOptions::new(),
    );
    let stype = ServiceTypeName::new("example_interfaces", "AddTwoInts");
    let name = Name::new("/", "add_two_ints").unwrap();

    let server: Server<AddTwoIntsRequest, AddTwoIntsResponse> =
      srv_node.create_server(&name, &stype).unwrap();
    let client: Client<AddTwoIntsRequest, AddTwoIntsResponse> =
      cli_node.create_client(&name, &stype).unwrap();

    // Run the server in a background thread: answer requests with a + b.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = stop.clone();
    let server_thread = std::thread::spawn(move || {
      let deadline = Instant::now() + Duration::from_secs(25);
      while !stop_srv.load(Ordering::Relaxed) && Instant::now() < deadline {
        if let Ok(Some((id, req))) = server.try_receive_request() {
          let _ = server.send_response(id, AddTwoIntsResponse { sum: req.a + req.b });
        }
        std::thread::sleep(Duration::from_millis(20));
      }
    });

    // Retry the call until the peers connect and the queryable is discovered.
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut sum = None;
    while Instant::now() < deadline {
      if let Ok(resp) = client.call(AddTwoIntsRequest { a: 2, b: 40 }) {
        sum = Some(resp.sum);
        break;
      }
      std::thread::sleep(Duration::from_millis(200));
    }

    stop.store(true, Ordering::Relaxed);
    let _ = server_thread.join();

    assert_eq!(sum, Some(42));
  }
}

//! Zenoh `Node` and `Topic` (E4).
//!
//! A minimal node surface over a shared [`Context`] session: it resolves ROS
//! names to key expressions and creates [`Publisher`]/[`Subscription`]
//! entities. Discovery (liveliness tokens, graph), services, and actions land
//! in E5/E6.

use std::{
  sync::atomic::{AtomicU64, Ordering},
  time::Duration,
};

use serde::{de::DeserializeOwned, Serialize};
use zenoh::{liveliness::LivelinessToken, Wait};

use super::{
  action::{
    ActionClient, ActionServer, FeedbackMessage, GetResultRequest, GetResultResponse,
    SendGoalRequest, SendGoalResponse,
  },
  context::Context,
  gid, keyexpr,
  parameters::{ParameterClient, ParameterEvent, ParameterServer},
  pubsub::{Publisher, Subscription},
  qos_encoding,
  rosout::{Log, Logger},
  service::{Client, Server},
  type_hash,
};
use crate::{
  action_msgs::{CancelGoalRequest, CancelGoalResponse, GoalStatusArray},
  names::{ActionTypeName, MessageTypeName, Name, NodeName, ServiceTypeName},
  parameters::Parameter,
  qos::QosProfile,
};

/// The `get_result` action service is queried with a long timeout, mirroring
/// rmw_zenoh's `**/_action/get_result/**` heuristic (a goal may take a long
/// time to finish). Bounded to avoid indefinite hangs; production may raise it.
const GET_RESULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Options for creating a [`Node`] on the Zenoh backend.
///
/// Currently a placeholder (parity with the DDS `NodeOptions`);
/// rosout/parameter toggles arrive with E8/E9.
#[derive(Clone, Debug, Default)]
pub struct NodeOptions {}

impl NodeOptions {
  /// Default node options.
  pub fn new() -> Self {
    Self {}
  }
}

/// A handle to a ROS 2 topic: its fully-qualified name, DDS-form type name, and
/// QoS. Produced by [`Node::create_topic`].
#[derive(Clone, Debug)]
pub struct Topic {
  fully_qualified_name: String,
  dds_type_name: String,
  qos: QosProfile,
}

impl Topic {
  /// The fully-qualified ROS topic name (e.g. `/chatter`).
  pub fn name(&self) -> &str {
    &self.fully_qualified_name
  }

  /// The DDS-form type name (e.g. `std_msgs::msg::dds_::String_`).
  pub fn type_name(&self) -> &str {
    &self.dds_type_name
  }

  /// The topic's QoS profile.
  pub fn qos(&self) -> &QosProfile {
    &self.qos
  }
}

/// A ROS 2 node backed by a shared Zenoh session.
pub struct Node {
  context: Context,
  node_name: NodeName,
  node_id: u64,
  zid: String,
  next_entity_id: AtomicU64,
  // Kept alive so the node stays discoverable; dropped => token undeclared.
  _node_token: Option<LivelinessToken>,
}

impl Node {
  pub(crate) fn new(
    context: Context,
    node_name: NodeName,
    node_id: u64,
    _options: NodeOptions,
  ) -> Self {
    let zid = context.session().zid().to_string();
    // Declare the node (NN) liveliness token so peers discover this node.
    let ids = keyexpr::EntityIds {
      session_id: &zid,
      node_id,
      entity_id: node_id,
      enclave: "",
      namespace: node_name.namespace(),
      node_name: node_name.base_name(),
    };
    let node_key = keyexpr::node_liveliness_keyexpr(context.domain_id(), &ids);
    let node_token = declare_liveliness(&context, node_key);
    Self {
      context,
      node_name,
      node_id,
      zid,
      next_entity_id: AtomicU64::new(0),
      _node_token: node_token,
    }
  }

  /// The node's name.
  pub fn name(&self) -> &NodeName {
    &self.node_name
  }

  /// The node's fully-qualified name (e.g. `/robot1/talker`).
  pub fn fully_qualified_name(&self) -> String {
    self.node_name.fully_qualified_name()
  }

  /// Create a [`Topic`] handle. `name` is resolved against the node namespace
  /// if relative.
  pub fn create_topic(&self, name: &Name, type_name: MessageTypeName, qos: &QosProfile) -> Topic {
    Topic {
      fully_qualified_name: resolve_fqn(name, &self.node_name),
      dds_type_name: type_name.dds_msg_type(),
      qos: qos.clone(),
    }
  }

  /// Create a publisher for `topic`. `qos` overrides the topic QoS if given.
  pub fn create_publisher<M: Serialize>(
    &self,
    topic: &Topic,
    qos: Option<QosProfile>,
  ) -> zenoh::Result<Publisher<M>> {
    let qos = qos.unwrap_or_else(|| topic.qos.clone());
    let domain = self.context.domain_id();
    let sender_hash = type_hash::sender_hash(&topic.dds_type_name);
    // A publisher `put`s on a concrete key (real hash if known, else placeholder).
    let key = keyexpr::topic_keyexpr(
      domain,
      &topic.fully_qualified_name,
      &topic.dds_type_name,
      sender_hash,
    );
    let zenoh_publisher = self.context.session().declare_publisher(key).wait()?;

    let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
    let liveliness_key = self.entity_liveliness_key(
      entity_id,
      keyexpr::EntityKind::Publisher,
      topic,
      sender_hash,
      &qos,
    );
    let source_gid = gid::gid_from_liveliness_key(&liveliness_key);
    let token = declare_liveliness(&self.context, liveliness_key);

    Ok(Publisher::new(zenoh_publisher, source_gid, token))
  }

  /// Create a subscription for `topic`. `qos` overrides the topic QoS if given.
  pub fn create_subscription<M: DeserializeOwned>(
    &self,
    topic: &Topic,
    qos: Option<QosProfile>,
  ) -> zenoh::Result<Subscription<M>> {
    let qos = qos.unwrap_or_else(|| topic.qos.clone());
    let domain = self.context.domain_id();
    // A subscription listens with a wildcard type-hash to receive from any
    // publisher regardless of its hash (liberal receive, ADR-0007).
    let key = keyexpr::topic_keyexpr(
      domain,
      &topic.fully_qualified_name,
      &topic.dds_type_name,
      type_hash::WILDCARD,
    );
    let zenoh_subscriber = self.context.session().declare_subscriber(key).wait()?;

    // The liveliness token, unlike the data key, is concrete (real-or-placeholder
    // hash) — it describes this entity for discovery.
    let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
    let sub_hash = type_hash::sender_hash(&topic.dds_type_name);
    let liveliness_key = self.entity_liveliness_key(
      entity_id,
      keyexpr::EntityKind::Subscription,
      topic,
      sub_hash,
      &qos,
    );
    let token = declare_liveliness(&self.context, liveliness_key);

    Ok(Subscription::new(zenoh_subscriber, token))
  }

  /// Build the liveliness key for a pub/sub entity of this node.
  fn entity_liveliness_key(
    &self,
    entity_id: u64,
    kind: keyexpr::EntityKind,
    topic: &Topic,
    hash: &str,
    qos: &QosProfile,
  ) -> String {
    self.liveliness_key(
      entity_id,
      kind,
      &topic.fully_qualified_name,
      &topic.dds_type_name,
      hash,
      qos,
    )
  }

  /// Build the liveliness key for any entity of this node, including the
  /// compact QoS encoding.
  fn liveliness_key(
    &self,
    entity_id: u64,
    kind: keyexpr::EntityKind,
    fqn: &str,
    dds_type: &str,
    hash: &str,
    qos: &QosProfile,
  ) -> String {
    let ids = keyexpr::EntityIds {
      session_id: &self.zid,
      node_id: self.node_id,
      entity_id,
      enclave: "",
      namespace: self.node_name.namespace(),
      node_name: self.node_name.base_name(),
    };
    keyexpr::entity_liveliness_keyexpr(
      self.context.domain_id(),
      &ids,
      kind,
      fqn,
      dds_type,
      hash,
      &qos_encoding::encode_qos(qos),
    )
  }

  /// Create a service client for `service` of the given service type.
  pub fn create_client<Req: serde::Serialize, Resp: DeserializeOwned>(
    &self,
    service: &Name,
    service_type: &ServiceTypeName,
  ) -> zenoh::Result<Client<Req, Resp>> {
    self.create_client_inner(service, service_type, None)
  }

  fn create_client_inner<Req: serde::Serialize, Resp: DeserializeOwned>(
    &self,
    service: &Name,
    service_type: &ServiceTypeName,
    timeout: Option<Duration>,
  ) -> zenoh::Result<Client<Req, Resp>> {
    let fqn = resolve_fqn(service, &self.node_name);
    let dds_type = service_type.dds_service_type();
    let domain = self.context.domain_id();
    // The client `get`s on the concrete service key (real hash if known, else
    // placeholder) so it matches the server's concrete queryable. A `get`
    // selector must match the queryable's key; for known interop types this is
    // the real RIHS01 hash and so also matches C++ servers.
    let hash = type_hash::sender_hash(&dds_type);
    let selector = keyexpr::topic_keyexpr(domain, &fqn, &dds_type, hash);

    let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
    let liveliness_key = self.liveliness_key(
      entity_id,
      keyexpr::EntityKind::ServiceClient,
      &fqn,
      &dds_type,
      hash,
      &QosProfile::default(),
    );
    let client_gid = gid::gid_from_liveliness_key(&liveliness_key);
    let token = declare_liveliness(&self.context, liveliness_key);

    Ok(Client::new_with_timeout(
      self.context.session().clone(),
      selector,
      client_gid,
      token,
      timeout,
    ))
  }

  /// Create a service server for `service` of the given service type.
  pub fn create_server<Req: DeserializeOwned, Resp: serde::Serialize>(
    &self,
    service: &Name,
    service_type: &ServiceTypeName,
  ) -> zenoh::Result<Server<Req, Resp>> {
    let fqn = resolve_fqn(service, &self.node_name);
    let dds_type = service_type.dds_service_type();
    let domain = self.context.domain_id();
    let hash = type_hash::sender_hash(&dds_type);
    // The server's queryable is concrete (real-or-placeholder hash).
    let key = keyexpr::topic_keyexpr(domain, &fqn, &dds_type, hash);
    let queryable = self
      .context
      .session()
      .declare_queryable(key)
      .complete(true)
      .wait()?;

    let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
    let liveliness_key = self.liveliness_key(
      entity_id,
      keyexpr::EntityKind::ServiceServer,
      &fqn,
      &dds_type,
      hash,
      &QosProfile::default(),
    );
    let token = declare_liveliness(&self.context, liveliness_key);

    Ok(Server::new(queryable, token))
  }

  /// Create an action client for `action` of the given action type.
  pub fn create_action_client<G, R, F>(
    &self,
    action: &Name,
    action_type: &ActionTypeName,
  ) -> zenoh::Result<ActionClient<G, R, F>>
  where
    G: Serialize,
    R: DeserializeOwned,
    F: DeserializeOwned,
  {
    let fqn = resolve_fqn(action, &self.node_name);
    let send_goal = self.create_client::<SendGoalRequest<G>, SendGoalResponse>(
      &action_sub_name(&fqn, "send_goal")?,
      &action_type.dds_action_service("_SendGoal"),
    )?;
    let get_result = self.create_client_inner::<GetResultRequest, GetResultResponse<R>>(
      &action_sub_name(&fqn, "get_result")?,
      &action_type.dds_action_service("_GetResult"),
      Some(GET_RESULT_TIMEOUT),
    )?;
    let feedback_topic = self.create_topic(
      &action_sub_name(&fqn, "feedback")?,
      action_type.dds_action_topic("_FeedbackMessage"),
      &QosProfile::default(),
    );
    let feedback = self.create_subscription::<FeedbackMessage<F>>(&feedback_topic, None)?;
    // cancel_goal + status use the shared `action_msgs` types (not
    // action-namespaced), matching rmw_zenoh / the DDS backend.
    let cancel_goal = self.create_client::<CancelGoalRequest, CancelGoalResponse>(
      &action_sub_name(&fqn, "cancel_goal")?,
      &ServiceTypeName::new("action_msgs", "CancelGoal"),
    )?;
    let status_topic = self.create_topic(
      &action_sub_name(&fqn, "status")?,
      MessageTypeName::new("action_msgs", "GoalStatusArray"),
      &QosProfile::default(),
    );
    let status = self.create_subscription::<GoalStatusArray>(&status_topic, None)?;
    Ok(ActionClient::new(
      send_goal,
      get_result,
      feedback,
      cancel_goal,
      status,
    ))
  }

  /// Create an action server for `action` of the given action type.
  pub fn create_action_server<G, R, F>(
    &self,
    action: &Name,
    action_type: &ActionTypeName,
  ) -> zenoh::Result<ActionServer<G, R, F>>
  where
    G: DeserializeOwned,
    R: Serialize,
    F: Serialize,
  {
    let fqn = resolve_fqn(action, &self.node_name);
    let send_goal = self.create_server::<SendGoalRequest<G>, SendGoalResponse>(
      &action_sub_name(&fqn, "send_goal")?,
      &action_type.dds_action_service("_SendGoal"),
    )?;
    let get_result = self.create_server::<GetResultRequest, GetResultResponse<R>>(
      &action_sub_name(&fqn, "get_result")?,
      &action_type.dds_action_service("_GetResult"),
    )?;
    let feedback_topic = self.create_topic(
      &action_sub_name(&fqn, "feedback")?,
      action_type.dds_action_topic("_FeedbackMessage"),
      &QosProfile::default(),
    );
    let feedback = self.create_publisher::<FeedbackMessage<F>>(&feedback_topic, None)?;
    let cancel_goal = self.create_server::<CancelGoalRequest, CancelGoalResponse>(
      &action_sub_name(&fqn, "cancel_goal")?,
      &ServiceTypeName::new("action_msgs", "CancelGoal"),
    )?;
    let status_topic = self.create_topic(
      &action_sub_name(&fqn, "status")?,
      MessageTypeName::new("action_msgs", "GoalStatusArray"),
      &QosProfile::default(),
    );
    let status = self.create_publisher::<GoalStatusArray>(&status_topic, None)?;
    Ok(ActionServer::new(
      send_goal,
      get_result,
      feedback,
      cancel_goal,
      status,
    ))
  }

  /// Create a [`ParameterServer`] for this node: the six `rcl_interfaces`
  /// parameter services (named under this node, e.g.
  /// `/talker/get_parameters`) plus the global `/parameter_events` publisher.
  ///
  /// `initial_parameters` seeds the parameter store (`use_sim_time` is always
  /// added). Drive the returned server with
  /// [`ParameterServer::spin_once`]/[`spin`](ParameterServer::spin).
  pub fn create_parameter_server(
    &self,
    initial_parameters: impl IntoIterator<Item = Parameter>,
  ) -> zenoh::Result<ParameterServer> {
    let node_fqn = self.node_name.fully_qualified_name();
    let svc = |base: &str| param_service_name(&node_fqn, base);
    let param_type = |ty: &str| ServiceTypeName::new("rcl_interfaces", ty);

    let get_parameters =
      self.create_server(&svc("get_parameters")?, &param_type("GetParameters"))?;
    let get_parameter_types = self.create_server(
      &svc("get_parameter_types")?,
      &param_type("GetParameterTypes"),
    )?;
    let set_parameters =
      self.create_server(&svc("set_parameters")?, &param_type("SetParameters"))?;
    let set_parameters_atomically = self.create_server(
      &svc("set_parameters_atomically")?,
      &param_type("SetParametersAtomically"),
    )?;
    let list_parameters =
      self.create_server(&svc("list_parameters")?, &param_type("ListParameters"))?;
    let describe_parameters = self.create_server(
      &svc("describe_parameters")?,
      &param_type("DescribeParameters"),
    )?;

    let events_topic = self.create_topic(
      &Name::new("/", "parameter_events").map_err(name_err)?,
      MessageTypeName::new("rcl_interfaces", "ParameterEvent"),
      &QosProfile::default(),
    );
    let events = self.create_publisher::<ParameterEvent>(&events_topic, None)?;

    Ok(ParameterServer::new(
      node_fqn,
      initial_parameters,
      get_parameters,
      get_parameter_types,
      set_parameters,
      set_parameters_atomically,
      list_parameters,
      describe_parameters,
      events,
    ))
  }

  /// Create a [`ParameterClient`] targeting `remote_node`'s parameter services.
  pub fn create_parameter_client(&self, remote_node: &NodeName) -> zenoh::Result<ParameterClient> {
    let remote_fqn = remote_node.fully_qualified_name();
    let svc = |base: &str| param_service_name(&remote_fqn, base);
    let param_type = |ty: &str| ServiceTypeName::new("rcl_interfaces", ty);

    let get_parameters =
      self.create_client(&svc("get_parameters")?, &param_type("GetParameters"))?;
    let get_parameter_types = self.create_client(
      &svc("get_parameter_types")?,
      &param_type("GetParameterTypes"),
    )?;
    let set_parameters =
      self.create_client(&svc("set_parameters")?, &param_type("SetParameters"))?;
    let list_parameters =
      self.create_client(&svc("list_parameters")?, &param_type("ListParameters"))?;
    let describe_parameters = self.create_client(
      &svc("describe_parameters")?,
      &param_type("DescribeParameters"),
    )?;

    Ok(ParameterClient::new(
      get_parameters,
      get_parameter_types,
      set_parameters,
      list_parameters,
      describe_parameters,
    ))
  }

  /// Create a rosout [`Logger`] for this node: a publisher on the global
  /// `/rosout` topic (`rcl_interfaces/msg/Log`). Records are stamped with this
  /// node's base name. Use the [`rosout!`](crate::rosout!) macro to log.
  pub fn create_logger(&self) -> zenoh::Result<Logger> {
    let publisher = self.create_publisher::<Log>(&self.rosout_topic()?, None)?;
    Ok(Logger::new(
      self.node_name.base_name().to_string(),
      publisher,
    ))
  }

  /// Subscribe to the global `/rosout` topic to read log records published by
  /// any node (the `read_rosout` capability).
  pub fn read_rosout(&self) -> zenoh::Result<Subscription<Log>> {
    self.create_subscription::<Log>(&self.rosout_topic()?, None)
  }

  fn rosout_topic(&self) -> zenoh::Result<Topic> {
    Ok(self.create_topic(
      &Name::new("/", "rosout").map_err(name_err)?,
      MessageTypeName::new("rcl_interfaces", "Log"),
      &QosProfile::default(),
    ))
  }

  /// Resolve once at least one publisher on `topic` is discovered (delegates to
  /// [`Context::wait_for_publisher`](crate::Context::wait_for_publisher)).
  pub async fn wait_for_publisher(&self, topic: &str) {
    self.context.wait_for_publisher(topic).await
  }

  /// Resolve once at least one subscription on `topic` is discovered (delegates
  /// to [`Context::wait_for_subscription`](crate::Context::wait_for_subscription)).
  pub async fn wait_for_subscription(&self, topic: &str) {
    self.context.wait_for_subscription(topic).await
  }
}

/// Build the absolute `Name` of a parameter service, e.g.
/// node fqn `/talker` + `get_parameters` → `/talker/get_parameters`.
fn param_service_name(node_fqn: &str, base: &str) -> zenoh::Result<Name> {
  Name::new(node_fqn, base).map_err(name_err)
}

fn name_err(e: crate::names::NameError) -> zenoh::Error {
  Box::new(e)
}

/// Build the absolute `Name` of an action sub-entity, e.g.
/// `/fibonacci` + `send_goal` → `/fibonacci/_action/send_goal`.
fn action_sub_name(action_fqn: &str, sub: &str) -> zenoh::Result<Name> {
  Name::new(&format!("{action_fqn}/_action"), sub).map_err(|e| -> zenoh::Error { Box::new(e) })
}

/// Declare a liveliness token on `key`, best-effort: a failure logs a warning
/// and returns `None` (the entity still works; it just isn't discoverable).
fn declare_liveliness(context: &Context, key: String) -> Option<LivelinessToken> {
  match context
    .session()
    .liveliness()
    .declare_token(key.clone())
    .wait()
  {
    Ok(token) => Some(token),
    Err(e) => {
      log::warn!("failed to declare liveliness token for {key}: {e}");
      None
    }
  }
}

/// Resolve a topic [`Name`] to its fully-qualified form against a node name.
/// Absolute names are used as-is; relative names are prefixed with the node
/// namespace.
fn resolve_fqn(name: &Name, node: &NodeName) -> String {
  if name.is_absolute() {
    format!("{name}")
  } else {
    let ns = node.namespace();
    if ns == "/" {
      format!("/{name}")
    } else {
      format!("{ns}/{name}")
    }
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use zenoh::{Config, Wait};

  use super::{
    keyexpr::{graph_cache_keyexpr, parse_liveliness_key, EntityKind},
    *,
  };
  use crate::{Context, ContextOptions};

  #[test]
  fn fqn_resolution() {
    let node = NodeName::new("/robot1", "talker").unwrap();
    let rel = Name::new("", "chatter").unwrap();
    assert_eq!(resolve_fqn(&rel, &node), "/robot1/chatter");

    let root_node = NodeName::new("/", "talker").unwrap();
    assert_eq!(resolve_fqn(&rel, &root_node), "/chatter");

    let abs = Name::new("/", "chatter").unwrap();
    assert_eq!(resolve_fqn(&abs, &node), "/chatter");
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

  // A publisher created on one context is discovered by another via its
  // liveliness tokens (node NN + publisher MP), parsed back correctly.
  #[test]
  fn discovers_entities_via_liveliness() {
    let a_port = 17515;
    let b_port = 17516;

    let ctx_a =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(a_port, None))).unwrap();
    let ctx_b =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(b_port, Some(a_port))))
        .unwrap();

    let node_a = ctx_a.new_node(NodeName::new("/", "talker").unwrap(), NodeOptions::new());
    let topic = node_a.create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &QosProfile::publisher_default(),
    );
    let _publisher: Publisher<String> = node_a.create_publisher(&topic, None).unwrap();

    let graph_key = graph_cache_keyexpr(ctx_b.domain_id());
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut saw_node = false;
    let mut saw_publisher = false;

    while Instant::now() < deadline && !(saw_node && saw_publisher) {
      let replies = ctx_b.session().liveliness().get(&graph_key).wait().unwrap();
      while let Ok(reply) = replies.recv() {
        if let Ok(sample) = reply.result() {
          if let Some(e) = parse_liveliness_key(sample.key_expr().as_str()) {
            match e.kind {
              EntityKind::Node if e.node_name == "talker" => saw_node = true,
              EntityKind::Publisher
                if e.topic_name.as_deref() == Some("/chatter") && e.node_name == "talker" =>
              {
                saw_publisher = true
              }
              _ => {}
            }
          }
        }
      }
      if saw_node && saw_publisher {
        break;
      }
      std::thread::sleep(Duration::from_millis(100));
    }

    assert!(saw_node, "did not discover the talker node token");
    assert!(
      saw_publisher,
      "did not discover the /chatter publisher token"
    );
  }
}

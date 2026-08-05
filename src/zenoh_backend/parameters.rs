//! Zenoh parameters (E8).
//!
//! ROS 2 parameters are not a middleware primitive — a node that "has
//! parameters" simply exposes six ordinary services and a `parameter_events`
//! topic (see `docs/zenoh_study/research/ros2_client_internals.md` and
//! `rcl_interfaces`). This module builds that composition on top of the Zenoh
//! service ([`super::service`]) and pub/sub ([`super::pubsub`]) layers, so the
//! `ros2 param list/get/set` tooling — and rclcpp/rclpy parameter clients —
//! interoperate with a `ros2-client` node.
//!
//! * `get_parameters` / `get_parameter_types` / `set_parameters` /
//!   `set_parameters_atomically` / `list_parameters` / `describe_parameters`
//!   services (`rcl_interfaces/srv/*`)
//! * `parameter_events` topic (`rcl_interfaces/msg/ParameterEvent`)
//!
//! The event message carries an owned [`builtin_interfaces::Time`] timestamp,
//! keeping the Zenoh backend independent of RustDDS (ADR-0004); the DDS
//! backend's `parameters::raw::ParameterEvent` uses `rustdds::Timestamp`
//! instead.
//!
//! Like the DDS backend's parameter machinery (which runs inside a `Spinner`),
//! the server here is driven by the application: call [`ParameterServer::spin`]
//! (or [`ParameterServer::spin_once`]) from your event loop to answer pending
//! requests. `set_parameters_atomically` is not implemented (it returns a
//! failure result, mirroring the DDS backend).

use std::{collections::BTreeMap, sync::Mutex};

use serde::{Deserialize, Serialize};

use super::{
  pubsub::Publisher,
  service::{Client, Server, ServiceError},
};
use crate::{
  builtin_interfaces::Time,
  parameters::{raw, Parameter, ParameterDescriptor, ParameterValue, SetParametersResult},
  rcl_interfaces::{
    DescribeParametersRequest, DescribeParametersResponse, GetParameterTypesRequest,
    GetParameterTypesResponse, GetParametersRequest, GetParametersResponse, ListParametersRequest,
    ListParametersResponse, ListParametersResult, SetParametersRequest, SetParametersResponse,
  },
};

/// `rcl_interfaces/msg/ParameterEvent` with an owned
/// [`builtin_interfaces::Time`] timestamp.
///
/// The field layout (and thus the CDR wire format) matches ROS 2's
/// `ParameterEvent.msg`: `builtin_interfaces/Time stamp`, `string node`, and
/// three `Parameter[]` arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterEvent {
  /// When the change happened.
  pub stamp: Time,
  /// Fully-qualified name of the node whose parameters changed.
  pub node: String,
  /// Parameters that were newly declared.
  pub new_parameters: Vec<raw::Parameter>,
  /// Parameters whose value changed.
  pub changed_parameters: Vec<raw::Parameter>,
  /// Parameters that were removed.
  pub deleted_parameters: Vec<raw::Parameter>,
}

/// A ROS 2 parameter server over Zenoh: a parameter store plus the six
/// `rcl_interfaces` services and the `parameter_events` publisher.
///
/// Create one with
/// [`Node::create_parameter_server`](crate::Node::create_parameter_server),
/// then call [`spin`](Self::spin) (or repeatedly
/// [`spin_once`](Self::spin_once)) to service incoming requests.
pub struct ParameterServer {
  node_fqn: String,
  store: Mutex<BTreeMap<String, ParameterValue>>,
  get_parameters: Server<GetParametersRequest, GetParametersResponse>,
  get_parameter_types: Server<GetParameterTypesRequest, GetParameterTypesResponse>,
  set_parameters: Server<SetParametersRequest, SetParametersResponse>,
  set_parameters_atomically: Server<SetParametersRequest, SetParametersResponse>,
  list_parameters: Server<ListParametersRequest, ListParametersResponse>,
  describe_parameters: Server<DescribeParametersRequest, DescribeParametersResponse>,
  events: Publisher<ParameterEvent>,
}

impl ParameterServer {
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn new(
    node_fqn: String,
    initial_parameters: impl IntoIterator<Item = Parameter>,
    get_parameters: Server<GetParametersRequest, GetParametersResponse>,
    get_parameter_types: Server<GetParameterTypesRequest, GetParameterTypesResponse>,
    set_parameters: Server<SetParametersRequest, SetParametersResponse>,
    set_parameters_atomically: Server<SetParametersRequest, SetParametersResponse>,
    list_parameters: Server<ListParametersRequest, ListParametersResponse>,
    describe_parameters: Server<DescribeParametersRequest, DescribeParametersResponse>,
    events: Publisher<ParameterEvent>,
  ) -> Self {
    let mut store: BTreeMap<String, ParameterValue> = initial_parameters
      .into_iter()
      .map(|Parameter { name, value }| (name, value))
      .collect();
    // Every node carries `use_sim_time` (default false), like rclcpp/rclpy.
    store
      .entry("use_sim_time".to_string())
      .or_insert(ParameterValue::Boolean(false));
    Self {
      node_fqn,
      store: Mutex::new(store),
      get_parameters,
      get_parameter_types,
      set_parameters,
      set_parameters_atomically,
      list_parameters,
      describe_parameters,
      events,
    }
  }

  // --- local (in-process) parameter access ---------------------------------

  /// Get a parameter's value, or `None` if it is not set.
  pub fn get_parameter(&self, name: &str) -> Option<ParameterValue> {
    self.store.lock().unwrap().get(name).cloned()
  }

  /// Set a parameter locally and publish a `parameter_events` notification.
  pub fn set_parameter(&self, name: &str, value: ParameterValue) {
    let _ = self.set_and_notify(name, value);
  }

  /// The names of all currently-set parameters.
  pub fn parameter_names(&self) -> Vec<String> {
    self.store.lock().unwrap().keys().cloned().collect()
  }

  fn set_and_notify(&self, name: &str, value: ParameterValue) -> SetParametersResult {
    let raw_param = raw::Parameter {
      name: name.to_string(),
      value: value.clone().into(),
    };
    let already_set = {
      let mut db = self.store.lock().unwrap();
      let already = db.contains_key(name);
      db.insert(name.to_string(), value);
      already
    };
    if already_set {
      self.publish_event(vec![], vec![raw_param], vec![]);
    } else {
      self.publish_event(vec![raw_param], vec![], vec![]);
    }
    Ok(())
  }

  fn publish_event(
    &self,
    new_parameters: Vec<raw::Parameter>,
    changed_parameters: Vec<raw::Parameter>,
    deleted_parameters: Vec<raw::Parameter>,
  ) {
    let event = ParameterEvent {
      stamp: Time::now(),
      node: self.node_fqn.clone(),
      new_parameters,
      changed_parameters,
      deleted_parameters,
    };
    if let Err(e) = self.events.publish(event) {
      log::warn!("parameter_events publish failed: {e}");
    }
  }

  // --- request servicing ---------------------------------------------------

  /// Answer every request currently pending on the six parameter services.
  /// Non-blocking: returns once each service's queue is drained.
  pub fn spin_once(&self) {
    self.handle_get_parameters();
    self.handle_get_parameter_types();
    self.handle_set_parameters();
    self.handle_set_parameters_atomically();
    self.handle_list_parameters();
    self.handle_describe_parameters();
  }

  /// Await and answer parameter requests until `stop()` returns `true`,
  /// polling roughly every `poll` duration.
  ///
  /// This is a convenience loop for applications that dedicate a thread to the
  /// parameter server; integrate [`spin_once`](Self::spin_once) into your own
  /// event loop instead if you already have one.
  pub fn spin(&self, poll: std::time::Duration, mut stop: impl FnMut() -> bool) {
    while !stop() {
      self.spin_once();
      std::thread::sleep(poll);
    }
  }

  fn handle_get_parameters(&self) {
    while let Ok(Some((id, req))) = self.get_parameters.try_receive_request() {
      let values = {
        let db = self.store.lock().unwrap();
        req
          .names
          .iter()
          .map(|name| db.get(name).cloned().unwrap_or(ParameterValue::NotSet))
          .map(raw::ParameterValue::from)
          .collect()
      };
      if let Err(e) = self
        .get_parameters
        .send_response(id, GetParametersResponse { values })
      {
        log::warn!("get_parameters response failed: {e}");
      }
    }
  }

  fn handle_get_parameter_types(&self) {
    while let Ok(Some((id, req))) = self.get_parameter_types.try_receive_request() {
      let values = {
        let db = self.store.lock().unwrap();
        req
          .names
          .iter()
          .map(|name| db.get(name).cloned().unwrap_or(ParameterValue::NotSet))
          .map(|v| v.to_parameter_type() as u8)
          .collect()
      };
      if let Err(e) = self
        .get_parameter_types
        .send_response(id, GetParameterTypesResponse { values })
      {
        log::warn!("get_parameter_types response failed: {e}");
      }
    }
  }

  fn handle_set_parameters(&self) {
    while let Ok(Some((id, req))) = self.set_parameters.try_receive_request() {
      let results = req
        .parameter
        .iter()
        .cloned()
        .map(Parameter::from)
        .map(|Parameter { name, value }| self.set_and_notify(&name, value))
        .map(raw::SetParametersResult::from)
        .collect();
      if let Err(e) = self
        .set_parameters
        .send_response(id, SetParametersResponse { results })
      {
        log::warn!("set_parameters response failed: {e}");
      }
    }
  }

  fn handle_set_parameters_atomically(&self) {
    while let Ok(Some((id, req))) = self.set_parameters_atomically.try_receive_request() {
      // Not implemented — mirror the DDS backend and fail every entry.
      let results = req
        .parameter
        .iter()
        .map(|_| {
          raw::SetParametersResult::from(Err(
            "Setting parameters atomically is not implemented.".to_owned(),
          ))
        })
        .collect();
      if let Err(e) = self
        .set_parameters_atomically
        .send_response(id, SetParametersResponse { results })
      {
        log::warn!("set_parameters_atomically response failed: {e}");
      }
    }
  }

  fn handle_list_parameters(&self) {
    while let Ok(Some((id, req))) = self.list_parameters.try_receive_request() {
      let names = {
        let db = self.store.lock().unwrap();
        db.keys()
          .filter(|name| {
            req.prefixes.is_empty() || req.prefixes.iter().any(|p| name.starts_with(p))
          })
          .cloned()
          .collect()
      };
      let result = ListParametersResult {
        names,
        prefixes: vec![],
      };
      if let Err(e) = self
        .list_parameters
        .send_response(id, ListParametersResponse { result })
      {
        log::warn!("list_parameters response failed: {e}");
      }
    }
  }

  fn handle_describe_parameters(&self) {
    while let Ok(Some((id, req))) = self.describe_parameters.try_receive_request() {
      let values = {
        let db = self.store.lock().unwrap();
        req
          .names
          .iter()
          .map(|name| match db.get(name) {
            Some(value) => ParameterDescriptor::from_value(name, value),
            None => ParameterDescriptor::unknown(name),
          })
          .map(raw::ParameterDescriptor::from)
          .collect()
      };
      if let Err(e) = self
        .describe_parameters
        .send_response(id, DescribeParametersResponse { values })
      {
        log::warn!("describe_parameters response failed: {e}");
      }
    }
  }
}

/// A client for a remote node's parameter services.
///
/// Create one with
/// [`Node::create_parameter_client`](crate::Node::create_parameter_client),
/// pointing at the node that hosts the parameters.
pub struct ParameterClient {
  get_parameters: Client<GetParametersRequest, GetParametersResponse>,
  get_parameter_types: Client<GetParameterTypesRequest, GetParameterTypesResponse>,
  set_parameters: Client<SetParametersRequest, SetParametersResponse>,
  list_parameters: Client<ListParametersRequest, ListParametersResponse>,
  describe_parameters: Client<DescribeParametersRequest, DescribeParametersResponse>,
}

impl ParameterClient {
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn new(
    get_parameters: Client<GetParametersRequest, GetParametersResponse>,
    get_parameter_types: Client<GetParameterTypesRequest, GetParameterTypesResponse>,
    set_parameters: Client<SetParametersRequest, SetParametersResponse>,
    list_parameters: Client<ListParametersRequest, ListParametersResponse>,
    describe_parameters: Client<DescribeParametersRequest, DescribeParametersResponse>,
  ) -> Self {
    Self {
      get_parameters,
      get_parameter_types,
      set_parameters,
      list_parameters,
      describe_parameters,
    }
  }

  /// Get the values of the named parameters (blocking). Unset parameters come
  /// back as [`ParameterValue::NotSet`].
  pub fn get_parameters(&self, names: &[&str]) -> Result<Vec<ParameterValue>, ServiceError> {
    let req = GetParametersRequest {
      names: names.iter().map(|s| s.to_string()).collect(),
    };
    let resp = self.get_parameters.call(req)?;
    Ok(resp.values.into_iter().map(ParameterValue::from).collect())
  }

  /// Get the [`ParameterType`](crate::parameters::ParameterType) codes of the
  /// named parameters (blocking).
  pub fn get_parameter_types(&self, names: &[&str]) -> Result<Vec<u8>, ServiceError> {
    let req = GetParameterTypesRequest {
      names: names.iter().map(|s| s.to_string()).collect(),
    };
    Ok(self.get_parameter_types.call(req)?.values)
  }

  /// Set parameters (blocking). Returns a per-parameter success/reason result.
  pub fn set_parameters(
    &self,
    parameters: impl IntoIterator<Item = Parameter>,
  ) -> Result<Vec<SetParametersResult>, ServiceError> {
    let req = SetParametersRequest {
      parameter: parameters.into_iter().map(raw::Parameter::from).collect(),
    };
    let resp = self.set_parameters.call(req)?;
    Ok(
      resp
        .results
        .into_iter()
        .map(|r| if r.successful { Ok(()) } else { Err(r.reason) })
        .collect(),
    )
  }

  /// List parameter names, optionally filtered by prefix (blocking).
  pub fn list_parameters(&self, prefixes: &[&str]) -> Result<Vec<String>, ServiceError> {
    let req = ListParametersRequest {
      prefixes: prefixes.iter().map(|s| s.to_string()).collect(),
      depth: 0,
    };
    Ok(self.list_parameters.call(req)?.result.names)
  }

  /// Describe the named parameters (blocking).
  pub fn describe_parameters(
    &self,
    names: &[&str],
  ) -> Result<Vec<raw::ParameterDescriptor>, ServiceError> {
    let req = DescribeParametersRequest {
      names: names.iter().map(|s| s.to_string()).collect(),
    };
    Ok(self.describe_parameters.call(req)?.values)
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

  use zenoh::Config;

  use crate::{
    parameters::ParameterType, Context, ContextOptions, Name, NodeName, NodeOptions, Parameter,
    ParameterValue,
  };

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
  fn parameter_get_set_list_roundtrip() {
    let srv_port = 17523;
    let cli_port = 17524;
    let srv_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(srv_port, None)))
        .unwrap();
    let cli_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(cli_port, Some(srv_port))),
    )
    .unwrap();

    let srv_node_name = NodeName::new("/", "param_holder").unwrap();
    let srv_node = srv_ctx.new_node(srv_node_name.clone(), NodeOptions::new());
    let cli_node = cli_ctx.new_node(
      NodeName::new("/", "param_client").unwrap(),
      NodeOptions::new(),
    );

    let server = srv_node
      .create_parameter_server([Parameter {
        name: "speed".to_string(),
        value: ParameterValue::Double(1.0),
      }])
      .unwrap();
    let client = cli_node.create_parameter_client(&srv_node_name).unwrap();

    // Server: answer requests in a background thread.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = stop.clone();
    let server_thread = std::thread::spawn(move || {
      let deadline = Instant::now() + Duration::from_secs(30);
      while !stop_srv.load(Ordering::Relaxed) && Instant::now() < deadline {
        server.spin_once();
        std::thread::sleep(Duration::from_millis(20));
      }
      // Report the final state back to the test.
      server.get_parameter("speed")
    });

    // Client: retry until the peers connect and the queryables are discovered.
    let deadline = Instant::now() + Duration::from_secs(30);

    // get the initial value
    let mut initial = None;
    while Instant::now() < deadline {
      if let Ok(vals) = client.get_parameters(&["speed"]) {
        initial = Some(vals);
        break;
      }
      std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(initial.as_deref(), Some(&[ParameterValue::Double(1.0)][..]));

    // set a new value + a brand-new parameter
    let set_results = client
      .set_parameters([
        Parameter {
          name: "speed".to_string(),
          value: ParameterValue::Double(2.5),
        },
        Parameter {
          name: "name".to_string(),
          value: ParameterValue::String("robby".to_string()),
        },
      ])
      .expect("set_parameters call");
    assert_eq!(set_results.len(), 2);
    assert!(set_results.iter().all(|r| r.is_ok()));

    // read back the changed value
    let got = client.get_parameters(&["speed"]).expect("get after set");
    assert_eq!(got, vec![ParameterValue::Double(2.5)]);

    // types
    let types = client
      .get_parameter_types(&["speed", "name"])
      .expect("get_parameter_types");
    assert_eq!(
      types,
      vec![ParameterType::Double as u8, ParameterType::String as u8]
    );

    // list should include the built-in use_sim_time plus our params
    let names = client.list_parameters(&[]).expect("list_parameters");
    assert!(names.contains(&"speed".to_string()));
    assert!(names.contains(&"name".to_string()));
    assert!(names.contains(&"use_sim_time".to_string()));

    // prefix filter
    let filtered = client.list_parameters(&["spe"]).expect("list w/ prefix");
    assert_eq!(filtered, vec!["speed".to_string()]);

    // an unset parameter comes back as NotSet
    let missing = client.get_parameters(&["nope"]).expect("get missing");
    assert_eq!(missing, vec![ParameterValue::NotSet]);

    stop.store(true, Ordering::Relaxed);
    let final_speed = server_thread.join().unwrap();
    assert_eq!(final_speed, Some(ParameterValue::Double(2.5)));

    // sanity: the client can reach the server's namespaced services
    let _ = Name::new("/param_holder", "get_parameters").unwrap();
  }
}

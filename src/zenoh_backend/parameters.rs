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
//! Events use shared [`raw::ParameterEvent`] (`stamp:
//! builtin_interfaces::Time`).
//!
//! Parameter values and the rules for changing them (declared names, fixed
//! types, `use_sim_time`, and the [`NodeOptions`](crate::NodeOptions)
//! validator and set action) are the same as on the DDS backend.
//!
//! Like the DDS backend's parameter machinery (which runs inside a `Spinner`),
//! the server here is driven by the application: call [`ParameterServer::spin`]
//! (or [`ParameterServer::spin_once`]) from your event loop to answer pending
//! requests.

use std::sync::Arc;

pub use raw::ParameterEvent;

use super::{
  pubsub::Publisher,
  service::{Client, Server},
};
use crate::{
  builtin_interfaces::Time,
  error::ServiceError,
  parameters::{
    Parameter, ParameterChanges, ParameterStore, ParameterValue, SetParametersResult, raw,
  },
  rcl_interfaces::{
    DescribeParametersRequest, DescribeParametersResponse, GetParameterTypesRequest,
    GetParameterTypesResponse, GetParametersRequest, GetParametersResponse, ListParametersRequest,
    ListParametersResponse, ListParametersResult, SetParametersAtomicallyResponse,
    SetParametersRequest, SetParametersResponse,
  },
};

/// A ROS 2 parameter server over Zenoh: a parameter store plus the six
/// `rcl_interfaces` services and the `parameter_events` publisher.
///
/// Create one with
/// [`Node::create_parameter_server`](crate::Node::create_parameter_server),
/// then call [`spin`](Self::spin) (or repeatedly
/// [`spin_once`](Self::spin_once)) to service incoming requests.
pub struct ParameterServer {
  node_fqn: String,
  store: Arc<ParameterStore>,
  get_parameters: Server<GetParametersRequest, GetParametersResponse>,
  get_parameter_types: Server<GetParameterTypesRequest, GetParameterTypesResponse>,
  set_parameters: Server<SetParametersRequest, SetParametersResponse>,
  set_parameters_atomically: Server<SetParametersRequest, SetParametersAtomicallyResponse>,
  list_parameters: Server<ListParametersRequest, ListParametersResponse>,
  describe_parameters: Server<DescribeParametersRequest, DescribeParametersResponse>,
  events: Publisher<ParameterEvent>,
}

impl ParameterServer {
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn new(
    node_fqn: String,
    store: Arc<ParameterStore>,
    get_parameters: Server<GetParametersRequest, GetParametersResponse>,
    get_parameter_types: Server<GetParameterTypesRequest, GetParameterTypesResponse>,
    set_parameters: Server<SetParametersRequest, SetParametersResponse>,
    set_parameters_atomically: Server<SetParametersRequest, SetParametersAtomicallyResponse>,
    list_parameters: Server<ListParametersRequest, ListParametersResponse>,
    describe_parameters: Server<DescribeParametersRequest, DescribeParametersResponse>,
    events: Publisher<ParameterEvent>,
  ) -> Self {
    Self {
      node_fqn,
      store,
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
    self.store.get(name)
  }

  /// Set a parameter locally and publish a `parameter_events` notification.
  /// The same rules apply as for remote set requests; setting `NotSet`
  /// deletes the parameter.
  pub fn set_parameter(&self, name: &str, value: ParameterValue) -> Result<(), String> {
    self.set_and_notify(name, value)
  }

  /// The names of all currently-set parameters.
  pub fn parameter_names(&self) -> Vec<String> {
    self.store.names()
  }

  /// Does the parameter exist?
  pub fn has_parameter(&self, name: &str) -> bool {
    self.store.has(name)
  }

  /// Remove a parameter, publishing a `parameter_events` notification if it
  /// existed.
  pub fn undeclare_parameter(&self, name: &str) {
    let changes = self.store.undeclare(name);
    self.publish_changes(changes);
  }

  /// Whether parameters not declared up front may be set.
  pub fn allow_undeclared_parameters(&self) -> bool {
    self.store.allow_undeclared()
  }

  fn set_and_notify(&self, name: &str, value: ParameterValue) -> SetParametersResult {
    let changes = self.store.set(name, value)?;
    self.publish_changes(changes);
    Ok(())
  }

  fn publish_changes(&self, changes: ParameterChanges) {
    if changes.is_empty() {
      return;
    }
    let event = changes.into_event(Time::now(), self.node_fqn.clone());
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
      let values = self
        .store
        .get_many(&req.names)
        .into_iter()
        .map(raw::ParameterValue::from)
        .collect();
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
      let values = self.store.get_types(&req.names);
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
        .into_iter()
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
      let params = req.parameter.into_iter().map(Parameter::from).collect();
      let result = self
        .store
        .set_atomically(params)
        .map(|changes| self.publish_changes(changes));
      if let Err(e) = self.set_parameters_atomically.send_response(
        id,
        SetParametersAtomicallyResponse {
          result: result.into(),
        },
      ) {
        log::warn!("set_parameters_atomically response failed: {e}");
      }
    }
  }

  fn handle_list_parameters(&self) {
    while let Ok(Some((id, req))) = self.list_parameters.try_receive_request() {
      let result = self.store.list(&req.prefixes, req.depth);
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
      let values = self
        .store
        .describe(&req.names)
        .into_iter()
        .map(raw::ParameterDescriptor::from)
        .collect();
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
  set_parameters_atomically: Client<SetParametersRequest, SetParametersAtomicallyResponse>,
  list_parameters: Client<ListParametersRequest, ListParametersResponse>,
  describe_parameters: Client<DescribeParametersRequest, DescribeParametersResponse>,
}

impl ParameterClient {
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn new(
    get_parameters: Client<GetParametersRequest, GetParametersResponse>,
    get_parameter_types: Client<GetParameterTypesRequest, GetParameterTypesResponse>,
    set_parameters: Client<SetParametersRequest, SetParametersResponse>,
    set_parameters_atomically: Client<SetParametersRequest, SetParametersAtomicallyResponse>,
    list_parameters: Client<ListParametersRequest, ListParametersResponse>,
    describe_parameters: Client<DescribeParametersRequest, DescribeParametersResponse>,
  ) -> Self {
    Self {
      get_parameters,
      get_parameter_types,
      set_parameters,
      set_parameters_atomically,
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

  /// Set parameters all-or-nothing (blocking): if any parameter is rejected,
  /// none are changed.
  pub fn set_parameters_atomically(
    &self,
    parameters: impl IntoIterator<Item = Parameter>,
  ) -> Result<SetParametersResult, ServiceError> {
    let req = SetParametersRequest {
      parameter: parameters.into_iter().map(raw::Parameter::from).collect(),
    };
    let r = self.set_parameters_atomically.call(req)?.result;
    Ok(if r.successful { Ok(()) } else { Err(r.reason) })
  }

  /// List parameter names, optionally filtered by prefix (blocking).
  ///
  /// A prefix matches a parameter of that name and parameters under that
  /// namespace (`prefix.`).
  pub fn list_parameters(&self, prefixes: &[&str]) -> Result<Vec<String>, ServiceError> {
    Ok(self.list_parameters_with_depth(prefixes, 0)?.names)
  }

  /// List parameter names and namespace prefixes (blocking). `depth` limits
  /// how many `.`-separated levels below the prefix are included; 0 means
  /// unlimited.
  pub fn list_parameters_with_depth(
    &self,
    prefixes: &[&str],
    depth: u64,
  ) -> Result<ListParametersResult, ServiceError> {
    let req = ListParametersRequest {
      prefixes: prefixes.iter().map(|s| s.to_string()).collect(),
      depth,
    };
    Ok(self.list_parameters.call(req)?.result)
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
      Arc,
      atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
  };

  use zenoh::Config;

  use crate::{
    Name, NodeName, NodeOptions, Parameter, ParameterValue,
    parameters::ParameterType,
    zenoh_backend::context::{Context, ContextOptions},
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
    let srv_node = srv_ctx
      .new_node(srv_node_name.clone(), NodeOptions::new())
      .unwrap();
    let cli_node = cli_ctx
      .new_node(
        NodeName::new("/", "param_client").unwrap(),
        NodeOptions::new(),
      )
      .unwrap();

    let server = srv_node
      .create_parameter_server([
        Parameter {
          name: "speed".to_string(),
          value: ParameterValue::Double(1.0),
        },
        Parameter {
          name: "name".to_string(),
          value: ParameterValue::String("".to_string()),
        },
      ])
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

    // set new values for both declared parameters
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

    // prefix filter: a prefix matches whole names or namespaces only
    let filtered = client.list_parameters(&["speed"]).expect("list w/ prefix");
    assert_eq!(filtered, vec!["speed".to_string()]);
    let partial = client.list_parameters(&["spe"]).expect("list w/ partial");
    assert!(partial.is_empty());

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

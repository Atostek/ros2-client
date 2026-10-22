//! Zenoh parameter services over a loopback peer connection: the rules from
//! `NodeOptions` apply to remote set requests, atomic sets are all-or-nothing,
//! and listing reports namespace prefixes.
#![cfg(feature = "zenoh")]

use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::{Duration, Instant},
};

use ros2_client::{
  NodeName, NodeOptions, Parameter, ParameterValue,
  zenoh::{Context, ContextOptions},
};

fn make_config(listen_port: u16, connect_port: Option<u16>) -> zenoh::Config {
  let mut c = zenoh::Config::default();
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

fn param(name: &str, value: ParameterValue) -> Parameter {
  Parameter {
    name: name.to_string(),
    value,
  }
}

#[test]
fn parameter_rules_atomic_set_and_prefixes() {
  let srv_port = 17531;
  let cli_port = 17532;
  let srv_ctx =
    Context::with_options(ContextOptions::new().zenoh_config(make_config(srv_port, None))).unwrap();
  let cli_ctx = Context::with_options(
    ContextOptions::new().zenoh_config(make_config(cli_port, Some(srv_port))),
  )
  .unwrap();

  let srv_node_name = NodeName::new("/", "param_rules_holder").unwrap();
  let srv_node = srv_ctx
    .new_node(
      srv_node_name.clone(),
      NodeOptions::new()
        .declare_parameter("rate", ParameterValue::Integer(10))
        .declare_parameter("gain.p", ParameterValue::Double(1.0))
        .declare_parameter("gain.limits.max", ParameterValue::Double(5.0))
        .parameter_validator(Box::new(|name, value| match (name, value) {
          ("rate", ParameterValue::Integer(r)) if *r <= 0 => Err("rate must be positive".into()),
          _ => Ok(()),
        })),
    )
    .unwrap();
  let cli_node = cli_ctx
    .new_node(
      NodeName::new("/", "param_rules_client").unwrap(),
      NodeOptions::new(),
    )
    .unwrap();
  let client = cli_node.create_parameter_client(&srv_node_name).unwrap();

  let stop = Arc::new(AtomicBool::new(false));
  let stop_srv = stop.clone();
  let server_thread = std::thread::spawn(move || {
    let server = srv_node.parameter_server().expect("parameter services");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !stop_srv.load(Ordering::Relaxed) && Instant::now() < deadline {
      server.spin_once();
      std::thread::sleep(Duration::from_millis(20));
    }
  });

  // Retry until the peers connect and the queryables are discovered.
  let deadline = Instant::now() + Duration::from_secs(30);
  let mut initial = None;
  while Instant::now() < deadline {
    if let Ok(vals) = client.get_parameters(&["rate"]) {
      initial = Some(vals);
      break;
    }
    std::thread::sleep(Duration::from_millis(200));
  }
  assert_eq!(initial, Some(vec![ParameterValue::Integer(10)]));

  // An atomic set with one invalid entry applies nothing.
  let result = client
    .set_parameters_atomically([
      param("rate", ParameterValue::Integer(20)),
      param("gain.p", ParameterValue::String("high".to_string())),
    ])
    .expect("set_parameters_atomically call");
  assert!(result.is_err());
  assert_eq!(
    client.get_parameters(&["rate", "gain.p"]).unwrap(),
    vec![ParameterValue::Integer(10), ParameterValue::Double(1.0)]
  );

  // A valid atomic set applies everything.
  let result = client
    .set_parameters_atomically([
      param("rate", ParameterValue::Integer(20)),
      param("gain.p", ParameterValue::Double(2.0)),
    ])
    .expect("set_parameters_atomically call");
  assert_eq!(result, Ok(()));
  assert_eq!(
    client.get_parameters(&["rate", "gain.p"]).unwrap(),
    vec![ParameterValue::Integer(20), ParameterValue::Double(2.0)]
  );

  // Type changes, validator rejections and undeclared names are refused.
  let results = client
    .set_parameters([
      param("rate", ParameterValue::Double(1.5)),
      param("rate", ParameterValue::Integer(-1)),
      param("undeclared", ParameterValue::Integer(1)),
    ])
    .expect("set_parameters call");
  assert_eq!(results.len(), 3);
  assert!(results.iter().all(|r| r.is_err()));
  assert_eq!(
    client.get_parameters(&["rate", "undeclared"]).unwrap(),
    vec![ParameterValue::Integer(20), ParameterValue::NotSet]
  );

  // Listing reports the namespaces of the listed names.
  let listed = client
    .list_parameters_with_depth(&[], 0)
    .expect("list_parameters call");
  for name in ["rate", "gain.p", "gain.limits.max", "use_sim_time"] {
    assert!(listed.names.contains(&name.to_string()), "{name} missing");
  }
  assert!(listed.prefixes.contains(&"gain".to_string()));
  assert!(listed.prefixes.contains(&"gain.limits".to_string()));

  // As in rclcpp, the separator after the prefix counts towards the depth.
  let listed = client
    .list_parameters_with_depth(&["gain"], 2)
    .expect("list_parameters call");
  assert_eq!(listed.names, vec!["gain.p".to_string()]);
  assert_eq!(listed.prefixes, vec!["gain".to_string()]);

  stop.store(true, Ordering::Relaxed);
  server_thread.join().unwrap();
}

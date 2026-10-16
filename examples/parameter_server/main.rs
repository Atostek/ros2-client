//! DDS node that declares a few parameters and serves them to `ros2 param`.
//!
//! ```text
//! cargo run --example parameter_server
//! cargo run --example parameter_server -- --no-parameter-services
//! ```
//!
//! `--no-parameter-services` keeps the local parameter store (so this process
//! still prints values) but does not offer the six `rcl_interfaces` services.

use std::time::Duration;

use ros2_client::{Context, Node, NodeName, NodeOptions, ParameterValue};

fn main() {
  pretty_env_logger::init();

  let services = !std::env::args().any(|arg| arg == "--no-parameter-services");
  let mut node = create_node(services);
  println!(
    ">>> node {} parameter services {}",
    node.fully_qualified_name(),
    if services { "on" } else { "off" }
  );

  smol::spawn(node.spinner().unwrap().spin()).detach();

  smol::block_on(async {
    loop {
      print_parameters(&node);
      smol::Timer::after(Duration::from_secs(5)).await;
    }
  });
}

fn create_node(start_parameter_services: bool) -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "parameter_server").unwrap(),
      NodeOptions::new()
        .enable_rosout(true)
        .start_parameter_services(start_parameter_services)
        .declare_parameter("enabled", ParameterValue::Boolean(true))
        .declare_parameter("rate_hz", ParameterValue::Integer(1))
        .declare_parameter("gain", ParameterValue::Double(1.0))
        .declare_parameter("label", ParameterValue::String("demo".into()))
        .declare_parameter(
          "tags",
          ParameterValue::StringArray(vec!["demo".into(), "rust".into()]),
        )
        .parameter_validator(Box::new(|name, value| {
          if name == "rate_hz"
            && let ParameterValue::Integer(rate) = value
            && *rate <= 0
          {
            return Err(format!("rate_hz must be > 0, got {rate}"));
          }
          Ok(())
        }))
        .parameter_set_action(Box::new(|name, value| {
          println!("{name} = {value:?}");
          Ok(())
        })),
    )
    .unwrap()
}

fn print_parameters(node: &Node) {
  for name in ["enabled", "rate_hz", "gain", "label", "tags"] {
    println!(">>> {name} = {:?}", node.get_parameter(name));
  }
}

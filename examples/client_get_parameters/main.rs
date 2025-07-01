use std::{env, time::Duration};

use futures::TryFutureExt;
use smol::future::FutureExt;
use ros2_client::{
  rcl_interfaces::{GetParametersRequest, GetParametersResponse},
  ros2::WriteError,
  service::CallServiceError,
  AService, Context, Name, Node, NodeName, NodeOptions, ParameterValue, ServiceMapping,
  ServiceTypeName,
};
use rustdds::{policy, QosPolicies, QosPolicyBuilder};

fn main() {
  log4rs::init_file("log4rs.yaml", Default::default()).unwrap();

  let args: Vec<String> = env::args().collect();
  if args.len() < 2 {
    println!("There is no args");
    println!(
      "For example: ./client_get_parameters /turtlesim background_r background_g background_b"
    );
    return;
  }

  println!(">>> ros2_service starting...");
  let mut node = create_node();
  let service_qos = create_qos();

  // Start background spinner.
  // E.g. waiting for server does not work without this.
  smol::spawn(node.spinner().unwrap().spin()).detach();

  println!(">>> ros2_service node started");

  let target_node = &args[1];
  println!(">>> target node is '{target_node}'");
  let service_name = Name::new(target_node, "get_parameters").unwrap();
  println!(">>> connecting service {service_name:?}");

  let client = node
    .create_client::<AService<GetParametersRequest, GetParametersResponse>>(
      ServiceMapping::Enhanced,
      &service_name,
      &ServiceTypeName::new("rcl_interfaces", "GetParameters"),
      service_qos.clone(),
      service_qos,
    )
    .unwrap();

  println!(">>> ros2_service client created");
  let request = GetParametersRequest {
    names: args[2..].to_vec(),
  };
  println!(">>> request = {request:?}");

  smol::block_on(async {
    println!(">>> Waiting for GetParameters server to appear.");
    client.wait_for_service(&node).await;
    println!(">>> Connected to GetParameters server.");

    match client.async_send_request(request).await {
      Ok(req_id) => {
        println!(">>> request sent {req_id:?}");
        let response_or_timeout = client
          .async_receive_response(req_id)
          .map_err(CallServiceError::<()>::from)
          .or(async {
            smol::Timer::after(Duration::from_secs(10)).await;
            println!(">>> Response timeout!!");
            Err(WriteError::WouldBlock { data: () }.into())
          });
        match response_or_timeout.await {
          Ok(response) => {
            println!(
              "<<< response parameters: {:?}",
              response
                .values
                .iter()
                .cloned()
                .map(ParameterValue::from)
                .collect::<Vec<ParameterValue>>()
            );
          }
          Err(e) => println!("<<< response error {e:?}"),
        }
      }
      Err(e) => println!(">>> request sending error {e:?}"),
    }
  });
}

fn create_qos() -> QosPolicies {
  let service_qos: QosPolicies = {
    QosPolicyBuilder::new()
      .reliability(policy::Reliability::Reliable {
        max_blocking_time: rustdds::Duration::from_millis(100),
      })
      .history(policy::History::KeepLast { depth: 1 })
      .build()
  };
  service_qos
}

fn create_node() -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "get_parameters_client").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

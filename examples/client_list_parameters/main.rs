use std::{env, time::Duration};

use futures::TryFutureExt;
use smol::future::FutureExt;
use ros2_client::{
  rcl_interfaces::{ListParametersRequest, ListParametersResponse},
  ros2::WriteError,
  service::CallServiceError,
  AService, Context, Name, Node, NodeName, NodeOptions, ServiceMapping, ServiceTypeName,
};
use rustdds::{policy, QosPolicies, QosPolicyBuilder};

fn main() {
  log4rs::init_file("log4rs.yaml", Default::default()).unwrap();

  let args: Vec<String> = env::args().collect();
  if args.len() < 2 {
    println!("There is no args. Please provide the name of the target node to query.");
    println!("For example: ./client_list_parameters /turtlesim");
    return;
  }

  let mut node = create_node();

  // Start background spinner.
  // E.g. waiting for server does not work without this.
  smol::spawn(node.spinner().unwrap().spin()).detach();

  let service_qos = create_qos();

  println!(">>> ros2_service node started");

  let target_node = &args[1];
  println!(">>> target node is '{target_node}'");
  let service_name = Name::new(target_node, "list_parameters").unwrap();
  println!(">>> connecting service {service_name:?}");

  let client = node
    .create_client::<AService<ListParametersRequest, ListParametersResponse>>(
      ServiceMapping::Enhanced,
      &service_name,
      &ServiceTypeName::new("rcl_interfaces", "ListParameters"),
      service_qos.clone(),
      service_qos,
    )
    .unwrap();

  let request = ListParametersRequest {
    depth: 0,
    prefixes: vec![],
  };

  smol::block_on(async {
    println!(">>> Waiting for ListParameters server to appear.");
    client.wait_for_service(&node).await;
    println!(">>> Connected to ListParameters server.");

    match client.async_send_request(request).await {
      Ok(req_id) => {
        println!(">>> request sent {req_id:?}");
        let response_or_timeout = client
          .async_receive_response(req_id)
          .map_err(CallServiceError::<()>::from)
          .or(async {
            smol::Timer::after(Duration::from_secs(15)).await;
            println!(">>> Response timeout!!");
            Err(WriteError::WouldBlock { data: () }.into())
          });
        match response_or_timeout.await {
          Ok(response) => {
            println!("<<< response: {response:?}");
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
      NodeName::new("/rustdds", "parameter_client").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

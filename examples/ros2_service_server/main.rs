use log::error;
use mio::{Events, Poll, PollOpt, Ready, Token};
use serde::{Deserialize, Serialize};
use ros2_client::{
  AService, Context, Message, Name, Node, NodeName, NodeOptions, ServiceMapping, ServiceTypeName,
};
use rustdds::{
  policy::{self, Deadline, Lifespan},
  Duration, QosPolicies, QosPolicyBuilder,
};

// This is an example / test program.
// Test this against minimal_client found in
// https://github.com/ros2/examples/blob/master/rclpy/services/minimal_client/examples_rclpy_minimal_client/client.py
// or
// % ros2 run examples_rclpy_minimal_client client
// or
// % ros2 run examples_rclcpp_minimal_client client_main

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTwoIntsRequest {
  pub a: i64,
  pub b: i64,
}
impl Message for AddTwoIntsRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTwoIntsResponse {
  pub sum: i64,
}
impl Message for AddTwoIntsResponse {}

fn main() {
  pretty_env_logger::init();

  println!(">>> ros2_service starting...");
  let mut node = create_node();
  let service_qos = create_qos();

  println!(">>> ros2_service node started");

  let server = node
    .create_server::<AService<AddTwoIntsRequest, AddTwoIntsResponse>>(
      ServiceMapping::Enhanced,
      &Name::new("/", "add_two_ints").unwrap(),
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
      service_qos.clone(),
      service_qos,
    )
    .unwrap();

  println!(">>> ros2_service server created");

  let poll = Poll::new().unwrap();

  poll
    .register(&server, Token(1), Ready::readable(), PollOpt::edge())
    .unwrap();

  loop {
    println!(">>> event loop iter");
    let mut events = Events::with_capacity(100);
    poll.poll(&mut events, None).unwrap();

    for event in events.iter() {
      match event.token() {
        Token(1) => match server.receive_request() {
          Ok(req_option) => match req_option {
            Some((id, request)) => {
              println!(">>> Request received - id: {id:?}, request: {request:?}");
              let sum = request.a + request.b;
              let response = AddTwoIntsResponse { sum };
              match server.send_response(id, response.clone()) {
                Ok(_) => println!(">>> Server sent response: {response:?} id: {id:?}",),

                Err(e) => error!(">>> Server response error: {e:?}"),
              }
            }
            None => {
              println!(">>> No request available.")
            }
          },
          Err(e) => {
            println!(">>> error with response handling, e: {e:?}")
          }
        },
        _ => println!(">>> Unknown poll token {:?}", event.token()),
      } // match
    } // for
  } // loop
} // main

fn create_qos() -> QosPolicies {
  let service_qos: QosPolicies = {
    QosPolicyBuilder::new()
      .history(policy::History::KeepLast { depth: 10 })
      .reliability(policy::Reliability::Reliable {
        max_blocking_time: Duration::from_millis(100),
      })
      .durability(policy::Durability::Volatile)
      .deadline(Deadline(Duration::INFINITE))
      .lifespan(Lifespan {
        duration: Duration::INFINITE,
      })
      .liveliness(policy::Liveliness::Automatic {
        lease_duration: Duration::INFINITE,
      })
      .build()
  };
  service_qos
}

fn create_node() -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "rustdds_server").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

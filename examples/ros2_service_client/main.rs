use std::time::{Duration, Instant};

use rustdds::mio::{Events, Poll, PollOpt, Ready, Token};
use serde::{Deserialize, Serialize};
use ros2_client::{
  qos::History, Context, Message, Name, Node, NodeName, NodeOptions, QosProfile, ServiceMapping,
  ServiceTypeName,
};

const RESPONSE_TOKEN: Token = Token(7); // Just an arbitrary value

// Test / demo program of ROS2 services, client side.
//
// To set up a server from ROS2:
// % ros2 run examples_rclcpp_minimal_service service_main
// or
// % ros2 run examples_rclpy_minimal_service service
//
// Then run this example.

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

  let service_mapping = service_mapping_from_env();
  println!(">>> using ServiceMapping::{service_mapping:?}");

  let client = node
    .create_client::<AddTwoIntsRequest, AddTwoIntsResponse>(
      service_mapping,
      &Name::new("/", "add_two_ints").unwrap(),
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
      service_qos.clone(),
      service_qos,
    )
    .unwrap();

  println!(">>> ros2_service client created");

  let poll = Poll::new().unwrap();

  poll
    .register(&client, RESPONSE_TOKEN, Ready::readable(), PollOpt::edge())
    .unwrap();

  let mut request_generator = 0;
  let mut request_sent_at = Instant::now(); // request rate limiter

  loop {
    let mut events = Events::with_capacity(100);
    poll
      .poll(&mut events, Some(Duration::from_secs(1)))
      .unwrap();

    for event in events.iter() {
      match event.token() {
        RESPONSE_TOKEN => {
          while let Ok(Some((id, response))) = client.receive_response() {
            println!(">>> Response received: response: {response:?} - response id: {id:?}, ",);
          }
        }
        _ => println!(">>> Unknown poll token {:?}", event.token()),
      }
    }

    let now = Instant::now(); // rate limit
    if now.duration_since(request_sent_at) > Duration::from_secs(2) {
      request_sent_at = now;
      println!(">>> request sending...");
      request_generator += 3;
      let a = request_generator % 5;
      let b = request_generator % 7;
      match client.send_request(AddTwoIntsRequest { a, b }) {
        Ok(id) => {
          println!(">>> request sent a={a} b={b}, {id:?}");
        }
        Err(e) => {
          println!(">>> request sending error {e:?}");
        }
      }
    }
  }
}

/// Select the service wire mapping from the `ROS2_SERVICE_MAPPING` environment
/// variable (`Basic`, `Enhanced`, or `Cyclone`). Defaults to `Enhanced`.
fn service_mapping_from_env() -> ServiceMapping {
  match std::env::var("ROS2_SERVICE_MAPPING").as_deref() {
    Ok("Basic") | Ok("basic") => ServiceMapping::Basic,
    Ok("Cyclone") | Ok("cyclone") => ServiceMapping::Cyclone,
    Ok("Enhanced") | Ok("enhanced") => ServiceMapping::Enhanced,
    Ok(other) => {
      eprintln!(">>> Unknown ROS2_SERVICE_MAPPING '{other}', using Enhanced");
      ServiceMapping::Enhanced
    }
    Err(_) => ServiceMapping::Enhanced,
  }
}

fn create_qos() -> QosProfile {
  QosProfile::publisher_default().history(History::KeepLast { depth: 1 })
}

fn create_node() -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "rustdds_client").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

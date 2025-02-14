use std::time::Duration;

#[allow(unused_imports)]
use log::{debug, error, info, warn};
use futures::{FutureExt as StdFutureExt, StreamExt, TryFutureExt};
use smol::{future::FutureExt, pin};
use serde::{Deserialize, Serialize};
use ros2_client::{
  service::CallServiceError, AService, Context, Message, Name, Node, NodeName, NodeOptions,
  ServiceMapping, ServiceTypeName,
};
use rustdds::{dds::WriteError, policy, QosPolicies, QosPolicyBuilder};

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

  // Set Ctrl-C handler
  let (stop_sender, stop_receiver) = smol::channel::bounded(2);
  ctrlc::set_handler(move || {
    // We will send two stop commands, one for reader, the other for writer.
    //stop_sender.send_blocking(()).unwrap_or(());
    stop_sender.send_blocking(()).unwrap_or(());
    // ignore errors, as we are quitting anyway
  })
  .expect("Error setting Ctrl-C handler");
  println!("Press Ctrl-C to quit.");

  debug!(">>> ros2_service starting...");
  let mut node = create_node();
  let service_qos = create_qos();

  debug!(">>> ros2_service node started");

  let client = node
    .create_client::<AService<AddTwoIntsRequest, AddTwoIntsResponse>>(
      ServiceMapping::Enhanced,
      &Name::new("/", "add_two_ints").unwrap(),
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
      service_qos.clone(),
      service_qos,
    )
    .unwrap();

  // Running node.spinner().spin() is similar to calling .spin() in the sense that
  // it runs background processes, such as graph event processing.
  //
  // However, in ros2-client Node does not call application back, but instead the
  // ROS application is implemented using async code.
  smol::spawn(node.spinner().unwrap().spin()).detach();

  debug!(">>> ros2_service client created");

  let mut request_generator = 0;

  let main_loop = async {
    let mut run = true;
    let stop = stop_receiver.recv().fuse();
    pin!(stop);
    let mut tick_stream = futures::StreamExt::fuse(smol::Timer::interval(Duration::from_secs(2)));

    while run {
      futures::select! {
        _ = stop => {
          run = false;
          println!("Stopping");
        }
        _tick = tick_stream.select_next_some() => {
          let service_is_ready = client.wait_for_service(&node).map(|_| true)
              .or(async {
                smol::Timer::after(Duration::from_secs(1));
                false
              }).await;
          if service_is_ready  {
            request_generator += 3;
            let a = request_generator % 5;
            let b = request_generator % 7;
            match client.async_send_request(AddTwoIntsRequest { a, b }).await {
              Ok(req_id) => {
                println!(">>> request sent a={} b={}, {:?}", a, b, req_id.sequence_number);
                let response = client.async_receive_response(req_id).map_err(CallServiceError::<()>::from)
                      .or(async {
                            smol::Timer::after(Duration::from_secs(2)).await;
                            Err(WriteError::WouldBlock { data: () }.into() )
                          });
                match response.await
                {
                  Ok(response) => {
                    println!("<<< response: {:?}", response);
                  }
                  Err(e) => println!("<<< response error {:?}", e),
                }
              }
              Err(e) => println!(">>> request sending error {:?}", e),
            } // match async_send_request

          } else { // service not ready
            println!(">>> waiting for Server to appear.");
          }
        }
      } // select!
    } // while
    debug!("main loop done");
  };

  // let status_event_stream = node.status_receiver().for_each(|event| async move
  // {   println!("{:?}", event);
  // });

  // run it!
  smol::block_on(main_loop);
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
      NodeName::new("/rustdds", "rustdds_client").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

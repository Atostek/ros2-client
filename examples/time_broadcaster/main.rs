use std::{convert::TryInto, time::Duration};

use chrono::{DateTime, Utc};
use ros2_client::*;

pub fn main() {
  log4rs::init_file("log4rs.yaml", Default::default()).unwrap();

  let context = Context::new().unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/", "time_broadcaster").unwrap(),
      NodeOptions::new()
        .enable_rosout(true)
        .declare_parameter("my_param", ParameterValue::String("foo".to_owned()))
        // example parameter validator function
        // Requirement to have exactly 3 chars is just an arbitrary restriction.
        .parameter_validator(Box::new(|name, value| match name {
          "my_param" => match value {
            ParameterValue::String(s) if s.len() == 3 => Ok(()),
            _ => Err("my_param must be a string of 3 chars".to_string()),
          },
          _ => Ok(()),
        }))
        // parameter set handler.
        .parameter_set_action(Box::new(|name, value| {
          println!("Setting {name}={value:?}");
          Ok(())
        })),
    )
    .unwrap();

  // "my_param" can be changed using
  // ros2 service call /time_broadcaster/set_parameters
  // rcl_interfaces/srv/SetParameters '{parameters: [{name: "my_param", value:
  // {type: 4, string_value: "bar"}}]}'
  //
  // read back:
  // ros2 service call /time_broadcaster/get_parameters
  // rcl_interfaces/srv/GetParameters '{names: ['my_param']}'
  //
  // or, more simply:
  //
  // ros2 param get --spin-time=5 /time_broadcaster my_param
  //
  // ros2 param set --spin-time=5 /time_broadcaster my_param bar
  //
  // ros2 param describe --spin-time=5 /time_broadcaster my_param

  let clock_publisher = node
    .create_publisher::<builtin_interfaces::Time>(
      &node
        .create_topic(
          &Name::new("/", "clock").unwrap(),
          MessageTypeName::new("builtin_interfaces", "Time"),
          &DEFAULT_PUBLISHER_QOS,
        )
        .unwrap(),
      None,
    )
    .unwrap();

  smol::spawn(node.spinner().unwrap().spin()).detach();

  // Define at which rates simlated time proceeds vs. real time.
  // Tiacks below are equal in length.
  // This also defines how often simulated clock is updated.
  let sim_time_tick = Duration::from_millis(1000);
  let real_time_tick = Duration::from_millis(2000);

  let mut sim_time = node.time_now();

  smol::block_on(async move {
    loop {
      println!("tick {:?}", DateTime::<Utc>::from(sim_time));
      clock_publisher.publish(sim_time.into()).unwrap();
      sim_time = sim_time + sim_time_tick.try_into().unwrap();
      smol::Timer::after(real_time_tick).await;
    }
  });
}

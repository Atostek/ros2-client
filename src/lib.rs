//! ROS 2 client library, similar to the [rclcpp](https://docs.ros.org/en/rolling/p/rclcpp/) or
//! [rclpy](https://docs.ros.org/en/rolling/p/rclpy/) libraries, in native Rust. The underlying DDS
//! implementation, [RustDDS](https://atostek.com/en/products/rustdds/), is also native Rust.
//!
//! # Example
//!
//! ```
//! use futures::StreamExt;
//! use ros2_client::*;
//!
//!   let context = Context::new().unwrap();
//!   let mut node = context
//!     .new_node(
//!       NodeName::new("/rustdds", "rustdds_listener").unwrap(),
//!       NodeOptions::new().enable_rosout(true),
//!     )
//!     .unwrap();
//!
//!   let chatter_topic = node
//!     .create_topic(
//!       &Name::new("/","topic").unwrap(),
//!       MessageTypeName::new("std_msgs", "String"),
//!       &ros2_client::DEFAULT_SUBSCRIPTION_QOS,
//!     )
//!     .unwrap();
//!   let chatter_subscription = node
//!     .create_subscription::<String>(&chatter_topic, None)
//!     .unwrap();
//!
//!   let subscription_stream = chatter_subscription
//!     .async_stream()
//!     .for_each(|result| async {
//!       match result {
//!         Ok((msg, _)) => println!("I heard: {msg}"),
//!         Err(e) => eprintln!("Receive request error: {:?}", e),
//!       }
//!     });
//!
//!   // Since we enabled rosout, let's log something
//!   rosout!(
//!     node,
//!     ros2::LogLevel::Info,
//!     "wow. very listening. such topics. much subscribe."
//!   );
//!
//!   // Uncomment this to execute until interrupted.
//!   // --> smol::block_on( subscription_stream );
//! ```

#[macro_use]
extern crate lazy_static;

/// Some builtin datatypes needed for ROS2 communication
/// Some convenience topic infos for ROS2 communication
pub mod builtin_topics;

#[doc(hidden)]
pub mod action_msgs; // action mechanism implementation

/// Some builtin interfaces for ROS2 communication
pub mod builtin_interfaces;

#[doc(hidden)]
pub mod context;

#[doc(hidden)] // needed for actions implementation
pub mod unique_identifier_msgs;

#[doc(hidden)]
#[deprecated] // we should remove the rest of these
pub mod interfaces;

/// ROS 2 Action machinery
pub mod action;
pub mod entities_info;
mod gid;
pub mod log;
pub mod message;
pub mod message_info;
pub mod names;
pub mod parameters;
#[doc(hidden)]
pub mod pubsub;
pub mod rcl_interfaces;
pub mod ros_time;
pub mod service;

pub mod steady_time;
mod wide_string;

#[doc(hidden)]
pub(crate) mod node;

// Re-exports from crate root to simplify usage
#[doc(inline)]
pub use context::*;
#[doc(inline)]
pub use message::Message;
#[doc(inline)]
pub use names::{ActionTypeName, MessageTypeName, Name, NodeName, ServiceTypeName};
#[doc(inline)]
pub use message_info::MessageInfo;
#[doc(inline)]
pub use node::*;
#[doc(inline)]
pub use parameters::{Parameter, ParameterValue};
#[doc(inline)]
pub use pubsub::*;
#[doc(inline)]
pub use service::{AService, Client, Server, Service, ServiceMapping};
#[doc(inline)]
pub use action::{Action, ActionTypes};
#[doc(inline)]
pub use wide_string::WString;
#[doc(inline)]
pub use ros_time::{ROSTime, SystemTime};

/// Module for stuff we do not want to export from top level;
pub mod ros2 {
  pub use rustdds::{qos::policy, Duration, QosPolicies, QosPolicyBuilder, Timestamp};
  //TODO: re-export RustDDS error types until ros2-client defines its own
  pub use rustdds::dds::{CreateError, ReadError, WaitError, WriteError};

  pub use crate::log::LogLevel;
  // TODO: What to do about SecurityError (exists based on feature "security")
  pub use crate::names::Name; // import Name as ros2::Name if there is clash
                              // otherwise
}

pub mod prelude {
  pub use crate::node::{
    logging::{NodeLoggingHandle, RosoutRaw},
    Node,
  };
}

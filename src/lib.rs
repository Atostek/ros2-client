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

// During the incremental Zenoh port, several backend-neutral helpers (name
// mangling, time, etc.) are currently only consumed by the DDS backend; they
// become live as E4–E6 wire up the Zenoh side. Allow dead_code on the `zenoh`
// build so it stays warning-clean without scattering per-item cfgs.
#![cfg_attr(not(feature = "dds"), allow(dead_code))]

// ---------------------------------------------------------------------------
// Middleware backend selection. Exactly one of `dds` (default) or `zenoh` must
// be enabled. See
// docs/decisions/0002-dual-backend-compile-time-feature-selection.md
// ---------------------------------------------------------------------------
#[cfg(all(feature = "dds", feature = "zenoh"))]
compile_error!(
  "features `dds` and `zenoh` are mutually exclusive: enable exactly one. \
   To use the Zenoh backend, build with `--no-default-features --features zenoh`."
);
#[cfg(not(any(feature = "dds", feature = "zenoh")))]
compile_error!(
  "no middleware backend selected: enable exactly one of `dds` (default) or \
   `zenoh`. You likely used `--no-default-features` without `--features dds` \
   or `--features zenoh`."
);

// lazy_static is only used by DDS-backend modules (builtin_topics, context).
#[cfg(feature = "dds")]
#[macro_use]
extern crate lazy_static;

// NOTE: modules that depend on RustDDS are gated behind the `dds` feature.
// The `zenoh` backend re-implements the corresponding public API incrementally
// (see docs/zenoh_study/refactoring_plan.md, issues E3–E9). Backend-neutral
// modules (names, message, qos, time, the wire-format spec) compile on both.

/// Some builtin datatypes needed for ROS2 communication
/// Some convenience topic infos for ROS2 communication
#[cfg(feature = "dds")]
pub mod builtin_topics;

#[doc(hidden)]
pub mod action_msgs; // action mechanism implementation

/// Some builtin interfaces for ROS2 communication
pub mod builtin_interfaces;

#[doc(hidden)]
#[cfg(feature = "dds")]
pub mod context;

#[doc(hidden)] // needed for actions implementation
pub mod unique_identifier_msgs;

#[doc(hidden)]
#[deprecated] // we should remove the rest of these
#[cfg(feature = "dds")]
pub mod interfaces;

/// ROS 2 Action machinery
#[cfg(feature = "dds")]
pub mod action;
/// ROS 2 distribution identification (compile-time selection + runtime check)
pub mod distributions;
#[cfg(feature = "dds")]
pub mod entities_info;
#[cfg(feature = "dds")]
mod gid;
pub mod log;
pub mod message;
#[cfg(feature = "dds")]
pub mod message_info;
pub mod names;
/// Rust-like representation of ROS 2 Parameters (backend-neutral).
pub mod parameters;
#[doc(hidden)]
#[cfg(feature = "dds")]
pub mod pubsub;
/// Backend-neutral Quality-of-Service profile.
pub mod qos;
/// `rcl_interfaces` message/service payload types (backend-neutral).
pub mod rcl_interfaces;
pub mod ros_time;
#[cfg(feature = "dds")]
pub mod rosout;
#[cfg(feature = "dds")]
pub mod service;

pub mod steady_time;
mod wide_string;

#[doc(hidden)]
#[cfg(feature = "dds")]
pub(crate) mod node;

/// Zenoh middleware backend (cargo feature `zenoh`).
///
/// The module is compiled unconditionally so its backend-neutral "wire-format
/// spec" submodules (key expressions, type hashes, GID) can be unit-tested on
/// any build. Submodules that depend on the `zenoh` crate are gated behind
/// `#[cfg(feature = "zenoh")]` inside the module.
pub(crate) mod zenoh_backend;

// Re-exports from crate root to simplify usage
#[cfg(feature = "dds")]
#[doc(inline)]
pub use context::*;
#[doc(inline)]
pub use distributions::{RosDistro, COMPILED_ROS_DISTRO};
#[doc(inline)]
pub use message::Message;
#[doc(inline)]
pub use names::{ActionTypeName, MessageTypeName, Name, NodeName, ServiceTypeName};
#[cfg(feature = "dds")]
#[doc(inline)]
pub use message_info::MessageInfo;
#[cfg(feature = "dds")]
#[doc(inline)]
pub use node::*;
#[doc(inline)]
pub use parameters::{Parameter, ParameterValue};
#[doc(inline)]
pub use qos::QosProfile;
#[cfg(feature = "dds")]
#[doc(inline)]
pub use pubsub::*;
#[cfg(feature = "dds")]
#[doc(inline)]
pub use service::{AService, Client, Server, Service, ServiceMapping};
#[cfg(feature = "dds")]
#[doc(inline)]
pub use action::{Action, ActionTypes};
#[doc(inline)]
pub use wide_string::WString;
#[doc(inline)]
pub use ros_time::{ROSTime, SystemTime};
#[cfg(feature = "dds")]
#[doc(inline)]
pub use rosout::{NodeLoggingHandle, RosoutRaw};
// Zenoh backend public API (incremental; see E3–E9).
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::context::{Context, ContextOptions};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::node::{Node, NodeOptions, Topic};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::pubsub::{MessageInfo, Publisher, Subscription};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::service::{Client, RmwRequestId, Server};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::action::{ActionClient, ActionServer, GoalId};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::parameters::{ParameterClient, ParameterEvent, ParameterServer};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::rosout::{Log, Logger};
#[cfg(feature = "zenoh")]
#[doc(inline)]
pub use zenoh_backend::{
  graph_cache::{GraphEntity, GraphEvent},
  keyexpr::EntityKind,
};

/// Module for stuff we do not want to export from top level;
pub mod ros2 {
  // RustDDS-derived re-exports are only available on the `dds` backend.
  // The `zenoh` backend provides owned equivalents (see issue E1 / ADR-0004).
  #[cfg(feature = "dds")]
  pub use rustdds::{qos::policy, Duration, QosPolicies, QosPolicyBuilder, Timestamp};
  //TODO: re-export RustDDS error types until ros2-client defines its own
  #[cfg(feature = "dds")]
  pub use rustdds::dds::{CreateError, ReadError, WaitError, WriteError};

  pub use crate::log::LogLevel;
  // TODO: What to do about SecurityError (exists based on feature "security")
  pub use crate::names::Name; // import Name as ros2::Name if there is clash
  // otherwise
  // Backend-neutral QoS (available on both backends).
  pub use crate::qos::QosProfile;
}

/// Re-export of the entire RustDDS,
/// to provide access to the same version that ros2-client uses.
///
/// Only available on the `dds` backend.
#[cfg(feature = "dds")]
pub use rustdds;

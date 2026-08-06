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
// Middleware backend selection. See
// docs/decisions/0002-dual-backend-compile-time-feature-selection.md and
// docs/decisions/0010-converge-api-surfaces.md (Phase 5).
//
// Both `dds` and `zenoh` may be enabled in the same build: entity types are
// namespaced under `ros2_client::dds` / `ros2_client::zenoh` so the two
// backends' `Context`/`Node`/… do not collide. When exactly one backend is
// enabled, its entity types are additionally re-exported at the crate root
// for convenience (as before).
// ---------------------------------------------------------------------------
#[cfg(not(any(feature = "dds", feature = "zenoh")))]
compile_error!(
  "no middleware backend selected: enable `dds` and/or `zenoh`. \
   You likely used `--no-default-features` without `--features dds` or `--features zenoh`."
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
/// Owned create/read/write/wait/service errors (ADR-0010 Phase 3).
pub mod error;
pub mod gid;
/// Backend-neutral ROS 2 graph / discovery types (`GraphEvent`, `GraphEntity`,
/// `EntityKind`, `DiscoveredTopic`). Always compiled; see ADR-0005 / ADR-0010.
pub mod graph;
pub mod log;
pub mod message;
pub mod message_info;
pub mod names;
/// Shared [`NodeOptions`] builder (ADR-0010 Phase 4), used by both backends.
pub mod node_options;
/// Rust-like representation of ROS 2 Parameters (backend-neutral).
pub mod parameters;
#[doc(hidden)]
#[cfg(feature = "dds")]
pub mod pubsub;
/// Backend-neutral Quality-of-Service profile.
pub mod qos;
/// `rcl_interfaces` message/service payload types (backend-neutral).
pub mod rcl_interfaces;
/// Owned service request identity ([`RmwRequestId`](request_id::RmwRequestId)).
pub mod request_id;
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

// ---------------------------------------------------------------------------
// Backend entity API namespaces (ADR-0010 Phase 5).
//
// Each backend's `Context` / `Node` / pub-sub / service / action / rosout
// types live behind an explicit module so a build enabling *both* `dds` and
// `zenoh` can reach both stacks without a name collision. Shared,
// backend-neutral types (QoS, errors, `MessageInfo`, `Gid`, discovery, …) are
// re-exported unconditionally at the crate root below, regardless of how many
// backends are enabled.
// ---------------------------------------------------------------------------

/// DDS backend entity API and escape hatches (ADR-0010 Phase 5).
///
/// Reachable as `ros2_client::dds::…` regardless of whether `zenoh` is also
/// enabled. When `zenoh` is *not* enabled, these types are additionally
/// re-exported at the crate root (e.g. `ros2_client::Context`) for
/// convenience, same as before this module existed.
///
/// # Escape hatches
///
/// [`Context::domain_participant`] and [`Context::from_domain_participant`]
/// give access to the underlying RustDDS
/// [`DomainParticipant`](rustdds::DomainParticipant). [`dds::rustdds`]
/// re-exports the whole RustDDS crate at the version `ros2-client` uses.
#[cfg(feature = "dds")]
pub mod dds {
  /// Escape hatch: the whole RustDDS crate, at the same version `ros2-client`
  /// uses internally. Was previously `ros2_client::rustdds` (ADR-0010 Phase 5
  /// moved it here to avoid crate-root pollution / collisions with `zenoh`).
  #[doc(inline)]
  pub use rustdds;

  #[doc(inline)]
  pub use crate::context::{
    Context, ContextOptions, DEFAULT_PUBLISHER_QOS, DEFAULT_SUBSCRIPTION_QOS,
  };
  #[doc(inline)]
  pub use crate::node::{
    Node, NodeCreateError, NodeEvent, ParameterError, ReaderWait, Spinner, WriterWait,
  };
  #[doc(inline)]
  pub use crate::pubsub::{Publisher, Subscription};
  #[doc(inline)]
  pub use crate::service::{AService, Client, Server, Service, ServiceMapping};
  #[doc(inline)]
  pub use crate::action::{Action, ActionTypes};
  #[doc(inline)]
  pub use crate::rosout::{NodeLoggingHandle, RosoutRaw};
}

/// Zenoh backend entity API (ADR-0010 Phase 5).
///
/// Reachable as `ros2_client::zenoh::…` regardless of whether `dds` is also
/// enabled. When `dds` is *not* enabled, these types are additionally
/// re-exported at the crate root (e.g. `ros2_client::Context`) for
/// convenience, same as before this module existed.
///
/// # Escape hatches
///
/// [`Context::session`] gives access to the underlying [`zenoh::Session`].
#[cfg(feature = "zenoh")]
pub mod zenoh {
  #[doc(inline)]
  pub use crate::zenoh_backend::context::{Context, ContextOptions};
  #[doc(inline)]
  pub use crate::zenoh_backend::node::{Node, Topic};
  #[doc(inline)]
  pub use crate::zenoh_backend::pubsub::{Publisher, Subscription};
  #[doc(inline)]
  pub use crate::zenoh_backend::service::{Client, Server};
  #[doc(inline)]
  pub use crate::zenoh_backend::action::{ActionClient, ActionServer, GoalId};
  #[doc(inline)]
  pub use crate::zenoh_backend::parameters::{ParameterClient, ParameterEvent, ParameterServer};
  #[doc(inline)]
  pub use crate::zenoh_backend::rosout::Logger;
}

// Re-exports from crate root to simplify usage
#[doc(inline)]
pub use distributions::{RosDistro, COMPILED_ROS_DISTRO};
#[doc(inline)]
pub use message::Message;
#[doc(inline)]
pub use names::{ActionTypeName, MessageTypeName, Name, NodeName, ServiceTypeName};
#[doc(inline)]
pub use gid::Gid;
#[doc(inline)]
pub use graph::{DiscoveredTopic, EntityKind, GraphEntity, GraphEvent};
#[doc(inline)]
pub use error::{
  CreateError, CreateResult, ReadError, ReadResult, ServiceError, ServiceResult, WaitError,
  WaitResult, WriteError, WriteResult,
};
#[doc(inline)]
pub use message_info::MessageInfo;
#[doc(inline)]
pub use request_id::RmwRequestId;
/// Shared by both backends (ADR-0010 Phase 4); see [`node_options`].
#[doc(inline)]
pub use node_options::NodeOptions;
#[doc(inline)]
pub use parameters::{Parameter, ParameterValue};
#[doc(inline)]
pub use qos::QosProfile;
#[doc(inline)]
pub use wide_string::WString;
#[doc(inline)]
pub use ros_time::{ROSTime, SystemTime};
#[doc(inline)]
pub use log::Log;
// Backend entity API re-exports at the crate root: only when exactly one
// backend feature is enabled, so `Context`/`Node`/… stay unambiguous. With
// both `dds` and `zenoh` enabled, use `ros2_client::dds::…` /
// `ros2_client::zenoh::…` explicitly (ADR-0010 Phase 5).
#[cfg(all(feature = "dds", not(feature = "zenoh")))]
#[doc(inline)]
pub use dds::{
  AService, Action, ActionTypes, Client, Context, ContextOptions, Node, NodeCreateError, NodeEvent,
  NodeLoggingHandle, ParameterError, Publisher, ReaderWait, RosoutRaw, Server, Service,
  ServiceMapping, Spinner, Subscription, WriterWait, DEFAULT_PUBLISHER_QOS,
  DEFAULT_SUBSCRIPTION_QOS,
};
#[cfg(all(feature = "zenoh", not(feature = "dds")))]
#[doc(inline)]
pub use zenoh::{
  ActionClient, ActionServer, Client, Context, ContextOptions, GoalId, Logger, Node,
  ParameterClient, ParameterEvent, ParameterServer, Publisher, Server, Subscription, Topic,
};

/// Module for stuff we do not want to export from top level;
pub mod ros2 {
  // RustDDS-derived re-exports are only available on the `dds` backend.
  // The `zenoh` backend provides owned equivalents (see issue E1 / ADR-0004).
  #[cfg(feature = "dds")]
  pub use rustdds::{qos::policy, Duration, QosPolicies, QosPolicyBuilder, Timestamp};

  // Owned operation errors (ADR-0010 Phase 3); previously RustDDS types.
  pub use crate::error::{CreateError, ReadError, WaitError, WriteError};
  pub use crate::log::LogLevel;
  // TODO: What to do about SecurityError (exists based on feature "security")
  pub use crate::names::Name; // import Name as ros2::Name if there is clash
  // otherwise
  // Backend-neutral QoS (available on both backends).
  pub use crate::qos::QosProfile;
}

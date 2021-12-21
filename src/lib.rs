//! ROS2 interface using DDS module
//!
//! # Examples
//!
//! ```
//! use rustdds::dds::DomainParticipant;
//! use rustdds::dds::data_types::TopicKind;
//! use rustdds::dds::traits::RTPSEntity;
//! use rustdds::ros2::RosParticipant;
//! use rustdds::ros2::NodeOptions;
//! use rustdds::ros2::Node;
//! use rustdds::ros2::builtin_datatypes::NodeEntitiesInfo;
//! use rustdds::dds::qos::QosPolicies;
//! use rustdds::serialization::CDRSerializerAdapter;
//!
//!
//!
//! // RosParticipant is needed for defined RosNodes to be visible in ROS2 network.
//! let mut ros_participant = RosParticipant::new().unwrap();
//!
//!
//! // declaring ros node
//! let mut ros_node = ros_participant.new_ros_node(
//!   "some_node_name",
//!   "/some_namespace",
//!   NodeOptions::new(false), // enable rosout?
//!   ).unwrap();
//!
//! // Creating some topic for Node
//! let some_topic = ros_node.create_ros_topic(
//!     "some_topic_name",
//!     "NodeEntitiesInfo".to_string(),
//!     &QosPolicies::builder().build(),
//!     TopicKind::NoKey)
//!   .unwrap();
//!
//! // declaring some writer that use non keyed types
//! let some_writer = ros_node
//!   .create_ros_nokey_publisher::<NodeEntitiesInfo, CDRSerializerAdapter<_>>(
//!     &some_topic, None)
//!   .unwrap();
//!
//! // Readers and RosParticipant implement mio Evented trait and thus function the same way as
//! // std::sync::mpcs and can be handled the same way for reading the data
//! ```

#[macro_use] extern crate lazy_static;

/// Some builtin datatypes needed for ROS2 communication
/// Some convenience topic infos for ROS2 communication
pub mod builtin_topics;

mod gid;
pub mod node_entities_info;
pub mod context;
pub mod participant_entities_info;
pub mod parameters;
pub mod log;

pub(crate) mod node;

pub use node::*;
pub use context::*;

/// Module for stuff we do not want to export from top level;
pub mod ros2 {
  pub use rustdds::{Duration,Timestamp};
}

use rustdds::*;

pub type RosSubscriber<D, DA> = no_key::DataReader<D, DA>;

pub type KeyedRosSubscriber<D, DA> = with_key::DataReader<D, DA>;

pub type RosPublisher<D, SA> = no_key::DataWriter<D, SA>;

pub type KeyedRosPublisher<D, SA> = with_key::DataWriter<D, SA>;

// Short-hand notation for CDR serialization

pub type RosSubscriberCdr<D> = no_key::DataReaderCdr<D>;

pub type KeyedRosSubscriberCdr<D> =  with_key::DataReaderCdr<D>;

pub type RosPublisherCdr<D> = no_key::DataWriterCdr<D>;

pub type KeyedRosPublisherCdr<D> = with_key::DataWriterCdr<D>;

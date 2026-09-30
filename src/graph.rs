//! Backend-neutral ROS 2 graph / discovery types (ADR-0005, ADR-0010 Phase 2).
//!
//! [`GraphEvent`] / [`GraphEntity`] / [`EntityKind`] describe discovery
//! (entities appearing/disappearing) without leaking either middleware's wire
//! types (`rustdds::DomainParticipantStatusEvent` on DDS, liveliness tokens on
//! Zenoh) into the public API. Both backends map their own discovery
//! mechanism onto these owned types:
//!
//! * The Zenoh backend derives them from parsed `@ros2_lv/**` liveliness tokens
//!   (see [`crate::zenoh_backend::graph_cache`]), which carry full
//!   node/topic/type information.
//! * The DDS backend derives them from
//!   [`rustdds::dds::statusevents::DomainParticipantStatusEvent`]
//!   matched-entity events (see [`crate::node`]). DDS SEDP matching events only
//!   carry GUIDs, not topic/node names, so the DDS-sourced [`GraphEntity`] is
//!   best-effort: `node_name` holds a `"guid:<GUID>"` placeholder and
//!   `name`/`type_name` are `None`. A future iteration could enrich this from
//!   `ros_discovery_info` / SEDP topic data.
//!
//! This module has no dependency on either middleware crate, so it always
//! compiles and is unit-testable regardless of which backend feature(s) are
//! enabled.

/// A change in the ROS 2 graph (an entity became visible or disappeared).
///
/// Backend-neutral: produced by both the DDS backend (mapped from
/// `DomainParticipantStatusEvent`, see [`crate::NodeEvent::Graph`]) and the
/// Zenoh backend (mapped from liveliness tokens, see
/// [`crate::zenoh_backend::context::Context::graph_event_stream`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphEvent {
  /// An entity became visible in the graph.
  EntityDeclared(GraphEntity),
  /// An entity was removed from the graph.
  EntityUndeclared(GraphEntity),
}

/// A discovered ROS 2 graph entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEntity {
  /// What kind of entity this is.
  pub kind: EntityKind,
  /// Fully-qualified name of the owning node (e.g. `/robot1/talker`).
  ///
  /// On the DDS backend this is currently a `"guid:<GUID>"` placeholder (see
  /// the module docs); on the Zenoh backend it is the real node name parsed
  /// from the liveliness token.
  pub node_name: String,
  /// Topic/service name (`None` for a node entity, or when unavailable).
  pub name: Option<String>,
  /// DDS-form type name (`None` for a node entity, or when unavailable).
  pub type_name: Option<String>,
}

/// Kind of a discovered ROS 2 graph entity.
///
/// Named after the two-letter codes used in `rmw_zenoh` liveliness keys (see
/// `docs/zenoh_study/research/rmw_zenoh.md` §2), but the type itself is
/// backend-neutral and also used to describe DDS-discovered entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
  /// Node.
  Node,
  /// Message publisher.
  Publisher,
  /// Message subscription.
  Subscription,
  /// Service server.
  ServiceServer,
  /// Service client.
  ServiceClient,
}

impl EntityKind {
  /// The two-letter code as it appears in a Zenoh liveliness key.
  pub const fn code(self) -> &'static str {
    match self {
      EntityKind::Node => "NN",
      EntityKind::Publisher => "MP",
      EntityKind::Subscription => "MS",
      EntityKind::ServiceServer => "SS",
      EntityKind::ServiceClient => "SC",
    }
  }
}

/// A discovered ROS 2 topic: its fully-qualified name and DDS-form type name.
///
/// Backend-neutral replacement for exposing
/// `rustdds::discovery::DiscoveredTopicData` directly (ADR-0010 Phase 2).
/// Produced by [`crate::context::Context::discovered_topics`] on the DDS
/// backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredTopic {
  /// Fully-qualified topic name (e.g. `/chatter`).
  pub name: String,
  /// DDS-form type name (e.g. `std_msgs::msg::dds_::String_`).
  pub type_name: String,
}

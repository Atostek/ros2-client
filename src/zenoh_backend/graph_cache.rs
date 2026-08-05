//! In-memory ROS 2 graph cache built from liveliness tokens (E5).
//!
//! A [`Context`](super::context::Context) declares a liveliness subscriber over
//! `@ros2_lv/<domain>/**`; each PUT/DELETE sample is fed to [`GraphCache`],
//! which parses the token key ([`parse_liveliness_key`]) and maintains the set
//! of live entities. Queries (entity counts, node names) read this set — the
//! Zenoh analogue of the DDS `ros_discovery_info` graph.
//!
//! The parsing/query logic is backend-neutral (no `zenoh` crate), so it is
//! unit-tested on any build.

use std::{collections::HashMap, sync::Mutex};

use async_channel::{Receiver, Sender};

use super::keyexpr::{parse_liveliness_key, EntityKind, ParsedEntity};

/// A change in the ROS 2 graph, delivered by
/// [`Context::graph_event_stream`](super::context::Context::graph_event_stream).
///
/// Backend-neutral: the same shape would be produced by the DDS backend's
/// discovery (ADR-0004). It replaces the RustDDS-specific `NodeEvent::DDS` for
/// graph observation on the Zenoh backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphEvent {
  /// An entity became visible in the graph.
  EntityDeclared(GraphEntity),
  /// An entity was removed from the graph.
  EntityUndeclared(GraphEntity),
}

/// A discovered ROS 2 graph entity (backend-neutral view of a liveliness
/// token).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEntity {
  /// What kind of entity this is.
  pub kind: EntityKind,
  /// Fully-qualified name of the owning node (e.g. `/robot1/talker`).
  pub node_name: String,
  /// Topic/service name (`None` for a node entity).
  pub name: Option<String>,
  /// DDS-form type name (`None` for a node entity).
  pub type_name: Option<String>,
}

impl From<&ParsedEntity> for GraphEntity {
  fn from(e: &ParsedEntity) -> Self {
    GraphEntity {
      kind: e.kind,
      node_name: join_fqn(&e.namespace, &e.node_name),
      name: e.topic_name.clone(),
      type_name: e.type_name.clone(),
    }
  }
}

/// A thread-safe cache of discovered ROS 2 entities, keyed by their liveliness
/// token key expression.
#[derive(Default)]
pub struct GraphCache {
  entities: Mutex<HashMap<String, ParsedEntity>>,
  // Fan-out event subscribers (one unbounded channel per `subscribe()` caller),
  // mirroring the DDS backend's `status_event_senders`.
  subscribers: Mutex<Vec<Sender<GraphEvent>>>,
}

impl GraphCache {
  /// Record an entity from a liveliness-token PUT. Non-token keys are ignored.
  pub fn apply_put(&self, key: &str) {
    if let Some(entity) = parse_liveliness_key(key) {
      let event = GraphEvent::EntityDeclared(GraphEntity::from(&entity));
      self.entities.lock().unwrap().insert(key.to_owned(), entity);
      self.broadcast(event);
    }
  }

  /// Remove an entity on a liveliness-token DELETE.
  pub fn apply_delete(&self, key: &str) {
    let removed = self.entities.lock().unwrap().remove(key);
    if let Some(entity) = removed {
      self.broadcast(GraphEvent::EntityUndeclared(GraphEntity::from(&entity)));
    }
  }

  /// Register a new event stream. Each subscriber receives every subsequent
  /// [`GraphEvent`]; drop the receiver to unsubscribe.
  pub fn subscribe(&self) -> Receiver<GraphEvent> {
    let (tx, rx) = async_channel::unbounded();
    self.subscribers.lock().unwrap().push(tx);
    rx
  }

  fn broadcast(&self, event: GraphEvent) {
    let mut subs = self.subscribers.lock().unwrap();
    // Drop closed receivers; deliver to the rest (unbounded => only fails if
    // closed).
    subs.retain(|s| !s.is_closed());
    for s in subs.iter() {
      let _ = s.try_send(event.clone());
    }
  }

  pub(crate) fn count_kind_on_topic(&self, kind: EntityKind, topic: &str) -> usize {
    self
      .entities
      .lock()
      .unwrap()
      .values()
      .filter(|e| e.kind == kind && e.topic_name.as_deref() == Some(topic))
      .count()
  }

  /// Number of discovered publishers on a topic (by fully-qualified name).
  pub fn publisher_count(&self, topic: &str) -> usize {
    self.count_kind_on_topic(EntityKind::Publisher, topic)
  }

  /// Number of discovered subscriptions on a topic.
  pub fn subscription_count(&self, topic: &str) -> usize {
    self.count_kind_on_topic(EntityKind::Subscription, topic)
  }

  /// Number of discovered service servers on a service name.
  pub fn service_server_count(&self, service: &str) -> usize {
    self.count_kind_on_topic(EntityKind::ServiceServer, service)
  }

  /// Fully-qualified names of all discovered nodes, sorted and de-duplicated.
  pub fn node_names(&self) -> Vec<String> {
    let guard = self.entities.lock().unwrap();
    let mut names: Vec<String> = guard
      .values()
      .filter(|e| e.kind == EntityKind::Node)
      .map(|e| join_fqn(&e.namespace, &e.node_name))
      .collect();
    names.sort();
    names.dedup();
    names
  }

  /// Total number of cached entities (all kinds). Mainly for diagnostics/tests.
  pub fn len(&self) -> usize {
    self.entities.lock().unwrap().len()
  }

  /// Whether the cache is empty.
  pub fn is_empty(&self) -> bool {
    self.entities.lock().unwrap().is_empty()
  }
}

fn join_fqn(namespace: &str, name: &str) -> String {
  if namespace.is_empty() || namespace == "/" {
    format!("/{name}")
  } else {
    format!("{namespace}/{name}")
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;
  use crate::zenoh_backend::keyexpr::{
    entity_liveliness_keyexpr, node_liveliness_keyexpr, EntityIds,
  };

  fn ids(entity_id: u64) -> EntityIds<'static> {
    EntityIds {
      session_id: "aac3178e146ba6f1fc6e6a4085e77f21",
      node_id: 0,
      entity_id,
      enclave: "",
      namespace: "",
      node_name: "talker",
    }
  }

  #[test]
  fn tracks_entities_and_counts() {
    let cache = GraphCache::default();
    let node_key = node_liveliness_keyexpr(0, &ids(0));
    let pub_key = entity_liveliness_keyexpr(
      0,
      &ids(1),
      EntityKind::Publisher,
      "/chatter",
      "std_msgs::msg::dds_::String_",
      "RIHS01_x",
      "::,7:,:,:,,",
    );

    cache.apply_put(&node_key);
    cache.apply_put(&pub_key);
    // A non-token key is ignored.
    cache.apply_put("0/chatter/std_msgs::msg::dds_::String_/hash");

    assert_eq!(cache.publisher_count("/chatter"), 1);
    assert_eq!(cache.subscription_count("/chatter"), 0);
    assert_eq!(cache.node_names(), vec!["/talker".to_string()]);
    assert_eq!(cache.len(), 2);

    // DELETE removes the publisher.
    cache.apply_delete(&pub_key);
    assert_eq!(cache.publisher_count("/chatter"), 0);
    assert_eq!(cache.len(), 1);
  }

  #[test]
  fn emits_declared_and_undeclared_events() {
    let cache = GraphCache::default();
    let stream = cache.subscribe();
    let pub_key = entity_liveliness_keyexpr(
      0,
      &ids(1),
      EntityKind::Publisher,
      "/chatter",
      "std_msgs::msg::dds_::String_",
      "RIHS01_x",
      "::,7:,:,:,,",
    );

    cache.apply_put(&pub_key);
    cache.apply_delete(&pub_key);
    // A non-token key produces no event.
    cache.apply_put("0/chatter/not-a-token");

    let declared = stream.try_recv().expect("a declared event");
    match declared {
      GraphEvent::EntityDeclared(e) => {
        assert_eq!(e.kind, EntityKind::Publisher);
        assert_eq!(e.name.as_deref(), Some("/chatter"));
        assert_eq!(e.node_name, "/talker");
      }
      other => panic!("expected EntityDeclared, got {:?}", other),
    }
    match stream.try_recv().expect("an undeclared event") {
      GraphEvent::EntityUndeclared(e) => assert_eq!(e.kind, EntityKind::Publisher),
      other => panic!("expected EntityUndeclared, got {:?}", other),
    }
    // No third event (the non-token PUT was ignored).
    assert!(stream.try_recv().is_err());
  }
}

//! Zenoh key-expression construction for the ROS 2 ↔ Zenoh mapping.
//!
//! Two families of keys, both defined by the official `rmw_zenoh` (see
//! `docs/zenoh_study/research/rmw_zenoh.md` §2–§3):
//!
//! * **Data-plane topic/service keys** — `<domain>/<name>/<type>/<type_hash>`,
//!   with the *real* namespace slashes preserved in the name.
//! * **Liveliness (discovery) keys** — under the `@ros2_lv` admin space, with
//!   the name **mangled** (`/`→`%`, empty→`%`) because Zenoh liveliness keys
//!   may not contain empty chunks.
//!
//! This module is pure string manipulation and carries no dependency on the
//! `zenoh` crate, so it is unit-tested on every build.

/// Admin-space prefix for ROS 2 liveliness tokens (`rmw_zenoh` `ADMIN_SPACE`).
pub const ADMIN_SPACE: &str = "@ros2_lv";

/// Two-letter entity kind codes used in liveliness keys.
///
/// From `rmw_zenoh` `liveliness_utils.cpp`.
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
  /// The two-letter code as it appears in a liveliness key.
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

/// Mangle a ROS name for use in a liveliness key: `/`→`%`, and an empty string
/// becomes a single `%` (Zenoh keys cannot contain empty chunks).
pub fn mangle(name: &str) -> String {
  if name.is_empty() {
    "%".to_owned()
  } else {
    name.replace('/', "%")
  }
}

/// Inverse of [`mangle`]: `%`→`/`. Note a lone `%` demangles to `/` (the root /
/// empty case is inherently ambiguous and callers treat `/` and empty alike).
pub fn demangle(name: &str) -> String {
  name.replace('%', "/")
}

/// Strip a single leading and trailing `/` from a fully-qualified name, keeping
/// interior slashes. `/chatter` → `chatter`, `/robot1/chatter` →
/// `robot1/chatter`.
fn strip_slashes(fq_name: &str) -> &str {
  fq_name.trim_matches('/')
}

/// Build a data-plane topic (or service) key expression:
/// `<domain>/<name>/<type>/<type_hash>`.
///
/// `fq_name` is the fully-qualified topic/service name (leading/trailing
/// slashes are stripped, interior slashes kept). `type_name` is the DDS-form
/// type name (e.g. `std_msgs::msg::dds_::String_`), `type_hash` the REP-2016
/// `RIHS01_…` string (or a wildcard for liberal receivers, see
/// [`super::type_hash`]).
pub fn topic_keyexpr(domain_id: u16, fq_name: &str, type_name: &str, type_hash: &str) -> String {
  format!(
    "{}/{}/{}/{}",
    domain_id,
    strip_slashes(fq_name),
    type_name,
    type_hash
  )
}

/// Fields common to every liveliness token.
#[derive(Clone, Debug)]
pub struct EntityIds<'a> {
  /// Zenoh session id (one per context), hex.
  pub session_id: &'a str,
  /// Node id within the context.
  pub node_id: u64,
  /// Entity id within the node. For a node token this equals `node_id`.
  pub entity_id: u64,
  /// SROS enclave, unmangled; empty for the default.
  pub enclave: &'a str,
  /// Node namespace, unmangled (e.g. `/` or `/robot1`).
  pub namespace: &'a str,
  /// Node base name.
  pub node_name: &'a str,
}

/// Build a **node** liveliness key (`NN`), 9 components:
/// `@ros2_lv/<domain>/<zid>/<nid>/<nid>/NN/<enclave>/<namespace>/<node>`.
pub fn node_liveliness_keyexpr(domain_id: u16, ids: &EntityIds) -> String {
  format!(
    "{admin}/{domain}/{zid}/{nid}/{nid}/{kind}/{enclave}/{ns}/{node}",
    admin = ADMIN_SPACE,
    domain = domain_id,
    zid = ids.session_id,
    nid = ids.node_id,
    kind = EntityKind::Node.code(),
    enclave = mangle(ids.enclave),
    ns = mangle(ids.namespace),
    node = mangle(ids.node_name),
  )
}

/// Build a publisher / subscription / service liveliness key.
///
/// `qualified_name` is the fully-qualified topic/service name (e.g.
/// `/chatter`); it is mangled. `qos` is the compact QoS encoding string (opaque
/// here — see the QoS-encoding work item; callers pass the already-encoded
/// value).
#[allow(clippy::too_many_arguments)]
pub fn entity_liveliness_keyexpr(
  domain_id: u16,
  ids: &EntityIds,
  kind: EntityKind,
  qualified_name: &str,
  type_name: &str,
  type_hash: &str,
  qos: &str,
) -> String {
  format!(
    "{admin}/{domain}/{zid}/{nid}/{eid}/{kind}/{enclave}/{ns}/{node}/{name}/{ty}/{hash}/{qos}",
    admin = ADMIN_SPACE,
    domain = domain_id,
    zid = ids.session_id,
    nid = ids.node_id,
    eid = ids.entity_id,
    kind = kind.code(),
    enclave = mangle(ids.enclave),
    ns = mangle(ids.namespace),
    node = mangle(ids.node_name),
    name = mangle(qualified_name),
    ty = type_name,
    hash = type_hash,
    qos = qos,
  )
}

/// The liveliness key expression a context subscribes to (and queries) to build
/// its graph cache: `@ros2_lv/<domain>/**`.
pub fn graph_cache_keyexpr(domain_id: u16) -> String {
  format!("{ADMIN_SPACE}/{domain_id}/**")
}

/// A liveliness token parsed back into its components (the inverse of
/// [`node_liveliness_keyexpr`] / [`entity_liveliness_keyexpr`]). Names are
/// demangled (`%`→`/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedEntity {
  /// ROS domain id.
  pub domain_id: u16,
  /// Zenoh session id (hex).
  pub zid: String,
  /// Node id within the session.
  pub node_id: u64,
  /// Entity id within the node (equals `node_id` for a node token).
  pub entity_id: u64,
  /// Entity kind.
  pub kind: EntityKind,
  /// Node namespace (demangled), e.g. `/` or `/robot1`.
  pub namespace: String,
  /// Node base name.
  pub node_name: String,
  /// Topic/service name (demangled), for non-node entities.
  pub topic_name: Option<String>,
  /// DDS-form type name, for non-node entities.
  pub type_name: Option<String>,
  /// REP-2016 type hash, for non-node entities.
  pub type_hash: Option<String>,
  /// Compact QoS string, for non-node entities.
  pub qos: Option<String>,
}

fn parse_kind(code: &str) -> Option<EntityKind> {
  Some(match code {
    "NN" => EntityKind::Node,
    "MP" => EntityKind::Publisher,
    "MS" => EntityKind::Subscription,
    "SS" => EntityKind::ServiceServer,
    "SC" => EntityKind::ServiceClient,
    _ => return None,
  })
}

/// Parse a liveliness token key expression into a [`ParsedEntity`], or `None`
/// if it is not a well-formed `@ros2_lv` token.
pub fn parse_liveliness_key(key: &str) -> Option<ParsedEntity> {
  let p: Vec<&str> = key.split('/').collect();
  if p.first() != Some(&ADMIN_SPACE) || p.len() < 9 {
    return None;
  }
  let domain_id = p[1].parse().ok()?;
  let zid = p[2].to_owned();
  let node_id = p[3].parse().ok()?;
  let entity_id = p[4].parse().ok()?;
  let kind = parse_kind(p[5])?;
  let namespace = demangle(p[7]); // p[6] is the enclave (unused here)
  let node_name = demangle(p[8]);

  let (topic_name, type_name, type_hash, qos) = if kind == EntityKind::Node {
    (None, None, None, None)
  } else {
    if p.len() < 13 {
      return None;
    }
    (
      Some(demangle(p[9])),
      Some(p[10].to_owned()),
      Some(p[11].to_owned()),
      Some(p[12].to_owned()),
    )
  };

  Some(ParsedEntity {
    domain_id,
    zid,
    node_id,
    entity_id,
    kind,
    namespace,
    node_name,
    topic_name,
    type_name,
    type_hash,
    qos,
  })
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  // Concrete type hashes from docs/zenoh_study/research/rmw_zenoh.md examples.
  const STRING_HASH: &str =
    "RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18";
  const ADDTWOINTS_HASH: &str =
    "RIHS01_e118de6bf5eeb66a2491b5bda11202e7b68f198d6f67922cf30364858239c81a";

  #[test]
  fn topic_key_matches_rmw_zenoh_examples() {
    assert_eq!(
      topic_keyexpr(0, "/chatter", "std_msgs::msg::dds_::String_", STRING_HASH),
      format!("0/chatter/std_msgs::msg::dds_::String_/{STRING_HASH}")
    );
    // Namespaced: interior slash kept, leading slash stripped.
    assert_eq!(
      topic_keyexpr(
        0,
        "/robot1/chatter",
        "std_msgs::msg::dds_::String_",
        STRING_HASH
      ),
      format!("0/robot1/chatter/std_msgs::msg::dds_::String_/{STRING_HASH}")
    );
    // Service key on domain 2.
    assert_eq!(
      topic_keyexpr(
        2,
        "/add_two_ints",
        "example_interfaces::srv::dds_::AddTwoInts_",
        ADDTWOINTS_HASH
      ),
      format!("2/add_two_ints/example_interfaces::srv::dds_::AddTwoInts_/{ADDTWOINTS_HASH}")
    );
  }

  #[test]
  fn mangling_roundtrips_and_handles_empty() {
    assert_eq!(mangle(""), "%");
    assert_eq!(mangle("/"), "%");
    assert_eq!(mangle("/chatter"), "%chatter");
    assert_eq!(mangle("/robot1/chatter"), "%robot1%chatter");
    assert_eq!(demangle("%chatter"), "/chatter");
    assert_eq!(demangle("%robot1%chatter"), "/robot1/chatter");
  }

  #[test]
  fn node_liveliness_key_matches_example() {
    // @ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/0/NN/%/%/listener
    let ids = EntityIds {
      session_id: "aac3178e146ba6f1fc6e6a4085e77f21",
      node_id: 0,
      entity_id: 0,
      enclave: "",
      namespace: "",
      node_name: "listener",
    };
    assert_eq!(
      node_liveliness_keyexpr(0, &ids),
      "@ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/0/NN/%/%/listener"
    );
  }

  #[test]
  fn subscription_liveliness_key_matches_example() {
    // @ros2_lv/0/aac.../0/10/MS/%/%/listener/%chatter/std_msgs::msg::dds_::String_/
    // <hash>/::,10:,:,:,,
    let ids = EntityIds {
      session_id: "aac3178e146ba6f1fc6e6a4085e77f21",
      node_id: 0,
      entity_id: 10,
      enclave: "",
      namespace: "",
      node_name: "listener",
    };
    let key = entity_liveliness_keyexpr(
      0,
      &ids,
      EntityKind::Subscription,
      "/chatter",
      "std_msgs::msg::dds_::String_",
      STRING_HASH,
      "::,10:,:,:,,",
    );
    assert_eq!(
      key,
      format!(
        "@ros2_lv/0/aac3178e146ba6f1fc6e6a4085e77f21/0/10/MS/%/%/listener/%chatter/\
         std_msgs::msg::dds_::String_/{STRING_HASH}/::,10:,:,:,,"
      )
    );
  }

  #[test]
  fn entity_kind_codes() {
    assert_eq!(EntityKind::Node.code(), "NN");
    assert_eq!(EntityKind::Publisher.code(), "MP");
    assert_eq!(EntityKind::Subscription.code(), "MS");
    assert_eq!(EntityKind::ServiceServer.code(), "SS");
    assert_eq!(EntityKind::ServiceClient.code(), "SC");
  }

  #[test]
  fn graph_cache_key() {
    assert_eq!(graph_cache_keyexpr(0), "@ros2_lv/0/**");
    assert_eq!(graph_cache_keyexpr(42), "@ros2_lv/42/**");
  }

  #[test]
  fn parse_node_token() {
    let ids = EntityIds {
      session_id: "aac3178e146ba6f1fc6e6a4085e77f21",
      node_id: 3,
      entity_id: 3,
      enclave: "",
      namespace: "/robot1",
      node_name: "talker",
    };
    let key = node_liveliness_keyexpr(0, &ids);
    let e = parse_liveliness_key(&key).expect("parse node token");
    assert_eq!(e.kind, EntityKind::Node);
    assert_eq!(e.domain_id, 0);
    assert_eq!(e.node_id, 3);
    assert_eq!(e.entity_id, 3);
    assert_eq!(e.namespace, "/robot1");
    assert_eq!(e.node_name, "talker");
    assert_eq!(e.topic_name, None);
  }

  #[test]
  fn parse_entity_token_roundtrip() {
    let ids = EntityIds {
      session_id: "aac3178e146ba6f1fc6e6a4085e77f21",
      node_id: 0,
      entity_id: 10,
      enclave: "",
      namespace: "",
      node_name: "listener",
    };
    let key = entity_liveliness_keyexpr(
      0,
      &ids,
      EntityKind::Subscription,
      "/chatter",
      "std_msgs::msg::dds_::String_",
      STRING_HASH,
      "::,10:,:,:,,",
    );
    let e = parse_liveliness_key(&key).expect("parse entity token");
    assert_eq!(e.kind, EntityKind::Subscription);
    assert_eq!(e.entity_id, 10);
    assert_eq!(e.node_name, "listener");
    assert_eq!(e.topic_name.as_deref(), Some("/chatter"));
    assert_eq!(e.type_name.as_deref(), Some("std_msgs::msg::dds_::String_"));
    assert_eq!(e.type_hash.as_deref(), Some(STRING_HASH));
    assert_eq!(e.qos.as_deref(), Some("::,10:,:,:,,"));
  }

  #[test]
  fn parse_rejects_non_tokens() {
    assert!(parse_liveliness_key("0/chatter/std_msgs::msg::dds_::String_/hash").is_none());
    assert!(parse_liveliness_key("@ros2_lv/0").is_none());
  }
}

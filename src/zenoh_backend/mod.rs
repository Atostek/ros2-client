//! Zenoh middleware backend for `ros2-client` (cargo feature `zenoh`).
//!
//! This module tree implements the ROS 2 ↔ Zenoh mapping described in
//! `docs/zenoh_study/` and mirrors the official `rmw_zenoh` design:
//!
//! - topics → key expressions `<domain>/<name>/<type>/<type_hash>`
//! - discovery → liveliness tokens under `@ros2_lv/<domain>/**` + a graph cache
//! - every message carries an attachment `(seq, source_timestamp, source_gid)`
//! - services → Zenoh queryable / get
//! - GID → XXH3-128 of the entity's liveliness key
//!
//! The "wire-format spec" submodules below ([`keyexpr`], [`type_hash`],
//! [`gid`]) are pure (no dependency on the `zenoh` crate), so they are also
//! compiled and unit-tested by `cargo test` on the default `dds` build.

// Backend-neutral wire-format spec (pure; compiled on every build).
pub(crate) mod gid;
pub(crate) mod graph_cache;
pub(crate) mod jazzy_types;
pub(crate) mod keyexpr;
pub(crate) mod qos_encoding;
pub(crate) mod type_description;
pub(crate) mod type_hash;

// Attachment (de)serialization needs the `zenoh-ext` serializer.
#[cfg(feature = "zenoh")]
pub(crate) mod attachment;

// CDR message (de)serialization needs `cdr-encoding`.
#[cfg(feature = "zenoh")]
pub(crate) mod cdr;

// Zenoh session / Context (needs the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod context;

// Node/Topic and Pub/Sub (need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod node;
#[cfg(feature = "zenoh")]
pub(crate) mod pubsub;

// Services over queryable/get (need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod service;

// Actions (services + topics; need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod action;

// Parameters (six services + `parameter_events` topic; need `zenoh`).
#[cfg(feature = "zenoh")]
pub(crate) mod parameters;

// rosout logging (`/rosout` topic; needs the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod rosout;

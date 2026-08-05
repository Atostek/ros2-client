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
//! The submodules are populated by the E2–E6 work items (see
//! `docs/zenoh_study/refactoring_plan.md`).
//!
//! The "wire-format spec" submodules below ([`keyexpr`], [`type_hash`],
//! [`gid`]) are pure (no dependency on the `zenoh` crate), so they compile and
//! are unit-tested on any build — including the default `dds` build, where they
//! are otherwise unused.
#![allow(dead_code)] // spec modules are unused on the `dds` build

// E2 — backend-neutral wire-format spec (pure; compiled on every build).
pub(crate) mod gid;
pub(crate) mod graph_cache;
pub(crate) mod keyexpr;
pub(crate) mod qos_encoding;
pub(crate) mod type_description;
pub(crate) mod type_hash;

// E2 — attachment (de)serialization needs the `zenoh-ext` serializer.
#[cfg(feature = "zenoh")]
pub(crate) mod attachment;

// E2/E4 — CDR message (de)serialization needs `cdr-encoding`.
#[cfg(feature = "zenoh")]
pub(crate) mod cdr;

// E3 — Zenoh session / Context (needs the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod context;

// E4 — Node/Topic and Pub/Sub (need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod node;
#[cfg(feature = "zenoh")]
pub(crate) mod pubsub;

// E6 — Services over queryable/get (need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod service;

// E7 — Actions (services + topics; need the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod action;

// E8 — Parameters (six services + `parameter_events` topic; need `zenoh`).
#[cfg(feature = "zenoh")]
pub(crate) mod parameters;

// E9 — rosout logging (`/rosout` topic; needs the `zenoh` crate).
#[cfg(feature = "zenoh")]
pub(crate) mod rosout;

// E4 — Pub/Sub.
// #[cfg(feature = "zenoh")]
// pub(crate) mod pubsub;

// E5 — Discovery: liveliness tokens + graph cache.
// #[cfg(feature = "zenoh")]
// pub(crate) mod graph_cache;

// E6 — Services over queryable / get.
// #[cfg(feature = "zenoh")]
// pub(crate) mod service;

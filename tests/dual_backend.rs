//! Sanity check that both backend entity APIs are reachable, unambiguously,
//! from the same build when both `dds` and `zenoh` are enabled (ADR-0010
//! Phase 5).
//!
//! Not run by default: requires `--features dds,zenoh` (neither is part of
//! `default`, and the crate does not otherwise build with both).

#![cfg(all(feature = "dds", feature = "zenoh"))]

#[test]
fn dual_backend_modules_construct() {
  let _ = ros2_client::dds::Context::new();
  let _ = ros2_client::zenoh::Context::new();
}

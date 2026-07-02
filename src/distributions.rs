//! ROS 2 distribution identification.
//!
//! The distribution this crate is compiled for is selected via the Cargo
//! feature chain (see `Cargo.toml`): each distribution feature enables all
//! older ones, so `cfg!(feature = "X")` means "at least distribution X".
//!
//! [`COMPILED_ROS_DISTRO`] exposes the selected distribution as a runtime
//! constant, and [`crate::distributions::verify_ros_distro_env`] compares it
//! against the `ROS_DISTRO` environment variable at [`crate::Context`]
//! initialization.

use std::fmt;

#[allow(unused_imports)]
use log::{error, info, warn};

/// A ROS 2 distribution.
///
/// Ordered chronologically, so comparisons (`<`, `>=`, ...) reflect release
/// order. Marked `#[non_exhaustive]` because future distributions will be
/// added.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RosDistro {
  Galactic,
  Humble,
  Iron,
  Jazzy,
  Kilted,
  Lyrical,
}

impl RosDistro {
  /// The lower-case distribution name, matching the `ROS_DISTRO` value ROS 2
  /// sets when its environment is sourced (e.g. `"jazzy"`).
  pub const fn as_str(self) -> &'static str {
    match self {
      RosDistro::Galactic => "galactic",
      RosDistro::Humble => "humble",
      RosDistro::Iron => "iron",
      RosDistro::Jazzy => "jazzy",
      RosDistro::Kilted => "kilted",
      RosDistro::Lyrical => "lyrical",
    }
  }
}

impl fmt::Display for RosDistro {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

/// The ROS 2 distribution this crate was compiled for, i.e. the highest
/// activated distribution feature in the chain.
pub const COMPILED_ROS_DISTRO: RosDistro = compiled_distro();

const fn compiled_distro() -> RosDistro {
  // Features are additive and each enables all older ones, so the newest
  // enabled feature identifies the selected distribution.
  if cfg!(feature = "lyrical") {
    RosDistro::Lyrical
  } else if cfg!(feature = "kilted") {
    RosDistro::Kilted
  } else if cfg!(feature = "jazzy") {
    RosDistro::Jazzy
  } else if cfg!(feature = "iron") {
    RosDistro::Iron
  } else if cfg!(feature = "humble") {
    RosDistro::Humble
  } else {
    RosDistro::Galactic
  }
}

/// Compare the `ROS_DISTRO` environment variable against the distribution this
/// crate was compiled for, and log the outcome:
///
/// * unset -> warning (we cannot tell whether the build matches the runtime),
/// * equal -> info (all good),
/// * `"rolling"` -> warning (rolling tracks the newest, assume compatible),
/// * anything else -> error (likely wire-incompatible, e.g. wrong Gid format).
pub fn verify_ros_distro_env() {
  let built = COMPILED_ROS_DISTRO;
  match std::env::var("ROS_DISTRO") {
    Err(_) => warn!("ROS_DISTRO is not set; ros2-client was built for '{built}'."),
    Ok(distro) if distro == built.as_str() => {
      info!("ROS_DISTRO='{distro}' matches the ros2-client build.");
    }
    Ok(distro) if distro == "rolling" => {
      warn!(
        "ROS_DISTRO='rolling'; ros2-client was built for '{built}'. Assuming compatible, but \
         rolling may have diverged."
      );
    }
    Ok(distro) => {
      error!(
        "ROS_DISTRO='{distro}' but ros2-client was built for '{built}'. These may be \
         incompatible (e.g. different Gid format); rebuild with --features {distro}."
      );
    }
  }
}

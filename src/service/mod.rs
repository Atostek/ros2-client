//! Implementation of ROS 2 [Services](https://docs.ros.org/en/rolling/Tutorials/Beginner-CLI-Tools/Understanding-ROS2-Services/Understanding-ROS2-Services.html)
#[allow(unused_imports)]
use log::{debug, error, info, warn};

pub mod client;
pub mod request_id;
pub mod server;
pub(super) mod wrappers;

pub use request_id::*;
use wrappers::*;
pub use server::*;
pub use client::*;

// --------------------------------------------
// --------------------------------------------

/// Selects how Service Requests and Responses are to be mapped to DDS.
///
/// There are different and incompatible ways to map Services onto DDS Topics.
/// In order to interoperate with ROS 2, you have to select the same mapping it
/// uses. The mapping used by ROS2 depends on the DDS implementation used and
/// its configuration.
///
/// For details, see OMG Specification
/// [RPC over DDS](https://www.omg.org/spec/DDS-RPC/1.0/About-DDS-RPC/)
/// Section "7.2.4 Basic and Enhanced Service Mapping for RPC over DDS",
/// which defines Service Mappings "Basic" and "Enhanced".
///
/// ServiceMapping::Cyclone represents a third mapping used by RMW for
/// CycloneDDS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceMapping {
  /// "Basic" service mapping from RPC over DDS specification.
  /// * RTI Connext with `RMW_CONNEXT_REQUEST_REPLY_MAPPING=basic`, but this is
  ///   not tested, so may not work.
  Basic,

  /// "Enhanced" service mapping from RPC over DDS specification.
  /// * ROS2 Foxy with eProsima DDS,
  /// * ROS2 Galactic with RTI Connext (rmw_connextdds, not rmw_connext_cpp) -
  ///   set environment variable `RMW_CONNEXT_REQUEST_REPLY_MAPPING=extended`
  ///   before running ROS2 executable.
  Enhanced,

  /// CycloneDDS-specific service mapping.
  /// Specification for this mapping is unknown, technical details are
  /// reverse-engineered from ROS2 sources.
  /// * ROS2 Galactic with CycloneDDS - Seems to work on the same host only, not
  ///   over actual network.
  Cyclone,
}

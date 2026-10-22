//! Shared [`NodeOptions`] builder for both middleware backends (ADR-0010).
//!
//! The builder surface (`enable_rosout`, `read_rosout`,
//! `start_parameter_services`, `declare_parameter`, `parameter_validator`,
//! `parameter_set_action`) is
//! unchanged, but fields are `pub(crate)` so each backend's `Node::new` can
//! read/consume them directly (DDS keeps the whole struct; Zenoh wires
//! `enable_rosout` / `enable_rosout_reading` / `start_parameter_services` /
//! `declared_parameters` into an optional `Logger` /
//! rosout [`Subscription`](crate::Subscription) /
//! `ParameterServer` at construction time; see
//! `src/zenoh_backend/node.rs`). Fields not honored by a backend are
//! documented as no-ops there (logged once at `debug` level).

use crate::parameters::{Parameter, ParameterValue, SetParametersResult};

/// Signature of a user-supplied parameter validator / set-action callback
/// (see [`NodeOptions::parameter_validator`] /
/// [`NodeOptions::parameter_set_action`]).
///
/// Currently only honored by the DDS backend; the Zenoh backend accepts and
/// stores it (for API parity) but does not yet call it (logged once at
/// `debug` level if set).
pub(crate) type ParameterFunc = dyn Fn(&str, &ParameterValue) -> SetParametersResult + Send + Sync;

/// Configuration of a [`Node`](crate::Node). This is a builder-like struct.
///
/// The NodeOptions struct does not contain
/// node_name, context, or namespace, because
/// they ae always needed and have no reasonable default.
#[must_use]
pub struct NodeOptions {
  pub(crate) enable_rosout: bool, // use rosout topic for logging?
  pub(crate) enable_rosout_reading: bool,
  pub(crate) start_parameter_services: bool,
  pub(crate) declared_parameters: Vec<Parameter>,
  pub(crate) allow_undeclared_parameters: bool,
  pub(crate) parameter_validator: Option<Box<ParameterFunc>>,
  pub(crate) parameter_set_action: Option<Box<ParameterFunc>>,
}

impl NodeOptions {
  /// Get a default NodeOptions
  pub fn new() -> NodeOptions {
    // These defaults are from rclpy reference
    // https://docs.ros2.org/latest/api/rclpy/api/node.html
    NodeOptions {
      enable_rosout: true,
      enable_rosout_reading: false,
      start_parameter_services: true,
      declared_parameters: Vec::new(),
      allow_undeclared_parameters: false,
      parameter_validator: None,
      parameter_set_action: None,
    }
  }
  pub fn enable_rosout(self, enable_rosout: bool) -> NodeOptions {
    NodeOptions {
      enable_rosout,
      ..self
    }
  }

  /// Whether to offer the six `rcl_interfaces` parameter services.
  ///
  /// The default is `true`. `false` means:
  ///
  /// * **DDS:** `Node::spinner` creates none of `get_parameters`,
  ///   `get_parameter_types`, `set_parameters`, `set_parameters_atomically`,
  ///   `list_parameters`, or `describe_parameters`. The local API
  ///   (`get_parameter`, `set_parameter`, `has_parameter`) still works, and
  ///   `/parameter_events` is still published.
  /// * **Zenoh:** no `ParameterServer` is created, so declared parameters are
  ///   not stored either. `Node::parameter_server()` returns `None`.
  pub fn start_parameter_services(self, start_parameter_services: bool) -> NodeOptions {
    NodeOptions {
      start_parameter_services,
      ..self
    }
  }

  pub fn read_rosout(self, enable_rosout_reading: bool) -> NodeOptions {
    NodeOptions {
      enable_rosout_reading,
      ..self
    }
  }

  pub fn declare_parameter(mut self, name: &str, value: ParameterValue) -> NodeOptions {
    self.declared_parameters.push(Parameter {
      name: name.to_owned(),
      value,
    });
    // TODO: check for duplicate parameter names
    self
  }

  /// Whether parameters that were not declared may be set, both locally and
  /// through the parameter services. Default `false`.
  ///
  /// When `true`, parameters may also change type (dynamic typing).
  pub fn allow_undeclared_parameters(self, allow_undeclared_parameters: bool) -> NodeOptions {
    NodeOptions {
      allow_undeclared_parameters,
      ..self
    }
  }

  pub fn parameter_validator(mut self, validator: Box<ParameterFunc>) -> NodeOptions {
    self.parameter_validator = Some(validator);
    self
  }

  pub fn parameter_set_action(mut self, action: Box<ParameterFunc>) -> NodeOptions {
    self.parameter_set_action = Some(action);
    self
  }
}

impl Default for NodeOptions {
  fn default() -> Self {
    Self::new()
  }
}

//! Rust-like representation of ROS2 Parameters
//!
//! Parameters are key-value paris that can be set in application code, on the
//! command line (not yet implemented), from environment variables (not
//! implemented), or remotely.
//!
//! Paramters can be queried and set remotely using e.g. the `ros2 param` or
//! `rqt` tools from ROS 2. This only works for [`Node`](crate::Node)s that have
//! enabled Parameter Services and are running a `Spinner`.

/// Named parameter
#[derive(Debug, Clone)]
pub struct Parameter {
  pub name: String,
  pub value: ParameterValue,
}

/// Rust-like representation of ROS2
/// [ParameterValue](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/ParameterValue.msg)
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ParameterValue {
  NotSet,
  Boolean(bool),
  Integer(i64),
  Double(f64),
  String(String),
  ByteArray(Vec<u8>),
  BooleanArray(Vec<bool>),
  IntegerArray(Vec<i64>),
  DoubleArray(Vec<f64>),
  StringArray(Vec<String>),
}

/// List of Parameter types supported by ROS 2.
/// <https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/msg/ParameterType.msg>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParameterType {
  NotSet = 0,
  Bool = 1,
  Integer = 2,
  Double = 3,
  String = 4,
  ByteArray = 5,
  BoolArray = 6,
  IntegerArray = 7,
  DoubleArray = 8,
  StringArray = 9,
}

impl ParameterValue {
  // https://github.com/ros2/rcl_interfaces/blob/rolling/rcl_interfaces/msg/ParameterType.msg
  pub fn to_parameter_type(&self) -> ParameterType {
    match self {
      ParameterValue::NotSet => ParameterType::NotSet,
      ParameterValue::Boolean(_) => ParameterType::Bool,
      ParameterValue::Integer(_) => ParameterType::Integer,
      ParameterValue::Double(_d) => ParameterType::Double,
      ParameterValue::String(_s) => ParameterType::String,
      ParameterValue::ByteArray(_a) => ParameterType::ByteArray,
      ParameterValue::BooleanArray(_a) => ParameterType::BoolArray,
      ParameterValue::IntegerArray(_a) => ParameterType::IntegerArray,
      ParameterValue::DoubleArray(_a) => ParameterType::DoubleArray,
      ParameterValue::StringArray(_a) => ParameterType::StringArray,
    }
  }

  pub fn to_parameter_type_raw(p: &ParameterValue) -> u8 {
    Self::to_parameter_type(p) as u8
  }
}

impl From<raw::Parameter> for Parameter {
  fn from(rp: raw::Parameter) -> Self {
    Parameter {
      name: rp.name,
      value: rp.value.into(),
    }
  }
}

impl From<raw::ParameterValue> for ParameterValue {
  fn from(rpv: raw::ParameterValue) -> ParameterValue {
    match rpv.ptype {
      raw::ParameterType::NOT_SET => ParameterValue::NotSet,
      raw::ParameterType::BOOL => ParameterValue::Boolean(rpv.boolean_value),
      raw::ParameterType::INTEGER => ParameterValue::Integer(rpv.int_value),
      raw::ParameterType::DOUBLE => ParameterValue::Double(rpv.double_value),
      raw::ParameterType::STRING => ParameterValue::String(rpv.string_value),

      raw::ParameterType::BYTE_ARRAY => ParameterValue::ByteArray(rpv.byte_array),
      raw::ParameterType::BOOL_ARRAY => ParameterValue::BooleanArray(rpv.bool_array),
      raw::ParameterType::INTEGER_ARRAY => ParameterValue::IntegerArray(rpv.int_array),
      raw::ParameterType::DOUBLE_ARRAY => ParameterValue::DoubleArray(rpv.double_array),
      raw::ParameterType::STRING_ARRAY => ParameterValue::StringArray(rpv.string_array),

      _ =>
      // This may be an unspecified case.
      // TODO: Do something better, at least log a warning.
      {
        ParameterValue::NotSet
      }
    }
  }
}

impl From<Parameter> for raw::Parameter {
  fn from(p: Parameter) -> raw::Parameter {
    raw::Parameter {
      name: p.name,
      value: p.value.into(),
    }
  }
}

impl From<ParameterValue> for raw::ParameterValue {
  fn from(p: ParameterValue) -> raw::ParameterValue {
    let mut value = raw::ParameterValue {
      ptype: raw::ParameterType::NOT_SET,
      boolean_value: false,
      int_value: 0,
      double_value: 0.0,
      string_value: String::new(),
      byte_array: Vec::new(),
      int_array: Vec::new(),
      bool_array: Vec::new(),
      double_array: Vec::new(),
      string_array: Vec::new(),
    };
    match p {
      ParameterValue::NotSet => (), // already there
      ParameterValue::Boolean(b) => {
        value.ptype = raw::ParameterType::BOOL;
        value.boolean_value = b;
      }
      ParameterValue::Integer(i) => {
        value.ptype = raw::ParameterType::INTEGER;
        value.int_value = i;
      }
      ParameterValue::Double(d) => {
        value.ptype = raw::ParameterType::DOUBLE;
        value.double_value = d;
      }
      ParameterValue::String(s) => {
        value.ptype = raw::ParameterType::STRING;
        value.string_value = s;
      }
      ParameterValue::ByteArray(a) => {
        value.ptype = raw::ParameterType::BYTE_ARRAY;
        value.byte_array = a;
      }
      ParameterValue::BooleanArray(a) => {
        value.ptype = raw::ParameterType::BOOL_ARRAY;
        value.bool_array = a;
      }
      ParameterValue::IntegerArray(a) => {
        value.ptype = raw::ParameterType::INTEGER_ARRAY;
        value.int_array = a;
      }
      ParameterValue::DoubleArray(a) => {
        value.ptype = raw::ParameterType::DOUBLE_ARRAY;
        value.double_array = a;
      }
      ParameterValue::StringArray(a) => {
        value.ptype = raw::ParameterType::STRING_ARRAY;
        value.string_array = a;
      }
    }
    value
  }
} // impl From

/// Result from attempt to set a Parameter value (remotely).
pub type SetParametersResult = Result<(), String>;

impl From<SetParametersResult> for raw::SetParametersResult {
  fn from(s: SetParametersResult) -> raw::SetParametersResult {
    match s {
      Ok(_) => raw::SetParametersResult {
        successful: true,
        reason: "".to_string(),
      },
      Err(reason) => raw::SetParametersResult {
        successful: false,
        reason,
      },
    }
  }
}

/// Documentation and constraints for a [`Parameter`]
pub struct ParameterDescriptor {
  pub name: String,
  pub param_type: ParameterType, // ParameterType.msg defines enum
  pub description: String,       /* Description of the parameter, visible from
                                  * introspection tools. */
  pub additional_constraints: String, /* Plain English description of additional constraints
                                       * which cannot be expressed.. */
  pub read_only: bool, // If 'true' then the value cannot change after it has been initialized.
  pub dynamic_typing: bool, // If true, the parameter is allowed to change type.
  pub range: NumericRange,
}

impl ParameterDescriptor {
  pub fn unknown(name: &str) -> Self {
    ParameterDescriptor {
      name: name.to_string(),
      param_type: ParameterType::NotSet,
      description: "unknown parameter".to_string(),
      additional_constraints: "".to_string(),
      read_only: true,
      dynamic_typing: false,
      range: NumericRange::NotSpecified,
    }
  }

  pub fn from_value(name: &str, value: &ParameterValue) -> Self {
    ParameterDescriptor {
      name: name.to_string(),
      param_type: value.to_parameter_type(),
      description: "(description missing, not implemented)".to_string(),
      additional_constraints: "".to_string(),
      read_only: false,
      dynamic_typing: false,
      range: NumericRange::NotSpecified,
    }
  }
}

/// Optional Limits for a numeric [`Parameter`]
pub enum NumericRange {
  NotSpecified,
  IntegerRange {
    from_value: i64,
    to_value: i64,
    step: i64,
  },
  FloatingPointRange {
    from_value: f64,
    to_value: f64,
    step: f64,
  },
}

impl From<ParameterDescriptor> for raw::ParameterDescriptor {
  fn from(p: ParameterDescriptor) -> raw::ParameterDescriptor {
    let (integer_range, floating_point_range) = match p.range {
      NumericRange::NotSpecified => (vec![], vec![]),

      NumericRange::IntegerRange {
        from_value,
        to_value,
        step,
      } => (
        vec![raw::IntegerRange {
          from_value,
          to_value,
          step,
        }],
        vec![],
      ),

      NumericRange::FloatingPointRange {
        from_value,
        to_value,
        step,
      } => (
        vec![],
        vec![raw::FloatingPointRange {
          from_value,
          to_value,
          step,
        }],
      ),
    };

    raw::ParameterDescriptor {
      name: p.name,
      r#type: p.param_type as u8,
      description: p.description,
      additional_constraints: p.additional_constraints,
      read_only: p.read_only,
      dynamic_typing: p.dynamic_typing,
      integer_range,
      floating_point_range,
    }
  }
}

/// Raw, ROS2-compatible Parameters for sending over the wire.
/// Not for use in a Rust application.
pub mod raw {
  use rustdds::*;
  use serde::{Deserialize, Serialize};

  /// ROS2 [ParameterEvent](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/ParameterEvent.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ParameterEvent {
    pub timestamp: Timestamp,
    // fully qualified path
    pub node: String,
    pub new_parameters: Vec<Parameter>,
    pub changed_parameters: Vec<Parameter>,
    pub deleted_parameters: Vec<Parameter>,
  }

  /// [Parameter](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/Parameter.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Parameter {
    pub name: String,
    pub value: ParameterValue,
  }

  /// [ParameterValue](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/ParameterValue.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ParameterValue {
    pub ptype: u8,
    pub boolean_value: bool,
    pub int_value: i64,
    pub double_value: f64,
    pub string_value: String,
    pub byte_array: Vec<u8>,
    pub bool_array: Vec<bool>,
    pub int_array: Vec<i64>,
    pub double_array: Vec<f64>,
    pub string_array: Vec<String>,
  }

  /// ROS2 defines this as an empty .msg
  /// [ParameterType](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/ParameterType.msg)
  pub struct ParameterType {}

  impl ParameterType {
    pub const NOT_SET: u8 = 0;

    pub const BOOL: u8 = 1;
    pub const INTEGER: u8 = 2;
    pub const DOUBLE: u8 = 3;
    pub const STRING: u8 = 4;
    pub const BYTE_ARRAY: u8 = 5;
    pub const BOOL_ARRAY: u8 = 6;
    pub const INTEGER_ARRAY: u8 = 7;
    pub const DOUBLE_ARRAY: u8 = 8;
    pub const STRING_ARRAY: u8 = 9;
  }

  /// [SetParameersResult](https://github.com/ros2/rcl_interfaces/blob/rolling/rcl_interfaces/msg/SetParametersResult.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct SetParametersResult {
    pub successful: bool,
    pub reason: String,
  }

  /// [ParameterDescriptor](https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/msg/ParameterDescriptor.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ParameterDescriptor {
    pub name: String,
    pub r#type: u8, // ParameterType.msg defines enum
    pub description: String, /* Description of the parameter, visible from
                     * introspection tools. */
    pub additional_constraints: String, /* Plain English description of additional constraints
                                         * which cannot be expressed.. */
    pub read_only: bool, // If 'true' then the value cannot change after it has been initialized.
    pub dynamic_typing: bool, // If true, the parameter is allowed to change type.
    pub floating_point_range: Vec<FloatingPointRange>,
    pub integer_range: Vec<IntegerRange>,
  }

  /// [IntegerRange](https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/msg/IntegerRange.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct IntegerRange {
    pub from_value: i64,
    pub to_value: i64,
    pub step: i64,
  }

  /// [FloatingPointRange](https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/msg/FloatingPointRange.msg)
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct FloatingPointRange {
    pub from_value: f64,
    pub to_value: f64,
    pub step: f64,
  }
}

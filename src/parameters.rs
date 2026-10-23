//! Rust-like representation of ROS2 Parameters
//!
//! Parameters are key-value paris that can be set in application code, on the
//! command line (not yet implemented), from environment variables (not
//! implemented), or remotely.
//!
//! Paramters can be queried and set remotely using e.g. the `ros2 param` or
//! `rqt` tools from ROS 2. This only works for [`Node`](crate::Node)s that have
//! enabled Parameter Services and are running a `Spinner`.

use std::{
  collections::BTreeMap,
  sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
  },
};

use crate::{node_options::ParameterFunc, rcl_interfaces::ListParametersResult};

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

/// The parameter changes made by one set or undeclare operation, as reported
/// in a `ParameterEvent`.
#[derive(Debug, Default)]
pub(crate) struct ParameterChanges {
  pub new_parameters: Vec<raw::Parameter>,
  pub changed_parameters: Vec<raw::Parameter>,
  pub deleted_parameters: Vec<raw::Parameter>,
}

impl ParameterChanges {
  pub fn is_empty(&self) -> bool {
    self.new_parameters.is_empty()
      && self.changed_parameters.is_empty()
      && self.deleted_parameters.is_empty()
  }

  /// A `ParameterEvent` reporting these changes.
  pub fn into_event(
    self,
    stamp: crate::builtin_interfaces::Time,
    node: String,
  ) -> raw::ParameterEvent {
    raw::ParameterEvent {
      stamp,
      node,
      new_parameters: self.new_parameters,
      changed_parameters: self.changed_parameters,
      deleted_parameters: self.deleted_parameters,
    }
  }
}

/// A Node's parameter values and the rules for changing them, shared by both
/// middleware backends and by local and remote (service) access.
///
/// Rules applied to every set:
/// - A parameter that does not exist may only be set if undeclared parameters
///   are allowed.
/// - `use_sim_time` must be a Boolean.
/// - A parameter may not change its type, unless undeclared parameters are
///   allowed (treated as dynamic typing).
/// - The user validator, if any, must accept the value.
///
/// Applying a value runs the user set action, if any, and then stores it.
/// Setting a parameter to `NotSet` deletes it.
///
/// The store never publishes anything; set operations return the
/// [`ParameterChanges`] the caller should publish as a `ParameterEvent`.
pub(crate) struct ParameterStore {
  values: Mutex<BTreeMap<String, ParameterValue>>,
  allow_undeclared: bool,
  // Not held at the same time as `values`, as the callbacks may be slow.
  validator: Option<Mutex<Box<ParameterFunc>>>,
  set_action: Option<Mutex<Box<ParameterFunc>>>,
  use_sim_time: AtomicBool,
}

impl ParameterStore {
  /// Built-in parameter present on every node.
  pub const USE_SIM_TIME: &'static str = "use_sim_time";

  /// Create a store holding `declared` plus `use_sim_time` (default `false`).
  ///
  /// Each declared value is validated and its set action run; the first
  /// failure is returned.
  pub fn new(
    declared: impl IntoIterator<Item = Parameter>,
    allow_undeclared: bool,
    validator: Option<Box<ParameterFunc>>,
    set_action: Option<Box<ParameterFunc>>,
  ) -> Result<Self, String> {
    // If a name is declared more than once, the last value wins.
    let mut values: BTreeMap<String, ParameterValue> = declared
      .into_iter()
      .map(|Parameter { name, value }| (name, value))
      .collect();
    values
      .entry(Self::USE_SIM_TIME.to_string())
      .or_insert(ParameterValue::Boolean(false));
    let initial: Vec<(String, ParameterValue)> =
      values.iter().map(|(n, v)| (n.clone(), v.clone())).collect();

    let store = ParameterStore {
      values: Mutex::new(values),
      allow_undeclared,
      validator: validator.map(Mutex::new),
      set_action: set_action.map(Mutex::new),
      use_sim_time: AtomicBool::new(false),
    };
    for (name, value) in &initial {
      store.check_value(name, value)?;
      store.run_set_action(name, value)?;
    }
    Ok(store)
  }

  pub fn allow_undeclared(&self) -> bool {
    self.allow_undeclared
  }

  /// Current value of the built-in `use_sim_time` parameter.
  #[cfg(any(feature = "dds", test))]
  pub fn use_sim_time(&self) -> bool {
    self.use_sim_time.load(Ordering::SeqCst)
  }

  pub fn get(&self, name: &str) -> Option<ParameterValue> {
    self.values.lock().unwrap().get(name).cloned()
  }

  /// Values for `names`, with `NotSet` for unknown names.
  pub fn get_many(&self, names: &[String]) -> Vec<ParameterValue> {
    let values = self.values.lock().unwrap();
    names
      .iter()
      .map(|name| values.get(name).cloned().unwrap_or(ParameterValue::NotSet))
      .collect()
  }

  /// `ParameterType` codes for `names`, with `NOT_SET` for unknown names.
  pub fn get_types(&self, names: &[String]) -> Vec<u8> {
    self
      .get_many(names)
      .iter()
      .map(ParameterValue::to_parameter_type_raw)
      .collect()
  }

  pub fn describe(&self, names: &[String]) -> Vec<ParameterDescriptor> {
    let values = self.values.lock().unwrap();
    names
      .iter()
      .map(|name| match values.get(name) {
        Some(value) => ParameterDescriptor::from_value(name, value),
        None => ParameterDescriptor::unknown(name),
      })
      .collect()
  }

  pub fn has(&self, name: &str) -> bool {
    self.values.lock().unwrap().contains_key(name)
  }

  pub fn names(&self) -> Vec<String> {
    self.values.lock().unwrap().keys().cloned().collect()
  }

  /// Answer a `ListParameters` request, with the same semantics as rclcpp.
  ///
  /// With no `prefixes`, lists names with fewer than `depth` `.` separators
  /// (`depth` 0 means unlimited). Otherwise lists names that equal a prefix,
  /// or lie under `prefix.` within `depth` further levels. The returned
  /// `prefixes` are the distinct namespaces (up to the last `.`) of the
  /// listed names.
  pub fn list(&self, prefixes: &[String], depth: u64) -> ListParametersResult {
    let within_depth = |s: &str| depth == 0 || (s.matches('.').count() as u64) < depth;
    let mut result = ListParametersResult {
      names: vec![],
      prefixes: vec![],
    };
    for name in self.values.lock().unwrap().keys() {
      let get_all = prefixes.is_empty() && within_depth(name);
      let prefix_matches = prefixes.iter().any(|prefix| {
        name == prefix
          || name
            .strip_prefix(prefix.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|rest| within_depth(&format!(".{rest}")))
      });
      if get_all || prefix_matches {
        result.names.push(name.clone());
        if let Some((namespace, _)) = name.rsplit_once('.')
          && !result.prefixes.iter().any(|p| p == namespace)
        {
          result.prefixes.push(namespace.to_string());
        }
      }
    }
    result
  }

  /// Set one parameter, subject to the rules.
  pub fn set(&self, name: &str, value: ParameterValue) -> Result<ParameterChanges, String> {
    self.validate(name, &value)?;
    let mut changes = ParameterChanges::default();
    self.apply(name.to_string(), value, &mut changes)?;
    Ok(changes)
  }

  /// Set several parameters as one all-or-nothing transaction.
  ///
  /// Every parameter is validated before any is applied. A set action that
  /// fails while applying cannot be rolled back, so set actions should not
  /// fail for values the validator has accepted.
  pub fn set_atomically(&self, params: Vec<Parameter>) -> Result<ParameterChanges, String> {
    for Parameter { name, value } in &params {
      self.validate(name, value)?;
    }
    let mut changes = ParameterChanges::default();
    for Parameter { name, value } in params {
      self.apply(name, value, &mut changes)?;
    }
    Ok(changes)
  }

  /// Remove a parameter, bypassing the rules (local API).
  pub fn undeclare(&self, name: &str) -> ParameterChanges {
    let mut changes = ParameterChanges::default();
    if let Some(value) = self.values.lock().unwrap().remove(name) {
      changes.deleted_parameters.push(raw::Parameter {
        name: name.to_string(),
        value: value.into(),
      });
    }
    changes
  }

  fn validate(&self, name: &str, value: &ParameterValue) -> SetParametersResult {
    let existing = self.get(name);
    if existing.is_none() && !self.allow_undeclared {
      return Err(format!(
        "Setting undeclared parameter '{name}' is not allowed."
      ));
    }
    if !self.allow_undeclared
      && !matches!(value, ParameterValue::NotSet)
      && let Some(existing) = &existing
      && !matches!(existing, ParameterValue::NotSet)
      && std::mem::discriminant(existing) != std::mem::discriminant(value)
    {
      return Err(format!(
        "Cannot change type of parameter '{name}' from {:?} to {:?}.",
        existing.to_parameter_type(),
        value.to_parameter_type()
      ));
    }
    self.check_value(name, value)
  }

  /// The built-in `use_sim_time` check, or else the user validator.
  fn check_value(&self, name: &str, value: &ParameterValue) -> SetParametersResult {
    if name == Self::USE_SIM_TIME {
      return match value {
        ParameterValue::Boolean(_) => Ok(()),
        _ => Err(format!("Parameter '{name}' must be Boolean.")),
      };
    }
    match &self.validator {
      Some(v) => v.lock().unwrap()(name, value),
      None => Ok(()),
    }
  }

  fn run_set_action(&self, name: &str, value: &ParameterValue) -> SetParametersResult {
    if name == Self::USE_SIM_TIME {
      if let ParameterValue::Boolean(b) = value {
        self.use_sim_time.store(*b, Ordering::SeqCst);
      }
      return Ok(());
    }
    match &self.set_action {
      Some(a) => a.lock().unwrap()(name, value),
      None => Ok(()),
    }
  }

  fn apply(
    &self,
    name: String,
    value: ParameterValue,
    changes: &mut ParameterChanges,
  ) -> SetParametersResult {
    self.run_set_action(&name, &value)?;
    let raw_p = raw::Parameter {
      name: name.clone(),
      value: value.clone().into(),
    };
    let mut values = self.values.lock().unwrap();
    match value {
      ParameterValue::NotSet => {
        if values.remove(&name).is_some() {
          changes.deleted_parameters.push(raw_p);
        }
      }
      _ => {
        if values.insert(name, value).is_some() {
          changes.changed_parameters.push(raw_p);
        } else {
          changes.new_parameters.push(raw_p);
        }
      }
    }
    Ok(())
  }
}

/// Raw, ROS2-compatible Parameters for sending over the wire.
/// Not for use in a Rust application.
pub mod raw {
  use serde::{Deserialize, Serialize};

  use crate::builtin_interfaces::Time;

  /// ROS2 [ParameterEvent](https://github.com/ros2/rcl_interfaces/blob/master/rcl_interfaces/msg/ParameterEvent.msg)
  ///
  /// Field layout matches the IDL (`stamp` is [`Time`]). Shared by DDS and
  /// Zenoh backends.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ParameterEvent {
    pub stamp: Time,
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

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::{Parameter, ParameterStore, ParameterValue};

  fn param(name: &str, value: ParameterValue) -> Parameter {
    Parameter {
      name: name.to_string(),
      value,
    }
  }

  fn store(allow_undeclared: bool) -> ParameterStore {
    ParameterStore::new(
      vec![param("p", ParameterValue::Integer(1))],
      allow_undeclared,
      None,
      None,
    )
    .unwrap()
  }

  #[test]
  fn use_sim_time_is_builtin_and_boolean() {
    let s = store(false);
    assert_eq!(s.get("use_sim_time"), Some(ParameterValue::Boolean(false)));
    assert!(s.set("use_sim_time", ParameterValue::Integer(1)).is_err());
    let changes = s
      .set("use_sim_time", ParameterValue::Boolean(true))
      .unwrap();
    assert!(s.use_sim_time());
    // It existed already, so this is a change, not a new parameter.
    assert_eq!(changes.changed_parameters.len(), 1);
    assert!(changes.new_parameters.is_empty());
  }

  #[test]
  fn declared_use_sim_time_is_kept() {
    let s = ParameterStore::new(
      vec![param("use_sim_time", ParameterValue::Boolean(true))],
      false,
      None,
      None,
    )
    .unwrap();
    assert!(s.use_sim_time());
  }

  #[test]
  fn undeclared_rule() {
    assert!(store(false).set("q", ParameterValue::Integer(1)).is_err());
    let changes = store(true).set("q", ParameterValue::Integer(1)).unwrap();
    assert_eq!(changes.new_parameters.len(), 1);
  }

  #[test]
  fn type_change_rule() {
    let s = store(false);
    assert!(s.set("p", ParameterValue::Integer(2)).is_ok());
    assert!(s.set("p", ParameterValue::Double(2.0)).is_err());
    assert_eq!(s.get("p"), Some(ParameterValue::Integer(2)));
    // Undeclared parameters allowed means dynamic typing.
    assert!(store(true).set("p", ParameterValue::Double(2.0)).is_ok());
  }

  #[test]
  fn not_set_deletes() {
    let s = store(false);
    let changes = s.set("p", ParameterValue::NotSet).unwrap();
    assert_eq!(changes.deleted_parameters.len(), 1);
    assert!(!s.has("p"));
  }

  #[test]
  fn atomic_set_is_all_or_nothing() {
    let s = store(false);
    let bad = vec![
      param("p", ParameterValue::Integer(5)),
      param("p2", ParameterValue::Integer(5)), // undeclared
    ];
    assert!(s.set_atomically(bad).is_err());
    assert_eq!(s.get("p"), Some(ParameterValue::Integer(1)));

    let good = vec![
      param("p", ParameterValue::Integer(5)),
      param("use_sim_time", ParameterValue::Boolean(true)),
    ];
    let changes = s.set_atomically(good).unwrap();
    assert_eq!(changes.changed_parameters.len(), 2);
    assert_eq!(s.get("p"), Some(ParameterValue::Integer(5)));
  }

  #[test]
  fn validator_and_set_action() {
    let applied = Arc::new(Mutex::new(Vec::new()));
    let applied_in_action = Arc::clone(&applied);
    let s = ParameterStore::new(
      vec![param("rate", ParameterValue::Integer(1))],
      false,
      Some(Box::new(|_name, value| match value {
        ParameterValue::Integer(i) if *i <= 0 => Err("must be positive".to_string()),
        _ => Ok(()),
      })),
      Some(Box::new(move |name, _value| {
        applied_in_action.lock().unwrap().push(name.to_string());
        Ok(())
      })),
    )
    .unwrap();
    assert!(s.set("rate", ParameterValue::Integer(0)).is_err());
    assert!(s.set("rate", ParameterValue::Integer(10)).is_ok());
    // Once for the declared value, once for the accepted set.
    assert_eq!(*applied.lock().unwrap(), vec!["rate", "rate"]);
  }

  #[test]
  fn declared_values_are_validated() {
    let result = ParameterStore::new(
      vec![param("rate", ParameterValue::Integer(0))],
      false,
      Some(Box::new(|_name, _value| Err("no".to_string()))),
      None,
    );
    assert!(result.is_err());
  }

  #[test]
  fn list_matches_rclcpp() {
    let s = ParameterStore::new(
      ["a", "a.b", "a.b.c", "ab", "x.y"]
        .iter()
        .map(|n| param(n, ParameterValue::Integer(0))),
      false,
      None,
      None,
    )
    .unwrap();
    let names = |prefixes: &[&str], depth| {
      let prefixes: Vec<String> = prefixes.iter().map(|p| p.to_string()).collect();
      s.list(&prefixes, depth)
    };

    let all = names(&[], 0);
    assert_eq!(
      all.names,
      ["a", "a.b", "a.b.c", "ab", "use_sim_time", "x.y"]
    );
    assert_eq!(all.prefixes, ["a", "a.b", "x"]);

    // depth 1: only names without a separator.
    assert_eq!(names(&[], 1).names, ["a", "ab", "use_sim_time"]);

    // A prefix matches itself and names under `prefix.`, not "ab".
    assert_eq!(names(&["a"], 0).names, ["a", "a.b", "a.b.c"]);
    // The separator after the prefix counts towards depth.
    assert_eq!(names(&["a"], 1).names, ["a"]);
    assert_eq!(names(&["a"], 2).names, ["a", "a.b"]);
  }
}

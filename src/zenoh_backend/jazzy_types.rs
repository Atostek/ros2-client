//! Jazzy type descriptions for the Zenoh send-direction hash table.
//!
//! Each description follows the Jazzy `.msg` / `.srv` / `.action` IDL
//! (field names and order, constants omitted). Service hashes include the
//! synthetic `<name>_Event` and Jazzy `ServiceEventInfo`. The resulting
//! `RIHS01` strings are what [`super::type_hash::known_type_hash`] returns,
//! and match `type_hashes` in the installed Jazzy `share/**/*.json` files.

use super::type_description::{
  Field, FieldType, IndividualTypeDescription, TypeDescription, service_type_description,
  time_description, type_id as t,
};

fn field(name: &str, field_type: FieldType) -> Field {
  Field::new(name, field_type)
}

fn scalar(name: &str, type_id: u8) -> Field {
  field(name, FieldType::scalar(type_id))
}

fn msg(name: &str, fields: Vec<Field>) -> IndividualTypeDescription {
  IndividualTypeDescription::new(name, fields)
}

fn nested(name: &str, type_name: &str) -> Field {
  field(name, FieldType::nested(type_name))
}

/// A service `TypeDescription`, plus `Time` and `ServiceEventInfo` in the
/// closure. `extra` is the other nested messages the request/response use.
fn service(
  name: &str,
  request_fields: Vec<Field>,
  response_fields: Vec<Field>,
  extra: Vec<IndividualTypeDescription>,
) -> TypeDescription {
  let request = msg(&format!("{name}_Request"), request_fields);
  let response = msg(&format!("{name}_Response"), response_fields);
  let event = msg(
    &format!("{name}_Event"),
    vec![
      nested("info", "service_msgs/msg/ServiceEventInfo"),
      field(
        "request",
        FieldType::nested_bounded_sequence(format!("{name}_Request"), 1),
      ),
      field(
        "response",
        FieldType::nested_bounded_sequence(format!("{name}_Response"), 1),
      ),
    ],
  );
  let mut referenced = vec![
    time_description(),
    super::type_description::service_event_info(),
  ];
  referenced.extend(extra);
  service_type_description(name, request, response, event, referenced)
}

fn uuid() -> IndividualTypeDescription {
  msg(
    "unique_identifier_msgs/msg/UUID",
    vec![field("uuid", FieldType::array(t::UINT8, 16))],
  )
}

fn goal_info() -> IndividualTypeDescription {
  msg(
    "action_msgs/msg/GoalInfo",
    vec![
      nested("goal_id", "unique_identifier_msgs/msg/UUID"),
      nested("stamp", "builtin_interfaces/msg/Time"),
    ],
  )
}

fn goal_status() -> IndividualTypeDescription {
  msg(
    "action_msgs/msg/GoalStatus",
    vec![
      nested("goal_info", "action_msgs/msg/GoalInfo"),
      scalar("status", t::INT8),
    ],
  )
}

fn parameter_value() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/ParameterValue",
    vec![
      scalar("type", t::UINT8),
      scalar("bool_value", t::BOOLEAN),
      scalar("integer_value", t::INT64),
      scalar("double_value", t::DOUBLE),
      scalar("string_value", t::STRING),
      field("byte_array_value", FieldType::unbounded_sequence(t::BYTE)),
      field(
        "bool_array_value",
        FieldType::unbounded_sequence(t::BOOLEAN),
      ),
      field(
        "integer_array_value",
        FieldType::unbounded_sequence(t::INT64),
      ),
      field(
        "double_array_value",
        FieldType::unbounded_sequence(t::DOUBLE),
      ),
      field(
        "string_array_value",
        FieldType::unbounded_sequence(t::STRING),
      ),
    ],
  )
}

fn parameter() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/Parameter",
    vec![
      scalar("name", t::STRING),
      nested("value", "rcl_interfaces/msg/ParameterValue"),
    ],
  )
}

fn set_parameters_result() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/SetParametersResult",
    vec![
      scalar("successful", t::BOOLEAN),
      scalar("reason", t::STRING),
    ],
  )
}

fn list_parameters_result() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/ListParametersResult",
    vec![
      field("names", FieldType::unbounded_sequence(t::STRING)),
      field("prefixes", FieldType::unbounded_sequence(t::STRING)),
    ],
  )
}

fn floating_point_range() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/FloatingPointRange",
    vec![
      scalar("from_value", t::DOUBLE),
      scalar("to_value", t::DOUBLE),
      scalar("step", t::DOUBLE),
    ],
  )
}

fn integer_range() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/IntegerRange",
    vec![
      scalar("from_value", t::INT64),
      scalar("to_value", t::INT64),
      scalar("step", t::UINT64),
    ],
  )
}

fn parameter_descriptor() -> IndividualTypeDescription {
  msg(
    "rcl_interfaces/msg/ParameterDescriptor",
    vec![
      scalar("name", t::STRING),
      scalar("type", t::UINT8),
      scalar("description", t::STRING),
      scalar("additional_constraints", t::STRING),
      scalar("read_only", t::BOOLEAN),
      scalar("dynamic_typing", t::BOOLEAN),
      field(
        "floating_point_range",
        FieldType::nested_bounded_sequence("rcl_interfaces/msg/FloatingPointRange", 1),
      ),
      field(
        "integer_range",
        FieldType::nested_bounded_sequence("rcl_interfaces/msg/IntegerRange", 1),
      ),
    ],
  )
}

fn fibonacci_goal() -> IndividualTypeDescription {
  msg(
    "action_tutorials_interfaces/action/Fibonacci_Goal",
    vec![scalar("order", t::INT32)],
  )
}

fn fibonacci_result() -> IndividualTypeDescription {
  msg(
    "action_tutorials_interfaces/action/Fibonacci_Result",
    vec![field("sequence", FieldType::unbounded_sequence(t::INT32))],
  )
}

fn fibonacci_feedback() -> IndividualTypeDescription {
  msg(
    "action_tutorials_interfaces/action/Fibonacci_Feedback",
    vec![field(
      "partial_sequence",
      FieldType::unbounded_sequence(t::INT32),
    )],
  )
}

/// `example_interfaces/srv/AddTwoInts` for Jazzy.
pub(crate) fn add_two_ints() -> TypeDescription {
  service(
    "example_interfaces/srv/AddTwoInts",
    vec![scalar("a", t::INT64), scalar("b", t::INT64)],
    vec![scalar("sum", t::INT64)],
    Vec::new(),
  )
}

pub(crate) fn log_message() -> TypeDescription {
  TypeDescription::new(
    msg(
      "rcl_interfaces/msg/Log",
      vec![
        nested("stamp", "builtin_interfaces/msg/Time"),
        scalar("level", t::UINT8),
        scalar("name", t::STRING),
        scalar("msg", t::STRING),
        scalar("file", t::STRING),
        scalar("function", t::STRING),
        scalar("line", t::UINT32),
      ],
    ),
    vec![time_description()],
  )
}

pub(crate) fn parameter_event() -> TypeDescription {
  TypeDescription::new(
    msg(
      "rcl_interfaces/msg/ParameterEvent",
      vec![
        nested("stamp", "builtin_interfaces/msg/Time"),
        scalar("node", t::STRING),
        field(
          "new_parameters",
          FieldType::nested_unbounded_sequence("rcl_interfaces/msg/Parameter"),
        ),
        field(
          "changed_parameters",
          FieldType::nested_unbounded_sequence("rcl_interfaces/msg/Parameter"),
        ),
        field(
          "deleted_parameters",
          FieldType::nested_unbounded_sequence("rcl_interfaces/msg/Parameter"),
        ),
      ],
    ),
    vec![time_description(), parameter(), parameter_value()],
  )
}

pub(crate) fn goal_status_array() -> TypeDescription {
  TypeDescription::new(
    msg(
      "action_msgs/msg/GoalStatusArray",
      vec![field(
        "status_list",
        FieldType::nested_unbounded_sequence("action_msgs/msg/GoalStatus"),
      )],
    ),
    vec![goal_status(), goal_info(), uuid(), time_description()],
  )
}

pub(crate) fn fibonacci_feedback_message() -> TypeDescription {
  TypeDescription::new(
    msg(
      "action_tutorials_interfaces/action/Fibonacci_FeedbackMessage",
      vec![
        nested("goal_id", "unique_identifier_msgs/msg/UUID"),
        nested(
          "feedback",
          "action_tutorials_interfaces/action/Fibonacci_Feedback",
        ),
      ],
    ),
    vec![uuid(), fibonacci_feedback()],
  )
}

pub(crate) fn cancel_goal() -> TypeDescription {
  service(
    "action_msgs/srv/CancelGoal",
    vec![nested("goal_info", "action_msgs/msg/GoalInfo")],
    vec![
      scalar("return_code", t::INT8),
      field(
        "goals_canceling",
        FieldType::nested_unbounded_sequence("action_msgs/msg/GoalInfo"),
      ),
    ],
    vec![goal_info(), uuid()],
  )
}

pub(crate) fn fibonacci_send_goal() -> TypeDescription {
  service(
    "action_tutorials_interfaces/action/Fibonacci_SendGoal",
    vec![
      nested("goal_id", "unique_identifier_msgs/msg/UUID"),
      nested("goal", "action_tutorials_interfaces/action/Fibonacci_Goal"),
    ],
    vec![
      scalar("accepted", t::BOOLEAN),
      nested("stamp", "builtin_interfaces/msg/Time"),
    ],
    vec![uuid(), fibonacci_goal()],
  )
}

pub(crate) fn fibonacci_get_result() -> TypeDescription {
  service(
    "action_tutorials_interfaces/action/Fibonacci_GetResult",
    vec![nested("goal_id", "unique_identifier_msgs/msg/UUID")],
    vec![
      scalar("status", t::INT8),
      nested(
        "result",
        "action_tutorials_interfaces/action/Fibonacci_Result",
      ),
    ],
    vec![uuid(), fibonacci_result()],
  )
}

pub(crate) fn get_parameters() -> TypeDescription {
  service(
    "rcl_interfaces/srv/GetParameters",
    vec![field("names", FieldType::unbounded_sequence(t::STRING))],
    vec![field(
      "values",
      FieldType::nested_unbounded_sequence("rcl_interfaces/msg/ParameterValue"),
    )],
    vec![parameter_value()],
  )
}

pub(crate) fn get_parameter_types() -> TypeDescription {
  service(
    "rcl_interfaces/srv/GetParameterTypes",
    vec![field("names", FieldType::unbounded_sequence(t::STRING))],
    vec![field("types", FieldType::unbounded_sequence(t::UINT8))],
    Vec::new(),
  )
}

pub(crate) fn set_parameters() -> TypeDescription {
  service(
    "rcl_interfaces/srv/SetParameters",
    vec![field(
      "parameters",
      FieldType::nested_unbounded_sequence("rcl_interfaces/msg/Parameter"),
    )],
    vec![field(
      "results",
      FieldType::nested_unbounded_sequence("rcl_interfaces/msg/SetParametersResult"),
    )],
    vec![parameter(), parameter_value(), set_parameters_result()],
  )
}

pub(crate) fn set_parameters_atomically() -> TypeDescription {
  service(
    "rcl_interfaces/srv/SetParametersAtomically",
    vec![field(
      "parameters",
      FieldType::nested_unbounded_sequence("rcl_interfaces/msg/Parameter"),
    )],
    vec![nested("result", "rcl_interfaces/msg/SetParametersResult")],
    vec![parameter(), parameter_value(), set_parameters_result()],
  )
}

pub(crate) fn list_parameters() -> TypeDescription {
  service(
    "rcl_interfaces/srv/ListParameters",
    vec![
      field("prefixes", FieldType::unbounded_sequence(t::STRING)),
      scalar("depth", t::UINT64),
    ],
    vec![nested("result", "rcl_interfaces/msg/ListParametersResult")],
    vec![list_parameters_result()],
  )
}

pub(crate) fn describe_parameters() -> TypeDescription {
  service(
    "rcl_interfaces/srv/DescribeParameters",
    vec![field("names", FieldType::unbounded_sequence(t::STRING))],
    vec![field(
      "descriptors",
      FieldType::nested_unbounded_sequence("rcl_interfaces/msg/ParameterDescriptor"),
    )],
    vec![
      parameter_descriptor(),
      floating_point_range(),
      integer_range(),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::interop_descriptions;

  /// Locked RIHS01 values for the Jazzy IDL in this module. A field-list
  /// change must update both the description and this table.
  const LOCKED: &[(&str, &str)] = &[
    (
      "example_interfaces::srv::dds_::AddTwoInts_",
      "RIHS01_e118de6bf5eeb66a2491b5bda11202e7b68f198d6f67922cf30364858239c81a",
    ),
    (
      "rcl_interfaces::msg::dds_::Log_",
      "RIHS01_e28ce254ca8abc06abf92773b74602cdbf116ed34fbaf294fb9f81da9f318eac",
    ),
    (
      "rcl_interfaces::msg::dds_::ParameterEvent_",
      "RIHS01_043e627780fcad87a22d225bc2a037361dba713fca6a6b9f4b869a5aa0393204",
    ),
    (
      "action_msgs::msg::dds_::GoalStatusArray_",
      "RIHS01_6c1684b00f177d37438febe6e709fc4e2b0d4248dca4854946f9ed8b30cda83e",
    ),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_FeedbackMessage_",
      "RIHS01_50fc26b9cac313652ecbeab3adf9b5414d59fd4d4d5f9058ddcc7525169927f1",
    ),
    (
      "action_msgs::srv::dds_::CancelGoal_",
      "RIHS01_573d8b0a534451d7bc2ac8c5ffde8ac14b8593b7001175d0cd6516dcbeb8689a",
    ),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_SendGoal_",
      "RIHS01_a0603060ed69fe2dfbd1a6f3b982a1749957ef346e4a4d2b311a05e305ec37bb",
    ),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_GetResult_",
      "RIHS01_8b47e383f1e31f6d8df6417ab54957e7d5ea24dad315646ad711ac3fdea81d58",
    ),
    (
      "rcl_interfaces::srv::dds_::GetParameters_",
      "RIHS01_bf9803d5c74cf989a5de3e0c2e99444599a627c7ff75f97b8c05b01003675cbc",
    ),
    (
      "rcl_interfaces::srv::dds_::GetParameterTypes_",
      "RIHS01_da199c878688b3e530bdfe3ca8f74cb9fa0c303101e980a9e8f260e25e1c80ca",
    ),
    (
      "rcl_interfaces::srv::dds_::SetParameters_",
      "RIHS01_56eed9a67e169f9cb6c1f987bc88f868c14a8fc9f743a263bc734c154015d7e0",
    ),
    (
      "rcl_interfaces::srv::dds_::SetParametersAtomically_",
      "RIHS01_0e192ef259c07fc3c07a13191d27002222e65e00ccec653ca05e856f79285fcd",
    ),
    (
      "rcl_interfaces::srv::dds_::ListParameters_",
      "RIHS01_3e6062bfbb27bfb8730d4cef2558221f51a11646d78e7bb30a1e83afac3aad9d",
    ),
    (
      "rcl_interfaces::srv::dds_::DescribeParameters_",
      "RIHS01_845b484d71eb0673dae682f2e3ba3c4851a65a3dcfb97bddd82c5b57e91e4cff",
    ),
  ];

  #[test]
  fn jazzy_hashes_are_locked() {
    let computed: Vec<_> = interop_descriptions()
      .into_iter()
      .map(|(name, td)| (name, td.rihs01()))
      .collect();
    assert_eq!(computed.len(), LOCKED.len());
    for (got, expected) in computed.iter().zip(LOCKED) {
      assert_eq!(got.0, expected.0);
      assert_eq!(got.1, expected.1, "hash drift for {}", expected.0);
      assert_eq!(
        super::super::type_hash::known_type_hash(expected.0),
        Some(expected.1)
      );
    }
  }
}

/// `(dds type name, description)` for every Jazzy interop type whose hash
/// must be concrete on the send / queryable side.
pub(crate) fn interop_descriptions() -> Vec<(&'static str, TypeDescription)> {
  vec![
    ("example_interfaces::srv::dds_::AddTwoInts_", add_two_ints()),
    ("rcl_interfaces::msg::dds_::Log_", log_message()),
    (
      "rcl_interfaces::msg::dds_::ParameterEvent_",
      parameter_event(),
    ),
    (
      "action_msgs::msg::dds_::GoalStatusArray_",
      goal_status_array(),
    ),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_FeedbackMessage_",
      fibonacci_feedback_message(),
    ),
    ("action_msgs::srv::dds_::CancelGoal_", cancel_goal()),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_SendGoal_",
      fibonacci_send_goal(),
    ),
    (
      "action_tutorials_interfaces::action::dds_::Fibonacci_GetResult_",
      fibonacci_get_result(),
    ),
    (
      "rcl_interfaces::srv::dds_::GetParameters_",
      get_parameters(),
    ),
    (
      "rcl_interfaces::srv::dds_::GetParameterTypes_",
      get_parameter_types(),
    ),
    (
      "rcl_interfaces::srv::dds_::SetParameters_",
      set_parameters(),
    ),
    (
      "rcl_interfaces::srv::dds_::SetParametersAtomically_",
      set_parameters_atomically(),
    ),
    (
      "rcl_interfaces::srv::dds_::ListParameters_",
      list_parameters(),
    ),
    (
      "rcl_interfaces::srv::dds_::DescribeParameters_",
      describe_parameters(),
    ),
  ]
}

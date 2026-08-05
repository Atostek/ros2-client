//! REP-2016 type descriptions and `RIHS01_…` hashing (ADR-0007).
//!
//! A ROS 2 type hash is `RIHS01_` + the lowercase-hex SHA-256 of a canonical
//! JSON serialization of the type's [`TypeDescription`]. This module implements
//! that computation exactly as `rosidl_generator_type_description`'s
//! `calculate_type_hash` does, so a hash computed here matches the one a C++
//! peer embeds in its Zenoh keys.
//!
//! It removes the send-direction limitation of the hard-coded
//! [`super::type_hash`] table *for any type whose full field description is
//! available*: build the [`TypeDescription`] (message, or a service via
//! [`service_type_description`]) and call [`TypeDescription::rihs01`]. Wiring
//! this into `msggen` so generated types carry their computed hash — which
//! needs cross-package IDL resolution and a pinned distro's builtin
//! definitions — is the remaining follow-up (ADR-0007).
//!
//! The module is backend-neutral (no `zenoh` crate) and unit-tested against the
//! published hashes of `std_msgs/msg/String` and
//! `example_interfaces/srv/AddTwoInts`.

use sha2::{Digest, Sha256};

/// `type_description_interfaces/msg/FieldType` `type_id` constants.
///
/// The base (scalar) ids are 1..=22. Array / bounded-sequence /
/// unbounded-sequence variants of a base id `b` are `b + 48`, `b + 96`,
/// `b + 144` respectively.
pub mod type_id {
  /// No type set.
  pub const NOT_SET: u8 = 0;
  /// A nested (named) type.
  pub const NESTED_TYPE: u8 = 1;
  /// `int8`.
  pub const INT8: u8 = 2;
  /// `uint8`.
  pub const UINT8: u8 = 3;
  /// `int16`.
  pub const INT16: u8 = 4;
  /// `uint16`.
  pub const UINT16: u8 = 5;
  /// `int32`.
  pub const INT32: u8 = 6;
  /// `uint32`.
  pub const UINT32: u8 = 7;
  /// `int64`.
  pub const INT64: u8 = 8;
  /// `uint64`.
  pub const UINT64: u8 = 9;
  /// `float`.
  pub const FLOAT: u8 = 10;
  /// `double`.
  pub const DOUBLE: u8 = 11;
  /// `long double`.
  pub const LONG_DOUBLE: u8 = 12;
  /// `char`.
  pub const CHAR: u8 = 13;
  /// `wchar`.
  pub const WCHAR: u8 = 14;
  /// `bool`.
  pub const BOOLEAN: u8 = 15;
  /// `byte`.
  pub const BYTE: u8 = 16;
  /// `string`.
  pub const STRING: u8 = 17;
  /// `wstring`.
  pub const WSTRING: u8 = 18;
  /// `string<=N` fixed (unused by the current rosidl parser).
  pub const FIXED_STRING: u8 = 19;
  /// `wstring<=N` fixed (unused by the current rosidl parser).
  pub const FIXED_WSTRING: u8 = 20;
  /// Bounded `string<=N`.
  pub const BOUNDED_STRING: u8 = 21;
  /// Bounded `wstring<=N`.
  pub const BOUNDED_WSTRING: u8 = 22;

  /// Offset added to a base id for a fixed-size array variant.
  pub const ARRAY_OFFSET: u8 = 48;
  /// Offset added to a base id for a bounded-sequence variant.
  pub const BOUNDED_SEQUENCE_OFFSET: u8 = 96;
  /// Offset added to a base id for an unbounded-sequence variant.
  pub const UNBOUNDED_SEQUENCE_OFFSET: u8 = 144;
}

/// A `type_description_interfaces/msg/FieldType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldType {
  /// The `FIELD_TYPE_*` id (see [`type_id`]).
  pub type_id: u8,
  /// Fixed-array length or bounded-sequence max (else 0).
  pub capacity: u64,
  /// Fixed/bounded (w)string length or max (else 0).
  pub string_capacity: u64,
  /// `/`-joined name of the nested type (else empty).
  pub nested_type_name: String,
}

impl FieldType {
  /// A scalar of the given base [`type_id`].
  pub fn scalar(type_id: u8) -> Self {
    Self {
      type_id,
      capacity: 0,
      string_capacity: 0,
      nested_type_name: String::new(),
    }
  }

  /// A fixed-size array `[capacity]` of the given base scalar id.
  pub fn array(base_type_id: u8, capacity: u64) -> Self {
    Self {
      type_id: base_type_id + type_id::ARRAY_OFFSET,
      capacity,
      string_capacity: 0,
      nested_type_name: String::new(),
    }
  }

  /// A reference to a nested (named) type.
  pub fn nested(name: impl Into<String>) -> Self {
    Self {
      type_id: type_id::NESTED_TYPE,
      capacity: 0,
      string_capacity: 0,
      nested_type_name: name.into(),
    }
  }

  /// A bounded sequence `[<=capacity]` of a nested (named) type.
  pub fn nested_bounded_sequence(name: impl Into<String>, capacity: u64) -> Self {
    Self {
      type_id: type_id::NESTED_TYPE + type_id::BOUNDED_SEQUENCE_OFFSET,
      capacity,
      string_capacity: 0,
      nested_type_name: name.into(),
    }
  }
}

/// A `type_description_interfaces/msg/Field`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
  /// Field name.
  pub name: String,
  /// Field type.
  pub field_type: FieldType,
  /// Literal default value (stripped before hashing; kept for completeness).
  pub default_value: String,
}

impl Field {
  /// A field with no default value.
  pub fn new(name: impl Into<String>, field_type: FieldType) -> Self {
    Self {
      name: name.into(),
      field_type,
      default_value: String::new(),
    }
  }
}

/// A `type_description_interfaces/msg/IndividualTypeDescription`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndividualTypeDescription {
  /// `/`-joined namespaced type name (e.g. `std_msgs/msg/String`).
  pub type_name: String,
  /// Ordered fields.
  pub fields: Vec<Field>,
}

impl IndividualTypeDescription {
  /// New description from a name and fields.
  pub fn new(type_name: impl Into<String>, fields: Vec<Field>) -> Self {
    Self {
      type_name: type_name.into(),
      fields,
    }
  }
}

/// A `type_description_interfaces/msg/TypeDescription`: the top type plus the
/// transitive closure of the nested types it references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDescription {
  /// The type being described.
  pub type_description: IndividualTypeDescription,
  /// The transitive closure of referenced nested types (any order; hashing
  /// sorts them by name).
  pub referenced_type_descriptions: Vec<IndividualTypeDescription>,
}

impl TypeDescription {
  /// New description.
  pub fn new(
    type_description: IndividualTypeDescription,
    referenced_type_descriptions: Vec<IndividualTypeDescription>,
  ) -> Self {
    Self {
      type_description,
      referenced_type_descriptions,
    }
  }

  /// The canonical hashable JSON representation (matches
  /// `calculate_type_hash`'s `json.dumps(..., separators=(', ', ': '))` with
  /// `default_value` stripped and referenced types sorted by name).
  pub fn hashable_json(&self) -> String {
    let mut refs: Vec<&IndividualTypeDescription> =
      self.referenced_type_descriptions.iter().collect();
    refs.sort_by(|a, b| a.type_name.cmp(&b.type_name));

    let mut out = String::new();
    out.push_str("{\"type_description\": ");
    write_itd(&self.type_description, &mut out);
    out.push_str(", \"referenced_type_descriptions\": [");
    for (i, itd) in refs.iter().enumerate() {
      if i > 0 {
        out.push_str(", ");
      }
      write_itd(itd, &mut out);
    }
    out.push_str("]}");
    out
  }

  /// The REP-2016 type hash string, `RIHS01_` + lowercase-hex SHA-256 of
  /// [`hashable_json`](Self::hashable_json).
  pub fn rihs01(&self) -> String {
    let json = self.hashable_json();
    let digest = Sha256::digest(json.as_bytes());
    let mut s = String::with_capacity(71);
    s.push_str("RIHS01_");
    for b in digest {
      s.push_str(&format!("{b:02x}"));
    }
    s
  }
}

/// Compose the `TypeDescription` of a service from its already-built request,
/// response, and event individual descriptions plus their referenced closure,
/// mirroring `rosidl_generator_type_description`'s `add_srv`.
///
/// The synthetic top type is named `<service_name>` (no suffix) with fields
/// `request_message`, `response_message`, `event_message` referencing
/// `<service_name>_Request` / `_Response` / `_Event`. The three individual
/// descriptions and every entry in `referenced` become the referenced closure.
pub fn service_type_description(
  service_name: &str,
  request: IndividualTypeDescription,
  response: IndividualTypeDescription,
  event: IndividualTypeDescription,
  referenced: Vec<IndividualTypeDescription>,
) -> TypeDescription {
  let top = IndividualTypeDescription::new(
    service_name.to_string(),
    vec![
      Field::new(
        "request_message",
        FieldType::nested(format!("{service_name}_Request")),
      ),
      Field::new(
        "response_message",
        FieldType::nested(format!("{service_name}_Response")),
      ),
      Field::new(
        "event_message",
        FieldType::nested(format!("{service_name}_Event")),
      ),
    ],
  );
  let mut refs = vec![request, response, event];
  refs.extend(referenced);
  TypeDescription::new(top, refs)
}

// --- canonical JSON writers ------------------------------------------------

fn write_itd(itd: &IndividualTypeDescription, out: &mut String) {
  out.push_str("{\"type_name\": ");
  write_json_string(&itd.type_name, out);
  out.push_str(", \"fields\": [");
  for (i, f) in itd.fields.iter().enumerate() {
    if i > 0 {
      out.push_str(", ");
    }
    write_field(f, out);
  }
  out.push_str("]}");
}

fn write_field(f: &Field, out: &mut String) {
  // `default_value` is stripped before hashing.
  out.push_str("{\"name\": ");
  write_json_string(&f.name, out);
  out.push_str(", \"type\": ");
  write_field_type(&f.field_type, out);
  out.push('}');
}

fn write_field_type(ft: &FieldType, out: &mut String) {
  out.push_str("{\"type_id\": ");
  out.push_str(&ft.type_id.to_string());
  out.push_str(", \"capacity\": ");
  out.push_str(&ft.capacity.to_string());
  out.push_str(", \"string_capacity\": ");
  out.push_str(&ft.string_capacity.to_string());
  out.push_str(", \"nested_type_name\": ");
  write_json_string(&ft.nested_type_name, out);
  out.push('}');
}

/// Write `s` as a JSON string with `ensure_ascii=True` semantics (matching
/// Python's `json.dumps`): named escapes for the common control chars,
/// `\uXXXX` for other control chars and all non-ASCII (UTF-16 code units).
fn write_json_string(s: &str, out: &mut String) {
  out.push('"');
  for c in s.chars() {
    match c {
      '"' => out.push_str("\\\""),
      '\\' => out.push_str("\\\\"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      '\u{08}' => out.push_str("\\b"),
      '\u{0c}' => out.push_str("\\f"),
      c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
      c if c.is_ascii() => out.push(c),
      c => {
        let mut buf = [0u16; 2];
        for unit in c.encode_utf16(&mut buf) {
          out.push_str(&format!("\\u{unit:04x}"));
        }
      }
    }
  }
  out.push('"');
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::{type_id as t, *};

  #[test]
  fn std_msgs_string_hash_matches() {
    // std_msgs/msg/String: single `string data` field, no referenced types.
    let td = TypeDescription::new(
      IndividualTypeDescription::new(
        "std_msgs/msg/String",
        vec![Field::new("data", FieldType::scalar(t::STRING))],
      ),
      vec![],
    );

    assert_eq!(
      td.hashable_json(),
      "{\"type_description\": {\"type_name\": \"std_msgs/msg/String\", \"fields\": \
       [{\"name\": \"data\", \"type\": {\"type_id\": 17, \"capacity\": 0, \
       \"string_capacity\": 0, \"nested_type_name\": \"\"}}]}, \
       \"referenced_type_descriptions\": []}"
    );
    assert_eq!(
      td.rihs01(),
      "RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18"
    );
    // The pure algorithm reproduces the value baked into the known-types table.
    assert_eq!(
      td.rihs01(),
      super::super::type_hash::known_type_hash("std_msgs::msg::dds_::String_").unwrap()
    );
  }

  #[test]
  fn add_two_ints_service_hash_matches() {
    let svc = "example_interfaces/srv/AddTwoInts";

    let request = IndividualTypeDescription::new(
      format!("{svc}_Request"),
      vec![
        Field::new("a", FieldType::scalar(t::INT64)),
        Field::new("b", FieldType::scalar(t::INT64)),
      ],
    );
    let response = IndividualTypeDescription::new(
      format!("{svc}_Response"),
      vec![Field::new("sum", FieldType::scalar(t::INT64))],
    );
    let event = IndividualTypeDescription::new(
      format!("{svc}_Event"),
      vec![
        Field::new(
          "info",
          FieldType::nested("service_msgs/msg/ServiceEventInfo"),
        ),
        Field::new(
          "request",
          FieldType::nested_bounded_sequence(format!("{svc}_Request"), 1),
        ),
        Field::new(
          "response",
          FieldType::nested_bounded_sequence(format!("{svc}_Response"), 1),
        ),
      ],
    );

    // Transitive closure (order irrelevant — hashing sorts by name).
    let time = IndividualTypeDescription::new(
      "builtin_interfaces/msg/Time",
      vec![
        Field::new("sec", FieldType::scalar(t::INT32)),
        Field::new("nanosec", FieldType::scalar(t::UINT32)),
      ],
    );
    // NOTE: this hash corresponds to the ROS distro where ServiceEventInfo's
    // `client_gid` is `uint8[16]` (type_id 51). Newer `char[16]` distros hash
    // differently.
    let service_event_info = IndividualTypeDescription::new(
      "service_msgs/msg/ServiceEventInfo",
      vec![
        Field::new("event_type", FieldType::scalar(t::UINT8)),
        Field::new("stamp", FieldType::nested("builtin_interfaces/msg/Time")),
        Field::new("client_gid", FieldType::array(t::UINT8, 16)),
        Field::new("sequence_number", FieldType::scalar(t::INT64)),
      ],
    );

    let td = service_type_description(
      svc,
      request.clone(),
      response.clone(),
      event.clone(),
      vec![time, service_event_info],
    );

    assert_eq!(
      td.rihs01(),
      "RIHS01_e118de6bf5eeb66a2491b5bda11202e7b68f198d6f67922cf30364858239c81a"
    );
    assert_eq!(
      td.rihs01(),
      super::super::type_hash::known_type_hash("example_interfaces::srv::dds_::AddTwoInts_")
        .unwrap()
    );
  }

  #[test]
  fn field_type_offsets() {
    assert_eq!(FieldType::array(t::UINT8, 16).type_id, 51);
    assert_eq!(
      FieldType::nested_bounded_sequence("x", 1).type_id,
      t::NESTED_TYPE + t::BOUNDED_SEQUENCE_OFFSET
    );
  }
}

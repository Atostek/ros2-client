use std::io;

use super::parser::{ArraySpecifier, BaseTypeName, Comment, Item, TypeName, Value};

pub fn print_struct_definition<W: io::Write>(
  w: &mut W,
  name: &str,
  lines: &[(Option<Item>, Option<Comment>)],
) -> io::Result<()> {
  // assume that first we have only constants and comments
  let is_not_field = |i: &Item| !matches!(i, Item::Field { .. });

  let not_yet = lines
    .iter()
    .take_while(|p| p.0.as_ref().is_none_or(is_not_field));
  let got_field = lines
    .iter()
    .skip_while(|p| p.0.as_ref().is_none_or(is_not_field));

  let mut got_first_constant = false;
  for (item, comment) in not_yet {
    match (item, comment) {
      (None, None) => writeln!(w)?, // empty line
      (None, Some(Comment(c))) => writeln!(w, "// {c}")?,
      (Some(item), comment_opt) => {
        match item {
          Item::Field { .. } => panic!("Why am i here?"),
          Item::Constant {
            type_name,
            const_name,
            value,
          } => {
            let rust_type = translate_type(type_name)?;
            let rust_value = translate_value(value, &rust_type);
            if !got_first_constant {
              writeln!(w, "impl {name} {{")?;
            }
            got_first_constant = true;
            writeln!(w, "  pub const {const_name}: {rust_type} = {rust_value};")?;
          }
        }

        if let Some(Comment(c)) = comment_opt {
          writeln!(w, "// {c}")?;
        }
      }
    }
  }
  if got_first_constant {
    writeln!(w, "}}")?;
  }

  writeln!(w, "#[derive(Debug, Serialize, Deserialize, Clone)]")?;
  writeln!(w, "pub struct {name} {{")?;
  // We only produce defaults for messages where each field has a default value, otherwise we would
  // need to define constructors instead of just implementing Default.
  // If we encounter any field without a default value, we set `default` to None, and skip all
  // default values.
  let mut default = Some(Vec::new());
  for (item, comment) in got_field {
    match (item, comment) {
      (None, None) => writeln!(w)?, // empty line
      (None, Some(Comment(c))) => writeln!(w, "  // {c}")?,
      (Some(item), comment_opt) => {
        write!(w, "  pub ")?;
        match item {
          Item::Field {
            type_name,
            field_name,
            default_value,
          } => {
            let rust_type = translate_type(type_name)?;
            write!(w, "{} : {}, ", escape_keywords(field_name), rust_type)?;
            if let Some(default) = default.as_mut() {
              if let Some(default_value) = default_value {
                let rust_value = translate_value(default_value, &rust_type);
                default.push(format!("{}: {rust_value}", escape_keywords(field_name)));
              } else {
                if !default.is_empty() {
                  write!(w, "// no default value for field {field_name}, skipping previous defaults")?;
                  default.clear();
                }
              }
            } else if default_value.is_some() {
              write!(
                w,
                "// no default value for a previous field, skipping default value for field {field_name}"
              )?;
            }
          }
          Item::Constant { const_name, .. } => write!(
            w,
            "// skipped constant {const_name} in the middle of struct"
          )?,
        }

        if let Some(Comment(c)) = comment_opt {
          writeln!(w, "// {c}")?;
        } else {
          writeln!(w)?;
        }
      }
    }
  }
  writeln!(w, "}}")?;
  if let Some(default) = default {
    if !default.is_empty() {
      writeln!(w, "impl Default for {name} {{")?;
      writeln!(w, "  fn default() -> Self {{")?;
      writeln!(w, "    Self {{")?;
      for field in default {
        writeln!(w, "      {field},")?;
      }
      writeln!(w, "    }}")?;
      writeln!(w, "  }}")?;
      writeln!(w, "}}")?;
    }
  }
  Ok(())
}

fn escape_keywords(id: &str) -> String {
  match id {
    "type" => {
      let mut s = "r#".to_string();
      s.push_str(id);
      s
    }
    _ => id.to_string(),
  }
}

const RUST_BYTESTRING: &str = "std::string::String";
const RUST_WIDE_STRING: &str = "WString";

fn translate_type(t: &TypeName) -> io::Result<String> {
  let mut base = String::new();
  match t.base {
    BaseTypeName::Primitive { ref name } => base.push_str(match name.as_str() {
      "bool" => "bool",
      "byte" => "u8",
      "char" => "u8",
      "float32" => "f32",
      "float64" => "f64",
      "int8" => "i8",
      "int16" => "i16",
      "int32" => "i32",
      "int64" => "i64",
      "uint8" => "u8",
      "uint16" => "u16",
      "uint32" => "u32",
      "uint64" => "u64",
      "string" => RUST_BYTESTRING,
      "wstring" => RUST_WIDE_STRING,
      other => panic!("Unexpected primitive type {}", other),
    }),
    BaseTypeName::BoundedString { .. } => base.push_str(RUST_BYTESTRING), /* We do not have type */
    // to represent
    // boundedness
    BaseTypeName::ComplexType {
      ref package_name,
      ref type_name,
    } => {
      if let Some(pkg) = package_name {
        base.push_str("super::");
        base.push_str(pkg);
        base.push_str("::");
      }
      base.push_str(type_name);
    }
  }

  match t.array_spec {
    None => {}
    Some(ArraySpecifier::Static { size }) => {
      base = format!("[{base};{size}]");
    }
    Some(ArraySpecifier::Unbounded) | Some(ArraySpecifier::Bounded { .. }) => {
      base = format!("Vec<{base}>");
    }
  }

  Ok(base)
}

fn translate_value(v: &Value, expected_rust_type: &str) -> String {
  let float_cast = if expected_rust_type == "f32" || expected_rust_type == "f64" {
    ".0"
  } else {
    ""
  };

  match v {
    Value::Bool(b) => {
      if *b {
        "true".to_string()
      } else {
        "false".to_string()
      }
    }
    Value::Float(f) => format!("{f}"),
    Value::Int(i) => format!("{i}{float_cast}"),
    Value::Uint(u) => format!("{u}{float_cast}"),
    Value::String(v) => String::from_utf8(v.to_vec()).unwrap(),
  }
}

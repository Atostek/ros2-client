use std::{
  collections::BTreeMap,
  ffi::OsStr,
  fs, io,
  io::Write,
  path::{Path, PathBuf},
};

use clap::{Arg, Command}; // command line argument processing

mod parser;
mod stringparser;
//mod gen;

use parser::{ArraySpecifier, BaseTypeName, Comment, Item, TypeName, Value};

fn main() -> io::Result<()> {
  //println!("msggen");

  let arg_matches = Command::new("msggen")
    .version("0.0.1")
    .author("Juhana Helovuo <juhana.helovuo@atostek.com>")
    .about("ros2-client .msg to Rust compiler for ros2-client / RustDDS")
    .after_help("Example: ./msggen --workspace /opt/ros/jazzy --type turtlesim/Pose --output generated_rust_dir")
    .arg(
      Arg::new("input")
        .short('i')
        .long("input")
        .help("Input .msg file name")
        .value_name("file"),
    )
    .arg(
      Arg::new("type")
        .short('t')
        .long("typename")
        .help("ROS 2 type to be translated. Can be used multiple times.")
        .value_name("package_name/type_name")
        .conflicts_with("input"),
    )
    .arg(
      Arg::new("output")
        .short('o')
        .long("output")
        .help("Output path")
        .value_name("file/dir"),
    )
    .arg(
      Arg::new("workspace")
        .short('w')
        .long("workspace")
        .help("Ros 2 workspace path")
        .value_name("dir"),
    )
    .arg(
      Arg::new("logdir")
        .short('l')
        .long("logdir")
        .help("Dir to write colcon logs")
        .value_name("logdir"),
    )
    .get_matches();

  let colcon_log_directory: PathBuf =
    if let Some(log_dir) = arg_matches.get_one::<String>("logdir").map(String::as_str) {
      PathBuf::from(log_dir)
    } else {
      PathBuf::from("/dev/null")
    };

  if let Some(input_file_name) = arg_matches.get_one::<String>("input").map(String::as_str) {
    // Just one input file
    let input_file = fs::File::open(input_file_name)?;

    let type_name = Path::new(input_file_name)
      .file_stem()
      .ok_or(io::Error::new(
        io::ErrorKind::Other,
        "Input file did not have base name?",
      ))?
      .to_string_lossy()
      .into_owned();

    let input = io::read_to_string(input_file)?;

    let msg = parser::msg_spec(&input).unwrap_or_else(|e| panic!("Parse error: {:?}", e));

    match arg_matches.get_one::<String>("output") {
      None => {
        print_struct_definition(&mut io::stdout(), &type_name, &msg.1)?;
      }
      Some(out_file_name) => {
        let mut out_file = fs::File::create(out_file_name)?;
        print_struct_definition(&mut out_file, &type_name, &msg.1)?;
      }
    }
  } else if let Some(ros2_types_requested) = arg_matches.get_many::<String>("type") {
    let output_dir = arg_matches
      .get_one::<String>("output")
      .ok_or(io::Error::new(io::ErrorKind::Other, "Output dir required"))?;
    let workspace_dir = arg_matches
      .get_one::<String>("workspace")
      .ok_or(io::Error::new(
        io::ErrorKind::Other,
        "ROS 2 workspace dir required",
      ))?;

    // Use colcon to determine what we need to translate
    let mut pkgs = Vec::new();
    println!(
      "Requested types: {:?}",
      ros2_types_requested.clone().collect::<Vec<&String>>()
    );
    for ros2_type in ros2_types_requested {
      use itertools::Itertools; // to get .unique()
      let new_pkgs = list_packges_with_msgs(workspace_dir, ros2_type, &colcon_log_directory)?;
      let prev_pkgs = pkgs;
      pkgs = prev_pkgs
        .iter()
        .chain(new_pkgs.iter())
        .unique()
        .cloned()
        .collect();
    }

    // Now we should have a Vec of unique required pkgs from most primitive to least
    // primitive.

    let mut mod_file_name = output_dir.clone();
    mod_file_name.extend(["/mod.rs"]);
    let mut mod_file = fs::File::create(mod_file_name)?;

    for pkg in &pkgs {
      let mut output_file_name = output_dir.clone();
      output_file_name.extend(["/", &pkg.name, ".rs"]);
      println!("Generating to {:?}", output_file_name);
      let mut out_file = fs::File::create(output_file_name)?;
      writeln!(mod_file, "mod {};", pkg.name)?;

      writeln!(out_file, "// Generated code. Do not modify.")?;
      writeln!(out_file, "use serde::{{Serialize,Deserialize}};")?;
      writeln!(out_file, "#[allow(unused_imports)]")?;
      writeln!(out_file, "use ros2_client::WString;")?;
      writeln!(out_file)?;

      for (ros2type, type_def) in &pkg.types {
        println!("  type {:?}", ros2type);
        let msg = parser::msg_spec(type_def).unwrap_or_else(|e| panic!("Parse error: {:?}", e));
        // TODO: msg.0 should be empty string here, warn if not.
        print_struct_definition(&mut out_file, ros2type, &msg.1)?;
      }
    }
  } else {
    println!("Please specify input by either -i or -t option.")
  }

  Ok(())
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
struct RosPkg {
  name: String,
  path: String,
  types: BTreeMap<String, String>, // .msg file name stems --> file contents
}

//use std::{ffi::OsStr, path::PathBuf};

use bstr::ByteSlice;

fn list_packges_with_msgs(
  workspace_dir: &str,
  ros2_abs_type: &str,
  colcon_log_directory: &Path,
) -> io::Result<Vec<RosPkg>> {
  let (package_name, _type_name) = ros2_abs_type.rsplit_once('/').ok_or(io::Error::new(
    io::ErrorKind::Other,
    "Need package_name/type_name",
  ))?;

  let cwd = std::env::current_dir()?;
  println!("Changing to {workspace_dir}");
  std::env::set_current_dir(workspace_dir)?;

  println!("Querying colcon");
  let mut colcon_log_option = "--log-base=".to_string();
  colcon_log_option.push_str(colcon_log_directory.to_str().unwrap());
  let colcon_output = std::process::Command::new("colcon")
    .arg(OsStr::new(&colcon_log_option))
    .arg("list")
    .arg("--topological-order")
    .arg("--packages-up-to")
    .arg(package_name)
    .output()?;

  if colcon_output.status.success() {
    let mut result = Vec::new();
    for line in colcon_output.stdout.lines() {
      match line
        .fields_with(|c| c.is_whitespace())
        .collect::<Vec<&[u8]>>()
        .as_slice()
      {
        [package_name, package_path, _build_tool] => {
          // let's see if there are any .msg
          let package_path = String::from_utf8_lossy(package_path).into_owned();
          let package_name = String::from_utf8_lossy(package_name).into_owned();
          let mut msg_dir = PathBuf::from(package_path.clone());
          msg_dir.push("msg");
          let mut types = BTreeMap::new();
          if let Ok(dir_iter) = fs::read_dir(msg_dir.clone()) {
            println!("Package path {msg_dir:?}");
            for dir_entry in dir_iter {
              let path = dir_entry?.path();
              if path.extension() == Some(OsStr::new("msg")) {
                if let Some(type_name) = path.file_stem() {
                  let msg_spec = io::read_to_string(fs::File::open(path.clone())?)?;
                  types.insert(type_name.to_string_lossy().into_owned(), msg_spec);
                } else {
                  // file name has no stem??
                  println!("Weird file name {:?}", path);
                }
              } else {
                println!("{:?} is not .msg", path);
              }
            } // for .msg files (types)
          } else {
            //println!("No {msg_dir:?}");
          }
          if !types.is_empty() {
            let pkg = RosPkg {
              name: package_name,
              path: package_path,
              types,
            };
            result.push(pkg);
          }
        } // package
        other => panic!("Colcon list output: {:?}", other),
      }
    } // for packages
    std::env::set_current_dir(cwd)?; // restore
    println!("Got {} packages", result.len());
    Ok(result)
  } else {
    Err(io::Error::new(
      io::ErrorKind::Other,
      format!(
        "Colcon failure: {}\nHave you run local_setup.bash?",
        String::from_utf8_lossy(&colcon_output.stderr)
      ),
    ))
  }
}

fn print_struct_definition<W: io::Write>(
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
            let rust_value = translate_value(value);
            write!(w, "pub const {const_name} : {rust_type} = {rust_value};")?;
          }
        }

        if let Some(Comment(c)) = comment_opt {
          writeln!(w, "// {c}")?;
        } else {
          writeln!(w)?;
        }
      }
    }
  }

  writeln!(w, "#[derive(Debug, Serialize, Deserialize)]")?;
  writeln!(w, "pub struct {name} {{")?;
  for (item, comment) in got_field {
    match (item, comment) {
      (None, None) => writeln!(w)?, // empty line
      (None, Some(Comment(c))) => writeln!(w, "  // {c}")?,
      (Some(item), comment_opt) => {
        write!(w, "  ")?;
        match item {
          Item::Field {
            type_name,
            field_name,
            ..
          } => {
            let rust_type = translate_type(type_name)?;
            write!(w, "{} : {}, ", escape_keywords(field_name), rust_type)?;
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

const RUST_BYTESTRING: &str = "String";
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
      base = format!("[{};{}]", base, size);
    }
    Some(ArraySpecifier::Unbounded) | Some(ArraySpecifier::Bounded { .. }) => {
      base = format!("Vec<{}>", base);
    }
  }

  Ok(base)
}

fn translate_value(v: &Value) -> String {
  match v {
    Value::Bool(b) => {
      if *b {
        "true".to_string()
      } else {
        "false".to_string()
      }
    }
    Value::Float(f) => format!("{f}"),
    Value::Int(i) => format!("{i}"),
    Value::Uint(u) => format!("{u}"),
    Value::String(v) => String::from_utf8(v.to_vec()).unwrap(),
  }
}

//! This module defines types to represent ROS 2 names for
//! * Message types, e.g. `std_msgs/String`
//! * Service types, e.g. `turtlesim/Spawn`
//! * action types, e.g. `turtlesim/RotateAbsolute`
//! *

use std::fmt;

// TODO:
// Conform fully to https://design.ros2.org/articles/topic_and_service_names.html
// and
// https://wiki.ros.org/Names --> Section 1.1.1 Valid Names

/// Names for Nodes
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeName {
  namespace: String,
  base_name: String,
}

impl NodeName {
  pub fn new(namespace: &str, base_name: &str) -> Result<NodeName, NameError> {
    match base_name.chars().next() {
      None => return Err(NameError::Empty),
      Some(c) if c.is_ascii_alphabetic() || c == '_' => { /*ok*/ }
      Some(other) => return Err(NameError::BadChar(other)),
    }

    if let Some(bad) = base_name
      .chars()
      .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
      return Err(NameError::BadChar(bad));
    }

    match namespace.chars().next() {
      None => {
        return Err(NameError::BadSlash(
          "<empty_namespace>".to_owned(),
          base_name.to_owned(),
        ))
      }
      Some(c) if c.is_ascii_alphabetic() || c == '/' => { /*ok*/ }
      // Character '~' is not accepted, because we do not know what that would mean in a Node's
      // name.
      Some(other) => return Err(NameError::BadChar(other)),
    }

    if namespace.starts_with('/') {
      // This is what we expect
    } else {
      return Err(NameError::BadSlash(
        namespace.to_owned(),
        base_name.to_owned(),
      ));
    }

    // Otherwise, what would be the absolute node name?
    if let Some(bad) = namespace
      .chars()
      .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '/'))
    {
      return Err(NameError::BadChar(bad));
    }

    if namespace.ends_with('/') && namespace != "/" {
      return Err(NameError::BadSlash(
        namespace.to_owned(),
        base_name.to_owned(),
      ));
    }

    Ok(NodeName {
      namespace: namespace.to_owned(),
      base_name: base_name.to_owned(),
    })
  }

  pub fn namespace(&self) -> &str {
    &self.namespace
  }
  pub fn base_name(&self) -> &str {
    &self.base_name
  }

  pub fn fully_qualified_name(&self) -> String {
    let mut fqn = self.namespace.clone();
    if fqn.ends_with('/') {
      // do nothing, it already ends with slash, i.e. is exactly "/"
    } else {
      fqn.push('/');
    }
    fqn.push_str(&self.base_name);
    fqn
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
  Empty,
  BadChar(char),
  BadSlash(String, String),
}

impl fmt::Display for NameError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      NameError::Empty => write!(f, "Base name must not be empty"),
      NameError::BadChar(c) => write!(f, "Bad chracters in Name: {c:?}"),
      NameError::BadSlash(ns, n) => write!(
        f,
        "Invalid placement of seprator slashes. namespace={ns}  name={n}"
      ),
    }
  }
}

impl std::error::Error for NameError {}

/// Names for Topics, Services
///
/// See [Names](https://wiki.ros.org/Names) for ROS 1.
/// and [topic and Service name mapping to DDS](https://design.ros2.org/articles/topic_and_service_names.html)
/// in ROS 2 documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
  base_name: String, // The last part of the full name. Must not be empty.
  preceeding_tokens: Vec<String>, // without separating slashes
  absolute: bool,    // in string format, absolute names begin with a slash
}

// TODO: We do not (yet) support tilde-expansion or brace-substitutions.

impl Name {
  /// Construct a new `Name` from namespace and base name.
  ///
  /// If the namespace begins with a slash (`/`) character, the Name will be
  /// absolute, otherwise it will be relative.
  /// The namespace may consist of several components, separated by slashes.
  /// Tha namespace must not end in a slash, unless the namespace is just `"/"`.
  ///
  /// Do not put slashes in the `base_name`.
  /// Base name is not allowed to be empty, but the namespace may be empty.
  ///
  /// Tilde or brace substitutions are not (yet) supported.
  pub fn new(namespace: &str, base_name: &str) -> Result<Name, NameError> {
    // TODO: Implement all of the checks here
    let (namespace_rel, absolute) = if let Some(rel) = namespace.strip_prefix('/') {
      (rel, true)
    } else {
      (namespace, false)
    };

    if base_name.is_empty() {
      return Err(NameError::Empty);
    }

    let ok_start_char = |c: char| c.is_ascii_alphabetic() || c == '_';
    let no_multi_underscore = |s: &str| !s.contains("__");

    if let Some(bad) = base_name
      .chars()
      .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
      return Err(NameError::BadChar(bad));
    } else if !base_name.starts_with(ok_start_char) {
      return Err(NameError::BadChar(base_name.chars().next().unwrap_or('?')));
    } else if !no_multi_underscore(base_name) {
      return Err(NameError::BadChar('_'));
    } else {
      // ok
    }

    let preceeding_tokens = if namespace_rel.is_empty() {
      // If the namespace is "" or "/", we want [] instead of [""]
      Vec::new()
    } else {
      namespace_rel
        .split('/')
        .map(str::to_owned)
        .collect::<Vec<String>>()
      // Starting slash, ending slash, or repeated slash all
      // produce empty strings.
    };

    if preceeding_tokens.iter().any(String::is_empty) {
      return Err(NameError::BadSlash(
        namespace_rel.to_owned(),
        base_name.to_owned(),
      ));
    }

    if preceeding_tokens.iter().all(|tok| {
      tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && tok.starts_with(ok_start_char)
        && no_multi_underscore(tok)
    }) { /* ok */
    } else {
      return Err(NameError::BadChar('?')); //TODO. Find which char is bad.
    }

    Ok(Name {
      base_name: base_name.to_owned(),
      preceeding_tokens,
      absolute,
    })
  }

  /// Construct a new `Name` from slash-separated namespace and base name.
  ///
  /// e.g. `myspace/some_name`
  pub fn parse(full_name: &str) -> Result<Name, NameError> {
    match full_name.rsplit_once('/') {
      // no slash, just a base name, so namespace is "".
      None => Name::new("", full_name),

      // Just a single slash, i.e. empty namespace and empty base name.
      // Not acceptable.
      Some(("", "")) => Err(NameError::Empty),

      // Last character was slash => base name is empty => bad.
      Some((bad, "")) => Err(NameError::BadSlash(bad.to_owned(), "".to_owned())),

      // Input was "/foobar", so name is absolute
      Some(("", base)) => Name::new("/", base),

      // General case: <nonempty> "/" <base_name>
      Some((prefix, base)) => {
        if prefix.ends_with('/') {
          // There was a double slash => Bad.
          Err(NameError::BadSlash(prefix.to_owned(), base.to_owned()))
        } else {
          Name::new(prefix, base)
        }
      }
    }
  }

  pub fn to_dds_name(&self, kind_prefix: &str, node: &NodeName, suffix: &str) -> String {
    let mut result = kind_prefix.to_owned();
    assert!(!result.ends_with('/')); // "rt"
    if self.absolute {
      // absolute name: do not add node namespace
    } else {
      // relative name: Prefix with Node namespace
      result.push_str(node.namespace()); // "rt/node_ns"
    }
    result.push('/'); // "rt/node_ns/" or "rt/"
    self.preceeding_tokens.iter().for_each(|tok| {
      result.push_str(tok);
      result.push('/');
    });
    // rt/node_ns/prec_tok1/
    result.push_str(&self.base_name);
    result.push_str(suffix);
    result
  }

  pub(crate) fn push(&self, new_suffix: &str) -> Name {
    //TODO: Check that we still satisfy naming rules
    let mut preceeding_tokens = self.preceeding_tokens.clone();
    preceeding_tokens.push(self.base_name.to_string());
    Name {
      base_name: new_suffix.to_string(),
      preceeding_tokens,
      absolute: self.absolute,
    }
  }

  pub fn is_absolute(&self) -> bool {
    self.absolute
  }
}

impl fmt::Display for Name {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    if self.absolute {
      write!(f, "/")?;
    }
    for t in &self.preceeding_tokens {
      write!(f, "{t}/")?;
    }
    write!(f, "{}", self.base_name)
  }
}

/// Name for `.msg` type, or a data type carried over a Topic.
///
/// This would be called a "Pacakge Resource Name", at least in ROS 1.
///
/// Note that this is not for naming Topics, but data types of Topics.
///
/// See [Names](https://wiki.ros.org/Names) Section 1.2 Package Resource Names.
#[derive(Clone, Debug)]
pub struct MessageTypeName {
  prefix: String, // typically "msg", but may be "action". What should this part be called?
  //TODO: String is strictly UTF-8, but ROS2 uses just byte strings that are recommended to be
  // UTF-8
  ros2_package_name: String, // or should this be "namespace"?
  ros2_type_name: String,
}

impl MessageTypeName {
  pub fn new(package_name: &str, type_name: &str) -> Self {
    //TODO: Ensure parameters have no leading/trailing slashes
    MessageTypeName {
      prefix: "msg".to_string(),
      ros2_package_name: package_name.to_owned(),
      ros2_type_name: type_name.to_owned(),
    }
  }

  pub(crate) fn new_prefix(package_name: &str, type_name: &str, prefix: String) -> Self {
    MessageTypeName {
      prefix,
      ros2_package_name: package_name.to_owned(),
      ros2_type_name: type_name.to_owned(),
    }
  }

  pub fn package_name(&self) -> &str {
    self.ros2_package_name.as_str()
  }

  pub fn type_name(&self) -> &str {
    self.ros2_type_name.as_str()
  }

  /// Convert to type name used over DDS
  pub fn dds_msg_type(&self) -> String {
    slash_to_colons(
      self.ros2_package_name.clone() + "/" + &self.prefix + "/dds_/" + &self.ros2_type_name + "_",
    )
  }
}

fn slash_to_colons(s: String) -> String {
  s.replace('/', "::")
}

/// Similar to [`MessageTypeName`], but names a Service type.
#[derive(Clone, Debug)]
pub struct ServiceTypeName {
  prefix: String,
  msg: MessageTypeName,
}

impl ServiceTypeName {
  pub fn new(package_name: &str, type_name: &str) -> Self {
    ServiceTypeName {
      prefix: "srv".to_string(),
      msg: MessageTypeName::new(package_name, type_name),
    }
  }

  pub(crate) fn new_prefix(package_name: &str, type_name: &str, prefix: String) -> Self {
    ServiceTypeName {
      prefix,
      msg: MessageTypeName::new(package_name, type_name),
    }
  }

  pub fn package_name(&self) -> &str {
    self.msg.package_name()
  }

  pub fn type_name(&self) -> &str {
    self.msg.type_name()
  }

  pub(crate) fn dds_request_type(&self) -> String {
    slash_to_colons(
      self.package_name().to_owned()
        + "/"
        + &self.prefix
        + "/dds_/"
        + self.type_name()
        + "_Request_",
    )
  }

  pub(crate) fn dds_response_type(&self) -> String {
    slash_to_colons(
      self.package_name().to_owned()
        + "/"
        + &self.prefix
        + "/dds_/"
        + self.type_name()
        + "_Response_",
    )
  }
}

/// Similar to [`MessageTypeName`], but names an Action type.
#[derive(Clone, Debug)]
pub struct ActionTypeName(MessageTypeName);

impl ActionTypeName {
  pub fn new(package_name: &str, type_name: &str) -> Self {
    ActionTypeName(MessageTypeName::new(package_name, type_name))
  }

  pub fn package_name(&self) -> &str {
    self.0.package_name()
  }

  pub fn type_name(&self) -> &str {
    self.0.type_name()
  }

  pub(crate) fn dds_action_topic(&self, topic: &str) -> MessageTypeName {
    MessageTypeName::new_prefix(
      self.package_name(),
      &(self.type_name().to_owned() + topic),
      "action".to_owned(),
    )
    //slash_to_colons(self.package_name().to_owned() + "/action/dds_/" +
    // &self.type_name())
  }

  pub(crate) fn dds_action_service(&self, srv: &str) -> ServiceTypeName {
    ServiceTypeName::new_prefix(
      self.package_name(),
      &(self.type_name().to_owned() + srv),
      "action".to_owned(),
    )
  }
}

// -------------------------------------------------------------------------------------
// -------------------------------------------------------------------------------------

#[test]
fn test_name() {
  assert!(Name::new("", "").is_err());
  assert!(Name::new("", "/").is_err());
  assert!(Name::new("a", "b").is_ok());
  assert!(Name::new("a", "_b").is_ok());
  assert!(Name::new("a", "b_b").is_ok()); // may contain [...] underscores (_), [...]
  assert!(Name::new("a", "b__b").is_err()); // must not contain any number of repeated underscores (_)
  assert!(Name::new("a2//a", "b").is_err()); // must not contain any number of
                                             // repeated forward slashes (/)
}

#[test]
fn test_name_parse() {
  // https://design.ros2.org/articles/topic_and_service_names.html

  assert!(Name::parse("").is_err()); // must not be empty
  assert!(Name::parse("/").is_err()); // must not be empty
  assert!(Name::parse("a/").is_err()); // must not be empty
  assert!(Name::parse("a/b/").is_err());

  assert!(Name::parse("2").is_err()); // must not start with a numeric character ([0-9])
  assert!(Name::parse("2/a").is_err()); // must not start with a numeric character ([0-9])
  assert!(Name::parse("a2/a").is_ok());
  assert!(Name::parse("_a2/a").is_ok()); // may contain [...] underscores (_), [...]
  assert!(Name::parse("some_name/a").is_ok()); // may contain [...] underscores (_), [...]
  assert!(Name::parse("__a2/a").is_err()); // must not contain any number of repeated underscores (_)
  assert!(Name::parse("a2//a").is_err()); // must not contain any number of repeated forward slashes (/)

  assert_eq!(Name::parse("a/nn").unwrap(), Name::new("a", "nn").unwrap());
  assert_eq!(
    Name::parse("a/b/c/nn").unwrap(),
    Name::new("a/b/c", "nn").unwrap()
  );
  assert_eq!(
    Name::parse("/a/b/c/nn").unwrap(),
    Name::new("/a/b/c", "nn").unwrap()
  );

  assert!(!Name::parse("a/nn").unwrap().is_absolute());
  assert!(Name::parse("/a/nn").unwrap().is_absolute());
}

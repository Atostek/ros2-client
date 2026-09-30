// Wide string, i.e. UTF-16 Strings
// We just wrap a pre-existing library to get proper Serialize and Deserialize.

use std::fmt;

use serde::{
  Deserialize, Deserializer, Serialize, Serializer,
  de::{SeqAccess, Visitor},
  ser::SerializeSeq,
};
use widestring::Utf16String;

/// UTF-16 strings, as required by the ROS type system.
///
/// We just wrap a pre-existing library to get proper Serialize and Deserialize.
#[derive(Clone, Debug)]
pub struct WString {
  inner: Utf16String,
}

impl WString {
  pub fn new() -> Self {
    WString {
      inner: Utf16String::new(),
    }
  }
}

impl Default for WString {
  fn default() -> Self {
    Self::new()
  }
}

impl From<Utf16String> for WString {
  fn from(inner: Utf16String) -> Self {
    WString { inner }
  }
}

impl From<WString> for Utf16String {
  fn from(w: WString) -> Utf16String {
    w.inner
  }
}

impl core::ops::Deref for WString {
  type Target = Utf16String;
  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl Serialize for WString {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(self.inner.len()))?;
    for e in self.inner.as_slice() {
      seq.serialize_element(e)?;
    }
    seq.end()
  }
}

impl<'de> Deserialize<'de> for WString {
  fn deserialize<D>(deserializer: D) -> Result<WString, D::Error>
  where
    D: Deserializer<'de>,
  {
    deserializer.deserialize_seq(WStringVisitor)
  }
}

struct WStringVisitor;

impl<'de> Visitor<'de> for WStringVisitor {
  type Value = WString;

  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    write!(formatter, "a wide string in UTF-16")
  }

  fn visit_seq<A>(self, mut seq: A) -> Result<WString, A::Error>
  where
    A: SeqAccess<'de>,
  {
    let mut inner: Utf16String = seq
      .size_hint()
      .map_or_else(Utf16String::new, Utf16String::with_capacity);
    while let Some(wc) = seq.next_element()? {
      inner.push(wc)
    }
    Ok(inner.into())
  }
}

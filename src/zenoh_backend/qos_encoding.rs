//! Compact QoS encoding for Zenoh liveliness tokens (E5).
//!
//! `rmw_zenoh` embeds a QoS string in each liveliness token's `<qos>` component
//! (see `docs/zenoh_study/research/rmw_zenoh.md` §6). The layout is six
//! `:`-separated groups:
//!
//! ```text
//! <rel>:<dur>:<hist>,<depth>:<dl_s>,<dl_ns>:<ls_s>,<ls_ns>:<liv>,<lease_s>,<lease_ns>
//! ```
//!
//! Every field except **depth** is *delta-encoded*: it is emitted only when it
//! differs from the RMW default profile, otherwise left empty and filled from
//! the default on decode. **Depth is always emitted.** This reproduces the
//! documented examples, e.g. a reliable/volatile/keep-last publisher with depth
//! 7 encodes as `::,7:,:,:,,`.
//!
//! Numeric policy codes match `rmw_qos_policy_*_t`. Byte-exact parity with C++
//! peers is confirmed by live interop (ADR-0007/0009); this module is unit
//! tested for the documented vectors and for round-tripping.

use std::time::Duration;

use crate::qos::{Durability, History, Liveliness, QosProfile, Reliability};

// RMW default profile (`rmw_qos_profile_default`) used as the delta reference.
const DEFAULT_RELIABILITY: Reliability = Reliability::Reliable;
const DEFAULT_DURABILITY: Durability = Durability::Volatile;
const DEFAULT_LIVELINESS: Liveliness = Liveliness::Automatic;
// Default history is KEEP_LAST; only KEEP_ALL is non-default.

fn reliability_code(r: Reliability) -> &'static str {
  match r {
    Reliability::Reliable => "1",
    Reliability::BestEffort => "2",
  }
}

fn durability_code(d: Durability) -> &'static str {
  match d {
    Durability::TransientLocal => "1",
    Durability::Volatile => "2",
  }
}

fn liveliness_code(l: Liveliness) -> &'static str {
  match l {
    Liveliness::Automatic => "1",
    Liveliness::ManualByTopic => "3",
  }
}

// A duration renders as two fields "<sec>,<nsec>"; `None` (infinite/default)
// renders as two empty fields.
fn duration_fields(d: Option<Duration>) -> (String, String) {
  match d {
    None => (String::new(), String::new()),
    Some(d) => (d.as_secs().to_string(), d.subsec_nanos().to_string()),
  }
}

/// Encode a [`QosProfile`] into the compact liveliness `<qos>` string.
pub fn encode_qos(q: &QosProfile) -> String {
  let rel = if q.reliability == DEFAULT_RELIABILITY {
    ""
  } else {
    reliability_code(q.reliability)
  };
  let dur = if q.durability == DEFAULT_DURABILITY {
    ""
  } else {
    durability_code(q.durability)
  };
  let (hist, depth) = match q.history {
    History::KeepLast { depth } => ("", depth),
    History::KeepAll => ("2", 0),
  };
  let liv = if q.liveliness == DEFAULT_LIVELINESS {
    ""
  } else {
    liveliness_code(q.liveliness)
  };
  let (dl_s, dl_ns) = duration_fields(q.deadline);
  let (ls_s, ls_ns) = duration_fields(q.lifespan);
  let (lease_s, lease_ns) = duration_fields(q.liveliness_lease);

  format!("{rel}:{dur}:{hist},{depth}:{dl_s},{dl_ns}:{ls_s},{ls_ns}:{liv},{lease_s},{lease_ns}")
}

/// Failure to parse a compact QoS string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QosDecodeError;

fn parse_duration(sec: &str, nsec: &str) -> Result<Option<Duration>, QosDecodeError> {
  match (sec.is_empty(), nsec.is_empty()) {
    (true, true) => Ok(None),
    _ => {
      let s: u64 = sec.parse().map_err(|_| QosDecodeError)?;
      let ns: u32 = nsec.parse().map_err(|_| QosDecodeError)?;
      Ok(Some(Duration::new(s, ns)))
    }
  }
}

/// Decode a compact liveliness `<qos>` string into a [`QosProfile`], filling
/// empty (default) fields from the RMW default profile.
pub fn decode_qos(s: &str) -> Result<QosProfile, QosDecodeError> {
  let groups: Vec<&str> = s.split(':').collect();
  if groups.len() != 6 {
    return Err(QosDecodeError);
  }

  let reliability = match groups[0] {
    "" => DEFAULT_RELIABILITY,
    "1" => Reliability::Reliable,
    "2" => Reliability::BestEffort,
    _ => return Err(QosDecodeError),
  };
  let durability = match groups[1] {
    "" => DEFAULT_DURABILITY,
    "1" => Durability::TransientLocal,
    "2" => Durability::Volatile,
    _ => return Err(QosDecodeError),
  };

  // history group: "<hist>,<depth>"
  let (hist_str, depth_str) = groups[2].split_once(',').ok_or(QosDecodeError)?;
  let depth: usize = if depth_str.is_empty() {
    0
  } else {
    depth_str.parse().map_err(|_| QosDecodeError)?
  };
  let history = match hist_str {
    "" | "1" => History::KeepLast { depth },
    "2" => History::KeepAll,
    _ => return Err(QosDecodeError),
  };

  let (dl_s, dl_ns) = groups[3].split_once(',').ok_or(QosDecodeError)?;
  let deadline = parse_duration(dl_s, dl_ns)?;
  let (ls_s, ls_ns) = groups[4].split_once(',').ok_or(QosDecodeError)?;
  let lifespan = parse_duration(ls_s, ls_ns)?;

  // liveliness group: "<liv>,<lease_s>,<lease_ns>"
  let mut liv_parts = groups[5].splitn(3, ',');
  let liv_str = liv_parts.next().unwrap_or("");
  let lease_s = liv_parts.next().unwrap_or("");
  let lease_ns = liv_parts.next().unwrap_or("");
  let liveliness = match liv_str {
    "" => DEFAULT_LIVELINESS,
    "1" => Liveliness::Automatic,
    "3" => Liveliness::ManualByTopic,
    _ => return Err(QosDecodeError),
  };
  let liveliness_lease = parse_duration(lease_s, lease_ns)?;

  Ok(QosProfile {
    reliability,
    durability,
    history,
    deadline,
    lifespan,
    liveliness,
    liveliness_lease,
  })
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  // The rmw_zenoh talker/listener examples: reliable, volatile, keep-last,
  // automatic, no deadline/lifespan/lease — only depth differs.
  fn keep_last(depth: usize) -> QosProfile {
    QosProfile {
      reliability: Reliability::Reliable,
      durability: Durability::Volatile,
      history: History::KeepLast { depth },
      deadline: None,
      lifespan: None,
      liveliness: Liveliness::Automatic,
      liveliness_lease: None,
    }
  }

  #[test]
  fn matches_rmw_zenoh_examples() {
    assert_eq!(encode_qos(&keep_last(7)), "::,7:,:,:,,");
    assert_eq!(encode_qos(&keep_last(10)), "::,10:,:,:,,");
  }

  #[test]
  fn round_trips() {
    let profiles = [
      keep_last(7),
      keep_last(10),
      QosProfile {
        reliability: Reliability::BestEffort,
        durability: Durability::TransientLocal,
        history: History::KeepAll,
        deadline: Some(Duration::new(1, 500)),
        lifespan: Some(Duration::new(10, 0)),
        liveliness: Liveliness::ManualByTopic,
        liveliness_lease: Some(Duration::new(2, 0)),
      },
    ];
    for p in profiles {
      let encoded = encode_qos(&p);
      let decoded = decode_qos(&encoded).expect("decode");
      // KeepAll loses depth (rmw uses 0); compare via re-encode for stability.
      assert_eq!(
        encode_qos(&decoded),
        encoded,
        "re-encode mismatch for {p:?}"
      );
    }
  }

  #[test]
  fn decode_examples() {
    assert_eq!(decode_qos("::,7:,:,:,,").unwrap(), keep_last(7));
    assert_eq!(decode_qos("::,10:,:,:,,").unwrap(), keep_last(10));
  }

  #[test]
  fn rejects_malformed() {
    assert_eq!(decode_qos("too:few:groups"), Err(QosDecodeError));
    assert_eq!(decode_qos(""), Err(QosDecodeError));
  }
}

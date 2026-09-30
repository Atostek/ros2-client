//! Backend-neutral Quality-of-Service profile.
//!
//! [`QosProfile`] is `ros2-client`'s own representation of a ROS 2 QoS profile
//! (the `rmw_qos_profile_t` knobs), independent of any middleware. It exists so
//! the public API does not have to name a `rustdds` type — the `zenoh` backend
//! cannot depend on `rustdds` (see
//! `docs/decisions/0004-owned-public-types.md`).
//!
//! * On the **`dds`** backend it converts to/from [`rustdds::QosPolicies`]
//!   (this module's `From` impls, gated on the `dds` feature).
//! * On the **`zenoh`** backend it drives publisher/subscriber options and the
//!   compact QoS encoding embedded in liveliness keys (E2/E5). [`WhenFull`] is
//!   part of [`Reliability::Reliable`] and is not part of that encoding.
//!
//! Phase 1 of ADR-0010: public `create_*` APIs take [`QosProfile`]; RustDDS
//! `QosPolicies` is only used at the DDS adapter boundary (via `From`).

use std::time::Duration;

/// What a Reliable `publish` does when the send window is full of samples that
/// have not been acknowledged yet.
///
/// This is part of [`Reliability::Reliable`]. The Zenoh liveliness QoS string
/// does not carry it; a decoded profile uses [`WhenFull::DEFAULT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhenFull {
  /// Return [`crate::WriteError::WouldBlock`] immediately.
  Fail,
  /// Wait up to this long for space, then return
  /// [`crate::WriteError::WouldBlock`].
  Wait(Duration),
  /// Wait until space is available.
  Block,
}

impl WhenFull {
  /// Historical default: wait 100 ms for send-buffer space.
  pub const DEFAULT: Self = Self::Wait(Duration::from_millis(100));
}

/// Delivery guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reliability {
  /// Samples may be dropped. `publish` does not wait for send-buffer space.
  BestEffort,
  /// Lost samples are retransmitted. `when_full` is what `publish` does when
  /// the send window is full.
  Reliable {
    /// Behavior when the reliable send window is full.
    when_full: WhenFull,
  },
}

/// Whether late-joining subscriptions receive previously-published samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
  /// Only samples published while a subscription is alive are delivered.
  Volatile,
  /// Late joiners receive the last `depth` samples (latched/transient-local).
  TransientLocal,
}

/// How many samples are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum History {
  /// Keep only the last `depth` samples.
  KeepLast {
    /// Number of samples retained.
    depth: usize,
  },
  /// Keep all samples (bounded by resource limits / backpressure).
  KeepAll,
}

/// Liveliness assertion policy. `ros2-client` (and `rmw_zenoh`) only support
/// automatic liveliness; `ManualByTopic` is accepted for completeness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveliness {
  /// The middleware asserts liveliness automatically.
  Automatic,
  /// The application asserts liveliness per topic.
  ManualByTopic,
}

/// A ROS 2 Quality-of-Service profile.
///
/// `deadline`, `lifespan`, and `liveliness_lease` use `None` to mean "infinite"
/// (no deadline / never expires / infinite lease), matching the ROS 2 default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QosProfile {
  /// Reliability policy.
  pub reliability: Reliability,
  /// Durability policy.
  pub durability: Durability,
  /// History policy.
  pub history: History,
  /// Maximum expected time between samples; `None` = infinite (no deadline).
  pub deadline: Option<Duration>,
  /// Maximum time a sample is valid; `None` = infinite (never expires).
  pub lifespan: Option<Duration>,
  /// Liveliness policy.
  pub liveliness: Liveliness,
  /// Liveliness lease duration; `None` = infinite.
  pub liveliness_lease: Option<Duration>,
}

impl QosProfile {
  /// The ROS 2 "sensor data" style default used for subscriptions: best-effort,
  /// volatile, keep-last depth 1, infinite deadline/lifespan, automatic
  /// liveliness.
  pub const fn subscription_default() -> Self {
    Self {
      reliability: Reliability::BestEffort,
      durability: Durability::Volatile,
      history: History::KeepLast { depth: 1 },
      deadline: None,
      lifespan: None,
      liveliness: Liveliness::Automatic,
      liveliness_lease: None,
    }
  }

  /// The default used for publishers: like [`Self::subscription_default`] but
  /// reliable, waiting 100 ms when the send window is full.
  pub const fn publisher_default() -> Self {
    Self {
      reliability: Reliability::Reliable {
        when_full: WhenFull::DEFAULT,
      },
      ..Self::subscription_default()
    }
  }

  /// Builder-style setter for reliability.
  #[must_use]
  pub const fn reliability(mut self, reliability: Reliability) -> Self {
    self.reliability = reliability;
    self
  }

  /// Set [`Reliability::BestEffort`].
  #[must_use]
  pub const fn reliability_best_effort(self) -> Self {
    self.reliability(Reliability::BestEffort)
  }

  /// Set [`Reliability::Reliable`] with the given full-window behavior.
  #[must_use]
  pub const fn reliability_reliable(self, when_full: WhenFull) -> Self {
    self.reliability(Reliability::Reliable { when_full })
  }

  /// Builder-style setter for durability.
  #[must_use]
  pub const fn durability(mut self, durability: Durability) -> Self {
    self.durability = durability;
    self
  }

  /// Builder-style setter for history.
  #[must_use]
  pub const fn history(mut self, history: History) -> Self {
    self.history = history;
    self
  }

  /// Builder-style setter for deadline (`None` = infinite).
  #[must_use]
  pub const fn deadline(mut self, deadline: Option<Duration>) -> Self {
    self.deadline = deadline;
    self
  }

  /// Builder-style setter for lifespan (`None` = infinite).
  #[must_use]
  pub const fn lifespan(mut self, lifespan: Option<Duration>) -> Self {
    self.lifespan = lifespan;
    self
  }

  /// Builder-style setter for liveliness.
  #[must_use]
  pub const fn liveliness(mut self, liveliness: Liveliness) -> Self {
    self.liveliness = liveliness;
    self
  }

  /// Builder-style setter for liveliness lease (`None` = infinite).
  #[must_use]
  pub const fn liveliness_lease(mut self, liveliness_lease: Option<Duration>) -> Self {
    self.liveliness_lease = liveliness_lease;
    self
  }
}

impl Default for QosProfile {
  fn default() -> Self {
    Self::subscription_default()
  }
}

// ---------------------------------------------------------------------------
// DDS backend conversions.
// ---------------------------------------------------------------------------

#[cfg(feature = "dds")]
mod dds_conv {
  use rustdds::{Duration as DdsDuration, QosPolicies, QosPolicyBuilder, policy};

  use super::{Durability, History, Liveliness, QosProfile, Reliability, WhenFull};

  fn to_max_blocking(when_full: WhenFull) -> DdsDuration {
    match when_full {
      WhenFull::Fail => DdsDuration::ZERO,
      WhenFull::Wait(d) => DdsDuration::from_nanos(d.as_nanos() as i64),
      WhenFull::Block => DdsDuration::INFINITE,
    }
  }

  fn from_max_blocking(d: DdsDuration) -> WhenFull {
    if d == DdsDuration::INFINITE {
      WhenFull::Block
    } else if d == DdsDuration::ZERO {
      WhenFull::Fail
    } else {
      // RTPS durations are 2^32 ticks per second, so `to_nanoseconds`
      // truncates. Use the nanosecond count that encodes back to the same
      // value.
      let truncated = d.to_nanoseconds().max(0) as u64;
      let nanos = if DdsDuration::from_nanos(truncated as i64) == d {
        truncated
      } else {
        truncated.saturating_add(1)
      };
      WhenFull::Wait(std::time::Duration::from_nanos(nanos))
    }
  }

  fn to_dds_duration(d: Option<std::time::Duration>) -> DdsDuration {
    match d {
      None => DdsDuration::INFINITE,
      Some(d) => DdsDuration::from_nanos(d.as_nanos() as i64),
    }
  }

  fn from_dds_duration(d: DdsDuration) -> Option<std::time::Duration> {
    if d == DdsDuration::INFINITE {
      None
    } else {
      Some(std::time::Duration::from_nanos(
        d.to_nanoseconds().max(0) as u64
      ))
    }
  }

  impl From<&QosProfile> for QosPolicies {
    fn from(p: &QosProfile) -> Self {
      QosPolicyBuilder::new()
        .durability(match p.durability {
          Durability::Volatile => policy::Durability::Volatile,
          Durability::TransientLocal => policy::Durability::TransientLocal,
        })
        .reliability(match p.reliability {
          Reliability::BestEffort => policy::Reliability::BestEffort,
          Reliability::Reliable { when_full } => policy::Reliability::Reliable {
            max_blocking_time: to_max_blocking(when_full),
          },
        })
        .history(match p.history {
          History::KeepLast { depth } => policy::History::KeepLast {
            depth: depth as i32,
          },
          History::KeepAll => policy::History::KeepAll,
        })
        .liveliness(match p.liveliness {
          Liveliness::Automatic => policy::Liveliness::Automatic {
            lease_duration: to_dds_duration(p.liveliness_lease),
          },
          Liveliness::ManualByTopic => policy::Liveliness::ManualByTopic {
            lease_duration: to_dds_duration(p.liveliness_lease),
          },
        })
        .deadline(policy::Deadline(to_dds_duration(p.deadline)))
        .lifespan(policy::Lifespan {
          duration: to_dds_duration(p.lifespan),
        })
        .build()
    }
  }

  impl From<QosProfile> for QosPolicies {
    fn from(p: QosProfile) -> Self {
      Self::from(&p)
    }
  }

  impl From<&QosPolicies> for QosProfile {
    fn from(q: &QosPolicies) -> Self {
      let reliability = match q.reliability() {
        Some(policy::Reliability::Reliable { max_blocking_time }) => Reliability::Reliable {
          when_full: from_max_blocking(max_blocking_time),
        },
        _ => Reliability::BestEffort,
      };
      let durability = match q.durability() {
        Some(policy::Durability::Volatile) | None => Durability::Volatile,
        Some(_) => Durability::TransientLocal, // TransientLocal/Transient/Persistent
      };
      let history = match q.history() {
        Some(policy::History::KeepAll) => History::KeepAll,
        Some(policy::History::KeepLast { depth }) => History::KeepLast {
          depth: depth.max(0) as usize,
        },
        None => History::KeepLast { depth: 1 },
      };
      let (liveliness, liveliness_lease) = match q.liveliness() {
        Some(policy::Liveliness::ManualByTopic { lease_duration }) => {
          (Liveliness::ManualByTopic, from_dds_duration(lease_duration))
        }
        Some(policy::Liveliness::Automatic { lease_duration }) => {
          (Liveliness::Automatic, from_dds_duration(lease_duration))
        }
        Some(policy::Liveliness::ManualByParticipant { lease_duration }) => {
          (Liveliness::Automatic, from_dds_duration(lease_duration))
        }
        None => (Liveliness::Automatic, None),
      };
      QosProfile {
        reliability,
        durability,
        history,
        deadline: q
          .deadline()
          .and_then(|policy::Deadline(d)| from_dds_duration(d)),
        lifespan: q.lifespan().and_then(|ls| from_dds_duration(ls.duration)),
        liveliness,
        liveliness_lease,
      }
    }
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_match_intent() {
    assert_eq!(
      QosProfile::subscription_default().reliability,
      Reliability::BestEffort
    );
    assert_eq!(
      QosProfile::publisher_default().reliability,
      Reliability::Reliable {
        when_full: WhenFull::DEFAULT
      }
    );
    assert_eq!(
      QosProfile::default().history,
      History::KeepLast { depth: 1 }
    );
  }

  #[test]
  fn builder_setters() {
    let q = QosProfile::default()
      .reliability_reliable(WhenFull::Fail)
      .durability(Durability::TransientLocal)
      .history(History::KeepLast { depth: 10 });
    assert_eq!(
      q.reliability,
      Reliability::Reliable {
        when_full: WhenFull::Fail
      }
    );
    assert_eq!(
      QosProfile::publisher_default()
        .reliability_best_effort()
        .reliability,
      Reliability::BestEffort
    );
    assert_eq!(q.durability, Durability::TransientLocal);
    assert_eq!(q.history, History::KeepLast { depth: 10 });
  }

  #[cfg(feature = "dds")]
  #[test]
  fn roundtrips_through_rustdds() {
    use std::time::Duration;
    let profiles = [
      QosProfile::subscription_default(),
      QosProfile::publisher_default(),
      QosProfile {
        reliability: Reliability::Reliable {
          when_full: WhenFull::DEFAULT,
        },
        durability: Durability::TransientLocal,
        history: History::KeepLast { depth: 10 },
        deadline: Some(Duration::from_millis(500)),
        lifespan: Some(Duration::from_secs(10)),
        liveliness: Liveliness::Automatic,
        liveliness_lease: Some(Duration::from_secs(2)),
      },
      QosProfile::publisher_default().reliability_reliable(WhenFull::Fail),
      QosProfile::publisher_default().reliability_reliable(WhenFull::Block),
      QosProfile::publisher_default()
        .reliability_reliable(WhenFull::Wait(Duration::from_millis(50))),
      QosProfile {
        history: History::KeepAll,
        ..QosProfile::publisher_default()
      },
    ];
    for p in profiles {
      let dds: rustdds::QosPolicies = (&p).into();
      let back: QosProfile = (&dds).into();
      assert_eq!(p, back, "QoS did not round-trip through rustdds: {p:?}");
    }
  }
}

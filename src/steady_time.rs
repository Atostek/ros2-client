//! Steady time has an arbitrary origin, but is guaranteed to run
//! monotonically (i.e. non-decreasing), regardless of clock corrections or
//! timezones.  
//!
//! *Note* Steady time is not actually steady in the sense that the underlying
//! clock would always run. Steady time clock might be paused e.g. because of
//! operating system sleep modes, hypervisors, or other infrastructure below
//! this library. This module is called `steady_time` only to better conform to
//! ROS 2 naming.
//!
//! The current implementation is based on `std::time::Instant`. Because of
//! this, there is no directo conversion to/from nanoseconds, or other time
//! types.
//!
//! Steady time should be used only when it is necessary, e.g. because of
//! interacting with hardware. Steady time cannot be simulated. Use ROS time
//! instead, whenever possible.

use std::{
  cmp::Ordering,
  convert::TryFrom,
  fmt,
  ops::{Add, Sub},
  sync::OnceLock,
  time::{Duration, Instant},
};

use chrono::{DateTime, Utc};

use crate::ROSTime;

/// Monotonic time in nanoseconds
///
/// To get offset to UTC time, use now_with_utc() note that the offset will
/// change over time, latest at the next leap second.
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug)]
pub struct Time {
  instant: Instant,
}

impl Time {
  pub fn now() -> Time {
    Self {
      instant: Instant::now(),
    }
  }

  /// returns the current time in two formats: Time and ROSTime
  pub fn now_with_ros_time() -> (Time, ROSTime) {
    let (st, ct) = Self::now_with_utc();
    let ros_time = ROSTime::try_from(ct)
      // try_from conversion already logs an error
      .unwrap_or(ROSTime::ZERO);
    (st, ros_time)
  }

  #[doc(hidden)]
  pub fn now_with_utc() -> (Time, DateTime<Utc>) {
    let m0 = Self::now();
    let utc = Utc::now();
    let m1 = Self::now();
    let diff = m1 - m0;
    //println!("now_with_utc() diff = {} ns" , diff.as_nanos() );
    // TODO: check that diff is very small and complain if not

    // We add half of the diff to compensate for the difference in call times.
    (m0 + TimeDiff::from_nanos(diff.as_nanos() / 2), utc)
  }
} // impl Time

/// A fixed per-process reference instant, so that steady [`Time`] values (which
/// have an arbitrary origin) can be displayed relative to a stable point.
fn process_origin() -> Instant {
  static ORIGIN: OnceLock<Instant> = OnceLock::new();
  *ORIGIN.get_or_init(Instant::now)
}

impl fmt::Display for Time {
  fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
    // Steady time has no absolute epoch, so show it relative to a process-wide
    // origin captured on first use.
    let since_origin = (*self
      - Time {
        instant: process_origin(),
      })
    .as_nanos() as f64
      / 1e9;
    write!(fmt, "steady_time({since_origin:+.9}s from process origin)")
  }
}

impl Sub for Time {
  type Output = TimeDiff;

  fn sub(self, other: Time) -> TimeDiff {
    self
      .instant
      .checked_duration_since(other.instant)
      // This fails if other > self. Then we try the other way around
      .map(|duration| TimeDiff {
        duration,
        is_negative: false,
      })
      .unwrap_or_else(|| TimeDiff {
        duration: other.instant.saturating_duration_since(self.instant),
        is_negative: true,
      })
  }
}

/// Note: This may panic on over/underflows
impl Sub<TimeDiff> for Time {
  type Output = Time;

  fn sub(self, diff: TimeDiff) -> Time {
    if diff.is_negative {
      Time {
        instant: self.instant - diff.duration,
      }
    } else {
      Time {
        instant: self.instant + diff.duration,
      }
    }
  }
}

/// Note: This may panic on over/underflows
impl Add<TimeDiff> for Time {
  type Output = Time;

  fn add(self, diff: TimeDiff) -> Time {
    if diff.is_negative {
      Time {
        instant: self.instant + diff.duration,
      }
    } else {
      Time {
        instant: self.instant - diff.duration,
      }
    }
  }
}

/// Time difference can be negative, unlike std::time::Duration
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeDiff {
  duration: Duration,
  is_negative: bool, // must be false if duration == Duration::ZERO
}

#[derive(Debug, Clone, Copy)]
pub struct NegativetimeDiffError {}

impl TimeDiff {
  pub const fn from_nanos(nanos: i64) -> TimeDiff {
    if nanos >= 0 {
      TimeDiff {
        duration: Duration::from_nanos(nanos as u64),
        is_negative: false,
      }
    } else {
      TimeDiff {
        duration: Duration::from_nanos(-nanos as u64),
        is_negative: true,
      }
    }
  }

  pub const fn from_millis(millis: i64) -> TimeDiff {
    Self::from_nanos(millis * 1_000_000)
  }

  pub const fn from_secs(secs: i64) -> TimeDiff {
    Self::from_nanos(secs * 1_000_000_000)
  }

  pub const fn as_nanos(self) -> i64 {
    let n = self.duration.as_nanos();
    let n = if n > (i64::MAX as u128) {
      i64::MAX
    } else {
      n as i64
    };
    if self.is_negative { -n } else { n }
  }

  pub const fn as_millis(self) -> i64 {
    self.as_nanos() / 1_000_000
  }

  #[allow(dead_code)]
  pub const fn as_seconds(self) -> i64 {
    self.as_nanos() / 1_000_000_000
  }

  pub fn as_duration(self) -> Result<Duration, NegativetimeDiffError> {
    if self.is_negative {
      Err(NegativetimeDiffError {})
    } else {
      Ok(self.duration)
    }
  }

  pub fn as_saturating_duration(self) -> Duration {
    if self.is_negative {
      Duration::ZERO
    } else {
      self.duration
    }
  }
}

impl Ord for TimeDiff {
  fn cmp(&self, other: &Self) -> Ordering {
    match (self.is_negative, other.is_negative) {
      (false, false) => self.duration.cmp(&other.duration),
      (true, true) => self.duration.cmp(&other.duration).reverse(),
      (false, true) => Ordering::Greater,
      (true, false) => Ordering::Less,
    }
  }
}

impl PartialOrd for TimeDiff {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl fmt::Display for TimeDiff {
  fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
    let nanos = self.as_nanos();
    let sign = if nanos < 0 { "-" } else { "" };
    let abs = nanos.unsigned_abs();
    write!(
      fmt,
      "{sign}{}.{:09}s",
      abs / 1_000_000_000,
      abs % 1_000_000_000
    )
  }
}

impl Add for TimeDiff {
  type Output = TimeDiff;
  fn add(self, other: TimeDiff) -> TimeDiff {
    Self::from_nanos(self.as_nanos() + other.as_nanos())
  }
}

impl Sub for TimeDiff {
  type Output = TimeDiff;
  fn sub(self, other: TimeDiff) -> TimeDiff {
    Self::from_nanos(self.as_nanos() - other.as_nanos())
  }
}

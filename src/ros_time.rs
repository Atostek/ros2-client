//! ROS timing abstractions [`ROSTime`] and [`SystemTime`]
//!
//! See the ROS 2 design article [Clock and Time](https://design.ros2.org/articles/clock_and_time.html).
//!
//! The abstaction Steady Time is not implemented here, because it is not
//! intended to be communicated over interfaces, but kept internal to a
//! component.
use std::{
  convert::TryFrom,
  ops::{Add, Sub},
  time::Duration,
};

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use log::error;
use rustdds::Timestamp;

/// ROS Time with nanosecond precision
///
/// This is the in-memory representation of builtin_interfaces::Time
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Serialize, Deserialize)]
pub struct ROSTime {
  nanos_since_epoch: i64,
}

impl ROSTime {
  /// Returns the current time for the system clock.
  ///
  /// To use simulation-capable time, ask from `Node`.
  pub(crate) fn now() -> Self {
    Self::try_from(chrono::Utc::now()).unwrap_or(Self::ZERO)
  }

  pub const ZERO: Self = Self::from_nanos(0);
  pub const UNIX_EPOCH: Self = Self::from_nanos(0);

  pub fn to_nanos(&self) -> i64 {
    self.nanos_since_epoch
  }

  pub const fn from_nanos(nanos_since_unix_epoch: i64) -> Self {
    Self {
      nanos_since_epoch: nanos_since_unix_epoch,
    }
  }
}

/// Overflow/underflow in timestamp conversion
#[derive(Clone, Debug)]
pub struct OutOfRangeError {}

// chrono <-> ROSTime

/// Fallible conversion to nanoseconds since non-leap-nanoseconds since
/// January 1, 1970 UTC
///
/// chrono docs:
///
/// "An i64 with nanosecond precision can span a range of ~584 years. This
/// function returns None on an out of range DateTime. The dates that can be
/// represented as nanoseconds are between 1677-09-21T00:12:43.145224192 and
/// 2262-04-11T23:47:16.854775807"
impl TryFrom<chrono::DateTime<Utc>> for ROSTime {
  type Error = OutOfRangeError;

  fn try_from(chrono_time: chrono::DateTime<Utc>) -> Result<ROSTime, OutOfRangeError> {
    chrono_time
      .timestamp_nanos_opt()
      .ok_or_else(|| {
        error!("ROSTime: chrono timestamp is out of range: {chrono_time:?}");
        OutOfRangeError {}
      })
      .map(ROSTime::from_nanos)
  }
}

impl From<ROSTime> for chrono::DateTime<Utc> {
  fn from(rt: ROSTime) -> chrono::DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_nanos(rt.to_nanos())
  }
}

// rustDDS::Timestamp <-> ROSTime

impl From<ROSTime> for Timestamp {
  fn from(rt: ROSTime) -> Timestamp {
    let chrono_time = chrono::DateTime::<Utc>::from(rt);
    Timestamp::try_from(chrono_time).unwrap_or_else(|e| {
      error!("Time conversion ROSTime to Timestamp error: {e} source={rt:?}");
      rustdds::Timestamp::INVALID
    })
  }
}

/// Failure to convert DDS/RTPS Timestamp (`Time_t`) to `ROSTime`.
///
/// See DDS Specification v1.4 Sections "2.3.2 PIM to PSM Mapping Rules"
/// and "2.3.3 DCPS PSM : IDL" for DDS `Time_t` type definitions and RTPS
/// Specification v2.5 Section "8.3.2 Type Definitions" for RTPS view of
/// `Time_t`.
pub enum TimestampConversionError {
  /// DDS Timestamp indicates an invalid value
  Invalid,
  /// DDS Timestamp indicates "infinity"  
  Infinite,
}

impl TryFrom<Timestamp> for ROSTime {
  type Error = TimestampConversionError;
  fn try_from(ts: Timestamp) -> Result<ROSTime, TimestampConversionError> {
    match ts {
      Timestamp::INVALID => Err(TimestampConversionError::Invalid),
      Timestamp::INFINITE => Err(TimestampConversionError::Infinite),
      ts => {
        let ticks: u64 = ts.to_ticks(); // tick length is (1 / 2^32) seconds
        let seconds = ticks >> 32;
        let frac_ticks = ticks - (seconds << 32); // fractional part only in ticks
        let frac_nanos = (frac_ticks * 1_000_000_000) >> 32;
        // Both Timestamp and ROSTime are represented as i64, but
        // units are different. Timestamp can count up to 2^32 = 4Gi seconds
        // from epoch, or until year 2106.
        // ROSTime can count up (2^63/10^9) or 9.22*10^9 seconds from epoch,
        // which is in year 2262.
        // Timestamp cannot be negative.
        // Therefore, we cannot overflow the i64 in ROStime.
        Ok(ROSTime::from_nanos(
          (seconds * 1_000_000_000 + frac_nanos) as i64,
        ))
      }
    }
  }
}

impl Sub for ROSTime {
  type Output = ROSDuration;

  fn sub(self, other: ROSTime) -> ROSDuration {
    ROSDuration {
      diff: self.nanos_since_epoch - other.nanos_since_epoch,
    }
  }
}

impl Sub<ROSDuration> for ROSTime {
  type Output = ROSTime;

  fn sub(self, other: ROSDuration) -> ROSTime {
    ROSTime {
      nanos_since_epoch: self.nanos_since_epoch - other.diff,
    }
  }
}

impl Add<ROSDuration> for ROSTime {
  type Output = ROSTime;

  fn add(self, other: ROSDuration) -> ROSTime {
    ROSTime {
      nanos_since_epoch: self.nanos_since_epoch + other.diff,
    }
  }
}

/// Difference between [`ROSTime`] or [`SystemTime`] instances
///
/// Supports conversions to/from
/// * [`std::time::Duration`]
/// * [`chrono::Duration`]
pub struct ROSDuration {
  diff: i64,
}

impl ROSDuration {
  /// Construct from nanosecond count
  pub const fn from_nanos(nanos: i64) -> Self {
    ROSDuration { diff: nanos }
  }

  /// Convert to nanoseconds.
  /// Returns `None` if `i64` would overflow.
  pub const fn to_nanos(&self) -> i64 {
    self.diff
  }
}

// std::time::Duration <-> ROSDuration

impl TryFrom<Duration> for ROSDuration {
  type Error = OutOfRangeError;

  fn try_from(std_duration: Duration) -> Result<Self, Self::Error> {
    let nanos = std_duration.as_nanos();
    if nanos <= (i64::MAX as u128) {
      Ok(ROSDuration { diff: nanos as i64 })
    } else {
      Err(OutOfRangeError {})
    }
  }
}

impl TryFrom<ROSDuration> for Duration {
  type Error = OutOfRangeError;

  fn try_from(ros_duration: ROSDuration) -> Result<Duration, Self::Error> {
    Ok(Duration::from_nanos(
      u64::try_from(ros_duration.to_nanos()).map_err(|_e| OutOfRangeError {})?,
    ))
  }
}

// chrono::Duration <-> ROSDuration

impl From<ROSDuration> for chrono::Duration {
  fn from(d: ROSDuration) -> chrono::Duration {
    chrono::Duration::nanoseconds(d.to_nanos())
  }
}

impl TryFrom<chrono::Duration> for ROSDuration {
  type Error = OutOfRangeError;

  fn try_from(c_duration: chrono::Duration) -> Result<Self, Self::Error> {
    c_duration
      .num_nanoseconds()
      .map(ROSDuration::from_nanos)
      .ok_or(OutOfRangeError {})
  }
}

// Addition and subtraction

/// Note: panics on overflow/underflow like integer arithmetic
impl Add for ROSDuration {
  type Output = ROSDuration;
  fn add(self, other: ROSDuration) -> ROSDuration {
    ROSDuration {
      diff: self.diff + other.diff,
    }
  }
}

/// Note: panics on overflow/underflow like integer arithmetic
impl Sub for ROSDuration {
  type Output = ROSDuration;
  fn sub(self, other: ROSDuration) -> ROSDuration {
    ROSDuration {
      diff: self.diff - other.diff,
    }
  }
}

/// Wall-clock time, the same as [`ROSTime`] except that it is never simulated.
///
/// Use this when you need the actual system clock regardless of any ROS 2
/// `use_sim_time` setting. It shares `ROSTime`'s representation (nanoseconds
/// since the Unix epoch) and conversions.
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Serialize, Deserialize)]
pub struct SystemTime {
  ros_time: ROSTime,
}

impl SystemTime {
  /// The current wall-clock time from the system clock.
  ///
  /// Unlike [`ROSTime::now`](ROSTime), this is always the real system time and
  /// cannot be simulated.
  pub fn now() -> Self {
    SystemTime {
      ros_time: ROSTime::now(),
    }
  }

  pub const ZERO: Self = Self::from_nanos(0);
  pub const UNIX_EPOCH: Self = Self::from_nanos(0);

  pub const fn from_nanos(nanos_since_unix_epoch: i64) -> Self {
    SystemTime {
      ros_time: ROSTime::from_nanos(nanos_since_unix_epoch),
    }
  }

  pub fn to_nanos(&self) -> i64 {
    self.ros_time.to_nanos()
  }
}

// SystemTime <-> ROSTime (same representation)

impl From<SystemTime> for ROSTime {
  fn from(st: SystemTime) -> ROSTime {
    st.ros_time
  }
}

impl From<ROSTime> for SystemTime {
  fn from(ros_time: ROSTime) -> SystemTime {
    SystemTime { ros_time }
  }
}

// SystemTime <-> chrono

impl TryFrom<chrono::DateTime<Utc>> for SystemTime {
  type Error = OutOfRangeError;
  fn try_from(chrono_time: chrono::DateTime<Utc>) -> Result<SystemTime, OutOfRangeError> {
    ROSTime::try_from(chrono_time).map(SystemTime::from)
  }
}

impl From<SystemTime> for chrono::DateTime<Utc> {
  fn from(st: SystemTime) -> chrono::DateTime<Utc> {
    st.ros_time.into()
  }
}

// SystemTime <-> rustdds::Timestamp

impl From<SystemTime> for Timestamp {
  fn from(st: SystemTime) -> Timestamp {
    st.ros_time.into()
  }
}

impl TryFrom<Timestamp> for SystemTime {
  type Error = TimestampConversionError;
  fn try_from(ts: Timestamp) -> Result<SystemTime, TimestampConversionError> {
    ROSTime::try_from(ts).map(SystemTime::from)
  }
}

// SystemTime <-> std::time::SystemTime

impl TryFrom<std::time::SystemTime> for SystemTime {
  type Error = OutOfRangeError;
  fn try_from(st: std::time::SystemTime) -> Result<SystemTime, OutOfRangeError> {
    let nanos = match st.duration_since(std::time::UNIX_EPOCH) {
      Ok(d) => i64::try_from(d.as_nanos()).map_err(|_| OutOfRangeError {})?,
      Err(before_epoch) => {
        -i64::try_from(before_epoch.duration().as_nanos()).map_err(|_| OutOfRangeError {})?
      }
    };
    Ok(SystemTime::from_nanos(nanos))
  }
}

impl TryFrom<SystemTime> for std::time::SystemTime {
  type Error = OutOfRangeError;
  fn try_from(st: SystemTime) -> Result<std::time::SystemTime, OutOfRangeError> {
    let nanos = st.to_nanos();
    if nanos >= 0 {
      std::time::UNIX_EPOCH
        .checked_add(Duration::from_nanos(nanos as u64))
        .ok_or(OutOfRangeError {})
    } else {
      std::time::UNIX_EPOCH
        .checked_sub(Duration::from_nanos(nanos.unsigned_abs()))
        .ok_or(OutOfRangeError {})
    }
  }
}

// Arithmetic mirroring ROSTime

impl Sub for SystemTime {
  type Output = ROSDuration;
  fn sub(self, other: SystemTime) -> ROSDuration {
    self.ros_time - other.ros_time
  }
}

impl Sub<ROSDuration> for SystemTime {
  type Output = SystemTime;
  fn sub(self, other: ROSDuration) -> SystemTime {
    SystemTime {
      ros_time: self.ros_time - other,
    }
  }
}

impl Add<ROSDuration> for SystemTime {
  type Output = SystemTime;
  fn add(self, other: ROSDuration) -> SystemTime {
    SystemTime {
      ros_time: self.ros_time + other,
    }
  }
}

#[cfg(test)]
mod test {
  use super::{ROSTime, SystemTime};

  #[test]
  fn rostime_chrono_roundtrip() {
    for nanos in [0i64, 1, -1, 1_700_000_000_000_000_000] {
      let t = ROSTime::from_nanos(nanos);
      let dt: chrono::DateTime<chrono::Utc> = t.into();
      assert_eq!(ROSTime::try_from(dt).unwrap(), t);
    }
  }

  #[test]
  fn systemtime_now_is_recent() {
    // now() should be a positive, plausible wall-clock value (after 2020).
    let now = SystemTime::now();
    assert!(now.to_nanos() > 1_577_836_800_000_000_000); // 2020-01-01T00:00:00Z
  }

  #[test]
  fn systemtime_std_roundtrip() {
    for nanos in [0i64, 1, 1_500_000_000, 1_700_000_000_000_000_000] {
      let t = SystemTime::from_nanos(nanos);
      let std_t: std::time::SystemTime = t.try_into().unwrap();
      assert_eq!(SystemTime::try_from(std_t).unwrap(), t);
    }
  }
}

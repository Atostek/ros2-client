//! Defines message types `Duration` and `Time`. See [builtin_interfaces](https://index.ros.org/p/builtin_interfaces/)
//!
//!  
//! The name "builtin_interfaces" is not very descriptive, but that is how
//! it is in ROS.
//!
//! Type "Time" in ROS 2 can mean either
//! * `builtin_interfaces::msg::Time`, which is the message type over the wire,
//!   or
//! * `rclcpp::Time`, which is a wrapper for `rcl_time_point_value_t` (in RCL),
//!   which again is a typedef for `rcutils_time_point_value_t`, which is in
//!   package `rclutils` and is a typedef for `int64_t`. Comment specifies this
//!   to be "A single point in time, measured in nanoseconds since the Unix
//!   epoch." This type is used for time-related computations.
//!
//! This module defines the over-the wire `Time` and `Duration` types.
//! The module [`ros_time`] defines types intended for computaiton and
//! in-memory representation.
//!
//! As the over-the wire time representation uses signed 32-bit integer for
//! seconds since the unix epoch, it is susceptible to the
//! [Year 2038 problem](https://en.wikipedia.org/wiki/Year_2038_problem).
//!
//! This implementation uses 64-bit nanosecond count in memory, which will not
//! overflow until the year 2262, but the serialization will saturate in 2038.

use serde::{Deserialize, Serialize};
use log::{error, warn};

use crate::{message::Message, ros_time::ROSTime};

/// Over-the wire representation of a timestamp.
///
/// The recommended constructor is [`From`]-conversion from [`ROSTime`].
///
/// The most useful things to do with these is send in a [`Message`] or
/// convert into a `ROSTime`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(from = "repr::Time", into = "repr::Time")]
pub struct Time {
  /// Nanoseconds since the Unix epoch
  nanos_since_epoch: i64,
}

impl Time {
  pub const ZERO: Time = Time {
    nanos_since_epoch: 0,
  };

  pub const DUMMY: Time = Time {
    nanos_since_epoch: 1234567890123,
  };

  /// Returns the current time for the system clock.
  ///
  /// To use simulation-capable time, ask from `Node`.
  pub(crate) fn now() -> Self {
    chrono::Utc::now()
      .timestamp_nanos_opt()
      .map(Self::from_nanos)
      .unwrap_or_else(|| {
        error!("Timestamp out of range.");
        Time::ZERO // Since we have to return something
                   // But your clock would have to rather far from year 2024 AD
                   // in order to trigger this default.
      })
  }

  pub fn from_nanos(nanos_since_epoch: i64) -> Self {
    Self { nanos_since_epoch }
  }

  pub fn to_nanos(&self) -> i64 {
    self.nanos_since_epoch
  }
}

// Conversions between `Time` and `repr::Time`.
//
// These are non-trivial, because
// the fractional part of `repr::Time`is (by definition) always positive,
// whereas the integer part is signed and may be negative.

impl From<repr::Time> for Time {
  fn from(rt: repr::Time) -> Time {
    // sanity check
    if rt.nanosec >= 1_000_000_000 {
      warn!(
        "builtin_interfaces::Time fractional part at 1 or greater: {} / 10^9 ",
        rt.nanosec
      );
    }

    // But convert in any case
    Time::from_nanos((rt.sec as i64) * 1_000_000_000 + (rt.nanosec as i64))

    // This same conversion formula works for both positive and negative Times.
    //
    // Positive numbers: No surprise, this is what you would expect.
    //
    // Negative: E.g. -1.5 sec is represented as -2 whole and 0.5 *10^9 nanosec
    // fractional. Then we have -2 * 10^9 + 0.5 * 10^9 = -1.5 * 10^9 .
  }
}

// Algorithm from https://github.com/ros2/rclcpp/blob/rolling/rclcpp/src/rclcpp/time.cpp#L278
// function `convert_rcl_time_to_sec_nanos`
impl From<Time> for repr::Time {
  fn from(t: Time) -> repr::Time {
    let t = t.to_nanos();
    let quot = t / 1_000_000_000;
    let rem = t % 1_000_000_000;

    // https://doc.rust-lang.org/reference/expressions/operator-expr.html#arithmetic-and-logical-binary-operators
    // "Rust uses a remainder defined with truncating division.
    // Given remainder = dividend % divisor,
    // the remainder will have the same sign as the dividend."

    if rem >= 0 {
      // positive time, no surprise here
      // OR, negative time, but a whole number of seconds, fractional part is zero
      repr::Time {
        // Saturate seconds to i32. This is different from C++ implementation
        // in rclcpp, which just uses
        // `ret.sec = static_cast<std::int32_t>(result.quot)`.
        sec: if quot > (i32::MAX as i64) {
          warn!("rcl_interfaces::Time conversion overflow");
          i32::MAX
        } else if quot < (i32::MIN as i64) {
          warn!("rcl_interfaces::Time conversion underflow");
          i32::MIN
        } else {
          quot as i32
        },
        nanosec: rem as u32,
      }
    } else {
      // Now `t` is negative AND `rem` is non-zero.
      // We do some non-obvious arithmetic:

      // saturate whole seconds
      let quot_sat = if quot >= (i32::MIN as i64) {
        quot as i32
      } else {
        warn!("rcl_interfaces::Time conversion underflow");
        i32::MIN
      };

      // Now, `rem` is between -999_999_999 and -1, inclusive.
      // Case rem = 0 is included in the positive branch.
      //
      // Adding 1_000_000_000 will make it positive, so cast to u32 is ok.
      //
      // It is also the right thing to do, because
      // * 0.0 sec = 0 sec and 0 nanosec
      // * -0.000_000_001 sec = -1 sec and 999_999_999 nanosec
      // * ...
      // * -0.99999999999 sec = -1 sec and 000_000_001 nanosec
      // * -1.0           sec = -1 sec and 0 nanosec
      // * -1.00000000001 sec = -2 sec and 999_999_999 nanosec
      repr::Time {
        sec: quot_sat - 1, // note -1
        nanosec: (1_000_000_000 + rem) as u32,
      }
    }
  }
}

// This private module defines the wire representation of Time
mod repr {
  use serde::{Deserialize, Serialize};

  use crate::message::Message;

  #[derive(Clone, Copy, Serialize, Deserialize, Debug)]
  pub struct Time {
    pub sec: i32,
    pub nanosec: u32,
  }
  impl Message for Time {}
}

// NOTE:
// This may panic, if the source ROSTime is unreasonably far in the past or
// future. If this is not ok, then TryFrom should be implemented and used.
impl From<ROSTime> for Time {
  fn from(rt: ROSTime) -> Time {
    Time::from_nanos(rt.to_nanos())
  }
}

impl From<Time> for ROSTime {
  fn from(t: Time) -> ROSTime {
    ROSTime::from_nanos(t.to_nanos())
  }
}

// TODO: Implement constructors and conversions to/from usual Rust time formats
// Note that this type does not specify a zero point in time.

// Converting a straight 64-bit nanoseconds value to Duration is non-trivial.
// See function `Duration::operator builtin_interfaces::msg::Duration() const`
// in https://github.com/ros2/rclcpp/blob/rolling/rclcpp/src/rclcpp/duration.cpp
//
// If dividing the raw nanosecond duration by 10^9 would overflow `i32`, then
// saturate to either to {sec = i32::max , nanosec = u32::max} (positive
// overflow) or { sec = i32::min , nanosec = 0 }.
//
// Converting non-negative nanoseconds to Duration is straightforward. Just use
// integer division by 10^9 and store quotient and remainder.
//
// Negative nanoseconds are converted by similar integer divsion, and the result
// is { sec = quotient - 1 , nanosec = 10^9 + remainder}
//
// E.g. -1.5*10^9 nanosec --> quotient = -1 , remainder = -5*10^8
// (We are using division with invariant: quotient * divisor + remainder ==
// dividend ) Now { sec = -2 , nanosec = +5 * 10^8 }
//
// -1 nanosec --> quotient = 0, remainder = -1 -->
// { sec = -1 , nanosec = 999_999_999 }

/// Over-the wire representation of Duration, i.e. difference between two
/// timestamps.
///
/// To actually compute a time difference, use types [`ROSTime`] and
/// [`ROSDuration`](crate::ros_time::ROSDuration), and convert to [`Duration`]
/// for sending in a [`Message`].
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Duration {
  sec: i32,     // ROS2: Seconds component, range is valid over any possible int32 value.
  nanosec: u32, /* ROS2:  Nanoseconds component in the range of [0, 10e9). */
}
impl Message for Duration {}

impl Duration {
  pub const fn zero() -> Self {
    Self { sec: 0, nanosec: 0 }
  }

  pub const fn from_secs(sec: i32) -> Self {
    Self { sec, nanosec: 0 }
  }

  pub const fn from_millis(millis: i64) -> Self {
    let nanos = millis * 1_000_000; // Maybe overflow, but result will also.
    Self::from_nanos(nanos)
  }

  pub const fn from_nanos(nanos: i64) -> Self {
    // This algorithm is from
    // https://github.com/ros2/rclcpp/blob/ea8daa37845e6137cba07a18eb653d97d87e6174/rclcpp/src/rclcpp/duration.cpp
    // lines 61-88

    // Except that we also test for quot underflow in case rem == 0

    let quot = nanos / 1_000_000_000;
    let rem = nanos % 1_000_000_000;
    // Rust `%` is the remainder operator.
    // If rem is negative, so is nanos
    if rem >= 0 {
      // positive or zero duration
      if quot > (i32::MAX as i64) {
        // overflow => saturate to max
        Duration {
          sec: i32::MAX,
          nanosec: u32::MAX,
        }
      } else if quot <= (i32::MIN as i64) {
        // underflow => saturate to min
        Duration {
          sec: i32::MIN,
          nanosec: 0,
        }
      } else {
        // normal case
        Duration {
          sec: quot as i32,
          nanosec: rem as u32,
        }
        // as-conversions will succeed: we know 0 <= quot <= i32::MAX, and
        // also 0 <= rem <= 1_000_000_000
      }
    } else {
      // duration was negative
      if quot <= (i32::MIN as i64) {
        // underflow => saturate to min
        Duration {
          sec: i32::MIN,
          nanosec: 0,
        }
      } else {
        // normal negative result
        Duration {
          sec: (quot + 1) as i32,
          nanosec: (1_000_000_000 + rem) as u32,
        }
        // i32::MIN <= quot < 0 => quot+1 is valid i32
        // -999_999_999 <= rem < 0 =>
        // 1 <= 1_000_000_000 + rem < 1_000_000_000 => valid u32
      }
    }
  }

  pub fn to_nanos(&self) -> i64 {
    let s = self.sec as i64;
    let ns = self.nanosec as i64;

    1_000_000_000 * s + ns
  }
}

#[cfg(test)]
mod test {
  use super::{repr, Time};

  fn repr_conv_test(t: Time) {
    let rt: repr::Time = t.into();
    println!("{rt:?}");
    assert_eq!(t, Time::from(rt))
  }

  #[test]
  fn repr_conversion() {
    repr_conv_test(Time::from_nanos(999_999_999));
    repr_conv_test(Time::from_nanos(1_000_000_000));
    repr_conv_test(Time::from_nanos(1_000_000_001));

    repr_conv_test(Time::from_nanos(1_999_999_999));
    repr_conv_test(Time::from_nanos(2_000_000_000));
    repr_conv_test(Time::from_nanos(2_000_000_001));

    repr_conv_test(Time::from_nanos(-999_999_999));
    repr_conv_test(Time::from_nanos(-1_000_000_000));
    repr_conv_test(Time::from_nanos(-1_000_000_001));

    repr_conv_test(Time::from_nanos(-1_999_999_999));
    repr_conv_test(Time::from_nanos(-2_000_000_000));
    repr_conv_test(Time::from_nanos(-2_000_000_001));

    repr_conv_test(Time::from_nanos(0));
    repr_conv_test(Time::from_nanos(1));
    repr_conv_test(Time::from_nanos(-1));
  }
}

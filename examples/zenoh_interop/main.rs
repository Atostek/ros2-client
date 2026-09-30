//! Finite Zenoh roles for interop against ROS 2 Jazzy + `rmw_zenoh`.
//!
//! Unlike [`zenoh_demo`](../zenoh_demo/main.rs), this process opens
//! [`Context::new`], so `ZENOH_SESSION_CONFIG_URI` and `ZENOH_CONFIG_OVERRIDE`
//! apply (point those at `rmw_zenohd`). Each subcommand exits on its own.
//!
//! ```console
//! cargo run --no-default-features --features zenoh --example zenoh_interop -- talker --count 5
//! ```
//!
//! The shell harness is `interop/zenoh/run_all.sh`.

use std::{
  collections::BTreeMap,
  process::ExitCode,
  thread::sleep,
  time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use ros2_client::{
  ActionTypeName, Context, MessageTypeName, Name, Node, NodeName, NodeOptions, QosProfile,
  ServiceTypeName,
  action_msgs::{CancelGoalResponseEnum, GoalInfo, GoalStatus, GoalStatusEnum},
  builtin_interfaces::Time,
  ros2::LogLevel,
  rosout,
};

fn main() -> ExitCode {
  let mut args = std::env::args().skip(1);
  let Some(cmd) = args.next() else {
    usage();
    return ExitCode::from(2);
  };
  let result = match cmd.as_str() {
    "talker" => {
      let (count, timeout) = flags(args, 30, 30);
      talker(count, timeout)
    }
    "listener" => {
      let (count, timeout) = flags(args, 1, 20);
      listener(count, timeout)
    }
    "service-server" => {
      let (_, timeout) = flags(args, 1, 30);
      service_server(timeout)
    }
    "service-client" => {
      let (_, timeout) = flags(args, 1, 20);
      service_client(timeout)
    }
    "action-server" => {
      let (_, timeout) = flags(args, 1, 60);
      action_server(timeout)
    }
    "action-client" => {
      let (_, timeout) = flags(args, 1, 45);
      action_client(timeout)
    }
    "params" => {
      let (_, timeout) = flags(args, 1, 30);
      params(timeout)
    }
    "rosout" => {
      let (_, timeout) = flags(args, 1, 20);
      rosout_once(timeout)
    }
    "help" | "-h" | "--help" => {
      usage();
      Ok(())
    }
    other => {
      eprintln!("unknown command {other}");
      usage();
      Err("usage".into())
    }
  };
  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      if err != "usage" {
        eprintln!("zenoh_interop: {err}");
      }
      ExitCode::from(1)
    }
  }
}

fn usage() {
  eprintln!(
    "\
usage: zenoh_interop <command> [--count N] [--timeout SECS]

  talker           publish std_msgs/String on /chatter
  listener         subscribe /chatter, print message len=
  service-server   example_interfaces/AddTwoInts on /add_two_ints
  service-client   call /add_two_ints with a=2 b=40
  action-server    action_tutorials_interfaces/Fibonacci on /fibonacci
  action-client    send Fibonacci goal order 5
  params           node /param_holder, parameter speed=1.0
  rosout           publish one Info line on /rosout

Opens Context::new() (honours ZENOH_CONFIG_OVERRIDE)."
  );
}

fn flags(
  args: impl Iterator<Item = String>,
  mut count: u32,
  mut timeout_secs: u64,
) -> (u32, Duration) {
  let args: Vec<String> = args.collect();
  let mut i = 0;
  while i < args.len() {
    match args[i].as_str() {
      "--count" => {
        i += 1;
        count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
          eprintln!("--count needs a number");
          std::process::exit(2);
        });
      }
      "--timeout" => {
        i += 1;
        timeout_secs = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
          eprintln!("--timeout needs a number of seconds");
          std::process::exit(2);
        });
      }
      other => {
        eprintln!("unknown flag {other}");
        std::process::exit(2);
      }
    }
    i += 1;
  }
  (count, Duration::from_secs(timeout_secs))
}

fn open_node(name: &str, options: NodeOptions) -> Result<Node, String> {
  let ctx = Context::new().map_err(|e| format!("open context: {e}"))?;
  ctx
    .new_node(
      NodeName::new("/", name).map_err(|e| e.to_string())?,
      options,
    )
    .map_err(|e| format!("create node {name}: {e}"))
}

fn quiet_node(name: &str) -> Result<Node, String> {
  // Parameter services stay on (no public switch). Rosout is off so only the
  // `rosout` subcommand publishes /rosout.
  open_node(name, NodeOptions::new().enable_rosout(false))
}

fn talker(count: u32, timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_talker")?;
  let topic = node.create_topic(
    &Name::new("/", "chatter").map_err(|e| e.to_string())?,
    MessageTypeName::new("std_msgs", "String"),
    &QosProfile::publisher_default(),
  );
  let publisher = node
    .create_publisher::<String>(&topic, None)
    .map_err(|e| format!("create publisher: {e}"))?;
  let deadline = Instant::now() + timeout;
  for n in 1..=count {
    if Instant::now() >= deadline {
      break;
    }
    let message = format!("count={n} hello-zenoh");
    publisher
      .publish(message)
      .map_err(|e| format!("publish: {e}"))?;
    println!("Talking, count={n}");
    sleep_until(Duration::from_millis(500), deadline);
  }
  Ok(())
}

fn listener(count: u32, timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_listener")?;
  let topic = node.create_topic(
    &Name::new("/", "chatter").map_err(|e| e.to_string())?,
    MessageTypeName::new("std_msgs", "String"),
    &QosProfile::publisher_default(),
  );
  let subscription = node
    .create_subscription::<String>(&topic, None)
    .map_err(|e| format!("create subscription: {e}"))?;
  let deadline = Instant::now() + timeout;
  let mut got = 0u32;
  while got < count && Instant::now() < deadline {
    match subscription.try_take() {
      Ok(Some((data, _info))) => {
        got += 1;
        println!("message len={}", data.len());
      }
      Ok(None) => sleep(Duration::from_millis(50)),
      Err(e) => return Err(format!("take: {e}")),
    }
  }
  if got < count {
    return Err(format!("heard {got} of {count} messages"));
  }
  Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct AddTwoIntsRequest {
  a: i64,
  b: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AddTwoIntsResponse {
  sum: i64,
}

fn service_server(timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_add_two_ints_server")?;
  let server = node
    .create_server::<AddTwoIntsRequest, AddTwoIntsResponse>(
      &Name::new("/", "add_two_ints").map_err(|e| e.to_string())?,
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
    )
    .map_err(|e| format!("create server: {e}"))?;
  println!("service-server ready");
  let deadline = Instant::now() + timeout;
  while Instant::now() < deadline {
    match server.try_receive_request() {
      Ok(Some((id, req))) => {
        let sum = req.a + req.b;
        server
          .send_response(id, AddTwoIntsResponse { sum })
          .map_err(|e| format!("send response: {e}"))?;
        println!("result of {} + {} = {sum}", req.a, req.b);
        return Ok(());
      }
      Ok(None) => sleep(Duration::from_millis(20)),
      Err(e) => return Err(format!("receive request: {e}")),
    }
  }
  Err("no AddTwoInts request before timeout".into())
}

fn service_client(timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_add_two_ints_client")?;
  let client = node
    .create_client::<AddTwoIntsRequest, AddTwoIntsResponse>(
      &Name::new("/", "add_two_ints").map_err(|e| e.to_string())?,
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
    )
    .map_err(|e| format!("create client: {e}"))?;
  let deadline = Instant::now() + timeout;
  let response = loop {
    if Instant::now() >= deadline {
      return Err("AddTwoInts call timed out".into());
    }
    match client.call(AddTwoIntsRequest { a: 2, b: 40 }) {
      Ok(resp) => break resp,
      Err(_) => sleep(Duration::from_millis(200)),
    }
  };
  println!("Response received: sum={}", response.sum);
  if response.sum != 42 {
    return Err(format!("expected sum 42, got {}", response.sum));
  }
  Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
struct FibGoal {
  order: i32,
}

#[derive(Clone, Serialize, Deserialize)]
struct FibResult {
  sequence: Vec<i32>,
}

/// CDR is positional. The IDL field is `partial_sequence`.
#[derive(Clone, Serialize, Deserialize)]
struct FibFeedback {
  sequence: Vec<i32>,
}

/// Official `action_tutorials` sequence: start `[0, 1]`, then `order - 1`
/// steps. Order 5 is `[0, 1, 1, 2, 3, 5]`.
fn fibonacci(order: i32) -> Vec<i32> {
  let mut sequence = vec![0, 1];
  let steps = order.max(0);
  for i in 1..steps {
    let i = i as usize;
    let next = sequence[i] + sequence[i - 1];
    sequence.push(next);
  }
  sequence
}

fn action_server(timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_fibonacci_server")?;
  let server = node
    .create_action_server::<FibGoal, FibResult, FibFeedback>(
      &Name::new("/", "fibonacci").map_err(|e| e.to_string())?,
      &ActionTypeName::new("action_tutorials_interfaces", "Fibonacci"),
    )
    .map_err(|e| format!("create action server: {e}"))?;
  println!("action-server ready");

  let deadline = Instant::now() + timeout;
  let mut statuses: BTreeMap<ros2_client::GoalId, GoalStatusEnum> = BTreeMap::new();
  let mut results: BTreeMap<ros2_client::GoalId, Vec<i32>> = BTreeMap::new();
  let mut pending_results: Vec<(ros2_client::RmwRequestId, ros2_client::GoalId)> = Vec::new();
  let mut completed = false;

  while Instant::now() < deadline {
    if let Some((id, goal_id, goal)) = server.try_receive_goal() {
      server
        .respond_goal(id, true)
        .map_err(|e| format!("respond goal: {e}"))?;
      statuses.insert(goal_id, GoalStatusEnum::Executing);
      let mut sequence = vec![0, 1];
      let _ = server.publish_feedback(
        goal_id,
        FibFeedback {
          sequence: sequence.clone(),
        },
      );
      println!("<<< Feedback: {sequence:?}");
      let steps = goal.order.max(0);
      let mut canceled = false;
      for i in 1..steps {
        if take_cancel(&server, &mut statuses) {
          canceled = true;
          break;
        }
        let i = i as usize;
        sequence.push(sequence[i] + sequence[i - 1]);
        let _ = server.publish_feedback(
          goal_id,
          FibFeedback {
            sequence: sequence.clone(),
          },
        );
        println!("<<< Feedback: {sequence:?}");
        sleep_until(Duration::from_millis(200), deadline);
      }
      statuses.insert(
        goal_id,
        if canceled {
          GoalStatusEnum::Canceled
        } else {
          GoalStatusEnum::Succeeded
        },
      );
      results.insert(goal_id, sequence);
    }

    if let Some((id, goal_id)) = server.try_receive_cancel() {
      apply_cancel(&server, id, goal_id, &mut statuses)?;
    }

    if let Some(req) = server.try_receive_result_request() {
      pending_results.push(req);
    }
    let mut answered = false;
    pending_results.retain(|(id, goal_id)| match results.get(goal_id).cloned() {
      Some(sequence) => {
        let status = statuses
          .get(goal_id)
          .copied()
          .unwrap_or(GoalStatusEnum::Succeeded);
        let _ = server.respond_result(
          *id,
          status as i8,
          FibResult {
            sequence: sequence.clone(),
          },
        );
        println!("<<< Action Result: {sequence:?}");
        answered = true;
        false
      }
      None => true,
    });
    if answered {
      completed = true;
    }

    let list: Vec<GoalStatus> = statuses
      .iter()
      .map(|(goal_id, status)| GoalStatus {
        goal_info: GoalInfo {
          goal_id: *goal_id,
          stamp: Time::ZERO,
        },
        status: *status,
      })
      .collect();
    let _ = server.publish_status(list);

    if completed {
      return Ok(());
    }
    sleep(Duration::from_millis(20));
  }
  Err("no Fibonacci result sent before timeout".into())
}

fn take_cancel(
  server: &ros2_client::ActionServer<FibGoal, FibResult, FibFeedback>,
  statuses: &mut BTreeMap<ros2_client::GoalId, GoalStatusEnum>,
) -> bool {
  if let Some((id, goal_id)) = server.try_receive_cancel() {
    let _ = apply_cancel(server, id, goal_id, statuses);
    return true;
  }
  false
}

fn apply_cancel(
  server: &ros2_client::ActionServer<FibGoal, FibResult, FibFeedback>,
  id: ros2_client::RmwRequestId,
  goal_id: ros2_client::GoalId,
  statuses: &mut BTreeMap<ros2_client::GoalId, GoalStatusEnum>,
) -> Result<(), String> {
  let zero = ros2_client::unique_identifier_msgs::UUID::ZERO;
  let mut canceling = Vec::new();
  if goal_id == zero {
    for (gid, status) in statuses.iter_mut() {
      *status = GoalStatusEnum::Canceled;
      canceling.push(GoalInfo {
        goal_id: *gid,
        stamp: Time::ZERO,
      });
    }
  } else if let Some(status) = statuses.get_mut(&goal_id) {
    *status = GoalStatusEnum::Canceled;
    canceling.push(GoalInfo {
      goal_id,
      stamp: Time::ZERO,
    });
  }
  let code = if canceling.is_empty() {
    CancelGoalResponseEnum::UnknownGoal
  } else {
    CancelGoalResponseEnum::None
  };
  server
    .respond_cancel(id, code, canceling)
    .map_err(|e| format!("respond cancel: {e}"))
}

fn action_client(timeout: Duration) -> Result<(), String> {
  let node = quiet_node("zenoh_fibonacci_client")?;
  let client = node
    .create_action_client::<FibGoal, FibResult, FibFeedback>(
      &Name::new("/", "fibonacci").map_err(|e| e.to_string())?,
      &ActionTypeName::new("action_tutorials_interfaces", "Fibonacci"),
    )
    .map_err(|e| format!("create action client: {e}"))?;
  let deadline = Instant::now() + timeout;
  let goal_id = loop {
    if Instant::now() >= deadline {
      return Err("Fibonacci goal was not accepted".into());
    }
    match client.send_goal(FibGoal { order: 5 }) {
      Ok((id, true)) => break id,
      _ => sleep(Duration::from_millis(200)),
    }
  };
  let (status, result) = loop {
    if Instant::now() >= deadline {
      return Err("Fibonacci result timed out".into());
    }
    match client.get_result(goal_id) {
      Ok(res) => break res,
      Err(_) => sleep(Duration::from_millis(200)),
    }
  };
  while let Some((_id, feedback)) = client.take_feedback() {
    println!("<<< Feedback: {:?}", feedback.sequence);
  }
  println!("<<< Action Result: {:?}", result.sequence);
  let expected = fibonacci(5);
  if status != GoalStatusEnum::Succeeded as i8 || result.sequence != expected {
    return Err(format!(
      "expected status {} sequence {expected:?}, got status {status} {:?}",
      GoalStatusEnum::Succeeded as i8,
      result.sequence
    ));
  }
  Ok(())
}

fn params(timeout: Duration) -> Result<(), String> {
  let node = open_node(
    "param_holder",
    NodeOptions::new()
      .enable_rosout(false)
      .declare_parameter("speed", ros2_client::ParameterValue::Double(1.0)),
  )?;
  let server = node
    .parameter_server()
    .ok_or("parameter server was not started")?;
  println!("param_holder ready speed=1.0");
  let deadline = Instant::now() + timeout;
  server.spin(Duration::from_millis(50), || Instant::now() >= deadline);
  Ok(())
}

fn rosout_once(timeout: Duration) -> Result<(), String> {
  let node = open_node("zenoh_rosout", NodeOptions::new().enable_rosout(true))?;
  let logger = node.logger().ok_or("logger was not started")?;
  // Repeat the line: a late `ros2 topic echo --once` does not get history.
  let deadline = Instant::now() + timeout;
  let mut first = true;
  while Instant::now() < deadline {
    rosout!(logger, LogLevel::Info, "rosout interop line");
    if first {
      println!("rosout published");
      first = false;
    }
    sleep_until(Duration::from_millis(500), deadline);
  }
  Ok(())
}

fn sleep_until(step: Duration, deadline: Instant) {
  let end = Instant::now() + step;
  let end = if end > deadline { deadline } else { end };
  if let Some(left) = end.checked_duration_since(Instant::now()) {
    sleep(left);
  }
}

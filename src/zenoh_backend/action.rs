//! Zenoh actions (E7).
//!
//! ROS 2 actions have no middleware-level concept — they are composed of
//! ordinary services + topics (see `docs/zenoh_study/research/rmw_zenoh.md`
//! §8). This module builds that composition on top of the Zenoh service
//! ([`super::service`]) and pub/sub ([`super::pubsub`]) layers:
//!
//! * `send_goal` service — `SendGoalRequest<G>` → `SendGoalResponse`
//! * `get_result` service — `GetResultRequest` → `GetResultResponse<R>`
//!   (queried with a long timeout, mirroring rmw_zenoh's `_action/get_result`)
//! * `feedback` topic — `FeedbackMessage<F>`
//!
//! Goals are correlated by an application-level `GoalId` (a random UUID),
//! exactly as in the DDS backend. Alongside send_goal/get_result/feedback, the
//! module also wires the `cancel_goal` service (`action_msgs/srv/CancelGoal`)
//! and the `status` topic (`action_msgs/msg/GoalStatusArray`), completing the
//! ROS 2 action surface.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{
  pubsub::{PublishError, Publisher, Subscription},
  service::{Client, RmwRequestId, Server, ServiceError},
};
use crate::{
  action_msgs::{
    CancelGoalRequest, CancelGoalResponse, CancelGoalResponseEnum, GoalInfo, GoalStatus,
    GoalStatusArray,
  },
  builtin_interfaces::Time,
  unique_identifier_msgs::UUID,
};

/// Application-level goal identifier.
pub type GoalId = UUID;

/// `action_msgs/GoalStatus` status codes.
pub mod goal_status {
  /// The goal is currently being executed.
  pub const EXECUTING: i8 = 2;
  /// The goal completed successfully.
  pub const SUCCEEDED: i8 = 4;
  /// The goal was aborted.
  pub const ABORTED: i8 = 5;
  /// The goal was canceled.
  pub const CANCELED: i8 = 6;
}

/// `<Action>_SendGoal_Request` — a goal plus its id.
#[derive(Serialize, Deserialize)]
pub struct SendGoalRequest<G> {
  /// Goal identifier.
  pub goal_id: GoalId,
  /// The goal payload.
  pub goal: G,
}

/// `<Action>_SendGoal_Response` — whether the goal was accepted.
#[derive(Serialize, Deserialize)]
pub struct SendGoalResponse {
  /// Whether the server accepted the goal.
  pub accepted: bool,
  /// Server timestamp when the decision was made.
  pub stamp: Time,
}

/// `<Action>_GetResult_Request`.
#[derive(Serialize, Deserialize)]
pub struct GetResultRequest {
  /// The goal whose result is requested.
  pub goal_id: GoalId,
}

/// `<Action>_GetResult_Response` — terminal status + result.
#[derive(Serialize, Deserialize)]
pub struct GetResultResponse<R> {
  /// Terminal goal status (see [`goal_status`]).
  pub status: i8,
  /// The result payload.
  pub result: R,
}

/// `<Action>_FeedbackMessage` — feedback for a goal.
#[derive(Serialize, Deserialize)]
pub struct FeedbackMessage<F> {
  /// The goal this feedback belongs to.
  pub goal_id: GoalId,
  /// The feedback payload.
  pub feedback: F,
}

/// An action client: send goals, receive feedback, fetch results, cancel
/// goals, and watch status.
pub struct ActionClient<G, R, F> {
  send_goal: Client<SendGoalRequest<G>, SendGoalResponse>,
  get_result: Client<GetResultRequest, GetResultResponse<R>>,
  feedback: Subscription<FeedbackMessage<F>>,
  cancel_goal: Client<CancelGoalRequest, CancelGoalResponse>,
  status: Subscription<GoalStatusArray>,
}

impl<G: Serialize, R: DeserializeOwned, F: DeserializeOwned> ActionClient<G, R, F> {
  pub(crate) fn new(
    send_goal: Client<SendGoalRequest<G>, SendGoalResponse>,
    get_result: Client<GetResultRequest, GetResultResponse<R>>,
    feedback: Subscription<FeedbackMessage<F>>,
    cancel_goal: Client<CancelGoalRequest, CancelGoalResponse>,
    status: Subscription<GoalStatusArray>,
  ) -> Self {
    Self {
      send_goal,
      get_result,
      feedback,
      cancel_goal,
      status,
    }
  }

  /// Send a goal (a fresh random `GoalId` is generated). Returns the id and
  /// whether the server accepted it.
  pub fn send_goal(&self, goal: G) -> Result<(GoalId, bool), ServiceError> {
    let goal_id = UUID::new_random();
    let resp = self.send_goal.call(SendGoalRequest { goal_id, goal })?;
    Ok((goal_id, resp.accepted))
  }

  /// Fetch the result for a goal (blocks until the server responds).
  pub fn get_result(&self, goal_id: GoalId) -> Result<(i8, R), ServiceError> {
    let resp = self.get_result.call(GetResultRequest { goal_id })?;
    Ok((resp.status, resp.result))
  }

  /// Take a feedback message if one is immediately available.
  pub fn take_feedback(&self) -> Option<(GoalId, F)> {
    match self.feedback.try_take() {
      Ok(Some((msg, _info))) => Some((msg.goal_id, msg.feedback)),
      _ => None,
    }
  }

  /// Request cancellation of a single goal by id (blocks for the server's
  /// [`CancelGoalResponse`]).
  pub fn cancel_goal(&self, goal_id: GoalId) -> Result<CancelGoalResponse, ServiceError> {
    self.cancel_goal.call(CancelGoalRequest {
      goal_info: GoalInfo {
        goal_id,
        stamp: Time::ZERO,
      },
    })
  }

  /// Request cancellation of all goals (zero goal id + zero stamp, per the
  /// `action_msgs/srv/CancelGoal` policy).
  pub fn cancel_all_goals(&self) -> Result<CancelGoalResponse, ServiceError> {
    self.cancel_goal.call(CancelGoalRequest {
      goal_info: GoalInfo {
        goal_id: UUID::ZERO,
        stamp: Time::ZERO,
      },
    })
  }

  /// Take the latest goal-status array if one is immediately available.
  pub fn take_status(&self) -> Option<GoalStatusArray> {
    match self.status.try_take() {
      Ok(Some((msg, _info))) => Some(msg),
      _ => None,
    }
  }
}

/// An action server: accept goals, publish feedback, answer result requests.
///
/// This exposes the primitives; the goal state machine (accept/execute/succeed)
/// is driven by the application, as in `ros2-client`'s DDS action server.
pub struct ActionServer<G, R, F> {
  send_goal: Server<SendGoalRequest<G>, SendGoalResponse>,
  get_result: Server<GetResultRequest, GetResultResponse<R>>,
  feedback: Publisher<FeedbackMessage<F>>,
  cancel_goal: Server<CancelGoalRequest, CancelGoalResponse>,
  status: Publisher<GoalStatusArray>,
}

impl<G: DeserializeOwned, R: Serialize, F: Serialize> ActionServer<G, R, F> {
  pub(crate) fn new(
    send_goal: Server<SendGoalRequest<G>, SendGoalResponse>,
    get_result: Server<GetResultRequest, GetResultResponse<R>>,
    feedback: Publisher<FeedbackMessage<F>>,
    cancel_goal: Server<CancelGoalRequest, CancelGoalResponse>,
    status: Publisher<GoalStatusArray>,
  ) -> Self {
    Self {
      send_goal,
      get_result,
      feedback,
      cancel_goal,
      status,
    }
  }

  /// Take a pending goal request if available: `(request id, goal id, goal)`.
  pub fn try_receive_goal(&self) -> Option<(RmwRequestId, GoalId, G)> {
    match self.send_goal.try_receive_request() {
      Ok(Some((id, req))) => Some((id, req.goal_id, req.goal)),
      _ => None,
    }
  }

  /// Respond to a goal request (accept/reject).
  pub fn respond_goal(&self, id: RmwRequestId, accepted: bool) -> Result<(), ServiceError> {
    self.send_goal.send_response(
      id,
      SendGoalResponse {
        accepted,
        stamp: Time::ZERO,
      },
    )
  }

  /// Publish feedback for a goal.
  pub fn publish_feedback(&self, goal_id: GoalId, feedback: F) -> Result<(), PublishError> {
    self.feedback.publish(FeedbackMessage { goal_id, feedback })
  }

  /// Take a pending result request if available: `(request id, goal id)`.
  pub fn try_receive_result_request(&self) -> Option<(RmwRequestId, GoalId)> {
    match self.get_result.try_receive_request() {
      Ok(Some((id, req))) => Some((id, req.goal_id)),
      _ => None,
    }
  }

  /// Respond to a result request with the terminal status and result.
  pub fn respond_result(
    &self,
    id: RmwRequestId,
    status: i8,
    result: R,
  ) -> Result<(), ServiceError> {
    self
      .get_result
      .send_response(id, GetResultResponse { status, result })
  }

  /// Take a pending cancel request if available: `(request id, goal id)`.
  /// A zero goal id means "cancel all" (see `action_msgs/srv/CancelGoal`).
  pub fn try_receive_cancel(&self) -> Option<(RmwRequestId, GoalId)> {
    match self.cancel_goal.try_receive_request() {
      Ok(Some((id, req))) => Some((id, req.goal_info.goal_id)),
      _ => None,
    }
  }

  /// Respond to a cancel request with a return code and the goals now
  /// transitioning to CANCELING.
  pub fn respond_cancel(
    &self,
    id: RmwRequestId,
    return_code: CancelGoalResponseEnum,
    goals_canceling: Vec<GoalInfo>,
  ) -> Result<(), ServiceError> {
    self.cancel_goal.send_response(
      id,
      CancelGoalResponse {
        return_code,
        goals_canceling,
      },
    )
  }

  /// Publish the current goal-status array on the `status` topic.
  pub fn publish_status(&self, status_list: Vec<GoalStatus>) -> Result<(), PublishError> {
    self.status.publish(GoalStatusArray { status_list })
  }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use std::{
    collections::BTreeMap,
    sync::{
      atomic::{AtomicBool, Ordering},
      Arc,
    },
    time::{Duration, Instant},
  };

  use serde::{Deserialize, Serialize};
  use zenoh::Config;

  use super::{goal_status, ActionClient, ActionServer, GoalId};
  use crate::{Context, ContextOptions, Name, NodeName, NodeOptions};

  #[derive(Serialize, Deserialize)]
  struct FibGoal {
    order: i32,
  }
  #[derive(Serialize, Deserialize)]
  struct FibResult {
    sequence: Vec<i32>,
  }
  #[derive(Serialize, Deserialize)]
  struct FibFeedback {
    sequence: Vec<i32>,
  }

  fn make_config(listen_port: u16, connect_port: Option<u16>) -> Config {
    let mut c = Config::default();
    c.insert_json5("mode", "\"peer\"").unwrap();
    c.insert_json5("scouting/multicast/enabled", "false")
      .unwrap();
    c.insert_json5(
      "listen/endpoints",
      &format!("[\"tcp/127.0.0.1:{listen_port}\"]"),
    )
    .unwrap();
    if let Some(p) = connect_port {
      c.insert_json5("connect/endpoints", &format!("[\"tcp/127.0.0.1:{p}\"]"))
        .unwrap();
    }
    c
  }

  fn fibonacci(order: i32) -> Vec<i32> {
    let mut seq = vec![0, 1];
    for i in 2..order.max(2) as usize {
      let next = seq[i - 1] + seq[i - 2];
      seq.push(next);
    }
    seq
  }

  #[test]
  fn fibonacci_action_roundtrip() {
    use crate::ActionTypeName;

    let srv_port = 17521;
    let cli_port = 17522;
    let srv_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(srv_port, None)))
        .unwrap();
    let cli_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(cli_port, Some(srv_port))),
    )
    .unwrap();

    let srv_node = srv_ctx.new_node(
      NodeName::new("/", "fib_server").unwrap(),
      NodeOptions::new(),
    );
    let cli_node = cli_ctx.new_node(
      NodeName::new("/", "fib_client").unwrap(),
      NodeOptions::new(),
    );
    let atype = ActionTypeName::new("action_tutorials_interfaces", "Fibonacci");
    let name = Name::new("/", "fibonacci").unwrap();

    let server: ActionServer<FibGoal, FibResult, FibFeedback> =
      srv_node.create_action_server(&name, &atype).unwrap();
    let client: ActionClient<FibGoal, FibResult, FibFeedback> =
      cli_node.create_action_client(&name, &atype).unwrap();

    // Server: accept goals, compute+publish feedback, answer result requests.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = stop.clone();
    let server_thread = std::thread::spawn(move || {
      let deadline = Instant::now() + Duration::from_secs(30);
      let mut results: BTreeMap<GoalId, Vec<i32>> = BTreeMap::new();
      let mut pending: Vec<(crate::RmwRequestId, GoalId)> = Vec::new();
      while !stop_srv.load(Ordering::Relaxed) && Instant::now() < deadline {
        if let Some((id, goal_id, goal)) = server.try_receive_goal() {
          let _ = server.respond_goal(id, true);
          let seq = fibonacci(goal.order);
          let _ = server.publish_feedback(
            goal_id,
            FibFeedback {
              sequence: seq.clone(),
            },
          );
          results.insert(goal_id, seq);
        }
        if let Some((id, goal_id)) = server.try_receive_result_request() {
          pending.push((id, goal_id));
        }
        pending.retain(|(id, goal_id)| match results.get(goal_id) {
          Some(seq) => {
            let _ = server.respond_result(
              *id,
              goal_status::SUCCEEDED,
              FibResult {
                sequence: seq.clone(),
              },
            );
            false
          }
          None => true,
        });
        std::thread::sleep(Duration::from_millis(20));
      }
    });

    // Client: send a goal (retry until accepted), then fetch the result.
    let deadline = Instant::now() + Duration::from_secs(30);
    let goal_id = loop {
      assert!(Instant::now() < deadline, "goal never accepted");
      match client.send_goal(FibGoal { order: 5 }) {
        Ok((gid, true)) => break gid,
        _ => std::thread::sleep(Duration::from_millis(200)),
      }
    };

    let mut outcome = None;
    while Instant::now() < deadline {
      if let Ok(res) = client.get_result(goal_id) {
        outcome = Some(res);
        break;
      }
      std::thread::sleep(Duration::from_millis(200));
    }

    stop.store(true, Ordering::Relaxed);
    let _ = server_thread.join();

    let (status, result) = outcome.expect("no result received");
    assert_eq!(status, goal_status::SUCCEEDED);
    assert_eq!(result.sequence, vec![0, 1, 1, 2, 3]);
  }

  #[test]
  fn cancel_and_status_roundtrip() {
    use crate::{
      action_msgs::{CancelGoalResponseEnum, GoalInfo, GoalStatus, GoalStatusEnum},
      builtin_interfaces::Time,
      ActionTypeName,
    };

    let srv_port = 17529;
    let cli_port = 17530;
    let srv_ctx =
      Context::with_options(ContextOptions::new().zenoh_config(make_config(srv_port, None)))
        .unwrap();
    let cli_ctx = Context::with_options(
      ContextOptions::new().zenoh_config(make_config(cli_port, Some(srv_port))),
    )
    .unwrap();

    let srv_node = srv_ctx.new_node(
      NodeName::new("/", "fib_server").unwrap(),
      NodeOptions::new(),
    );
    let cli_node = cli_ctx.new_node(
      NodeName::new("/", "fib_client").unwrap(),
      NodeOptions::new(),
    );
    let atype = ActionTypeName::new("action_tutorials_interfaces", "Fibonacci");
    let name = Name::new("/", "fibonacci").unwrap();

    let server: ActionServer<FibGoal, FibResult, FibFeedback> =
      srv_node.create_action_server(&name, &atype).unwrap();
    let client: ActionClient<FibGoal, FibResult, FibFeedback> =
      cli_node.create_action_client(&name, &atype).unwrap();

    // Server: track goal statuses, publish the array each tick, and honour
    // cancel requests by moving the goal to Canceled.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = stop.clone();
    let server_thread = std::thread::spawn(move || {
      let deadline = Instant::now() + Duration::from_secs(30);
      let mut statuses: BTreeMap<GoalId, GoalStatusEnum> = BTreeMap::new();
      while !stop_srv.load(Ordering::Relaxed) && Instant::now() < deadline {
        if let Some((id, goal_id, _goal)) = server.try_receive_goal() {
          let _ = server.respond_goal(id, true);
          statuses.insert(goal_id, GoalStatusEnum::Executing);
        }
        if let Some((id, goal_id)) = server.try_receive_cancel() {
          // Single-goal cancel (goal_id non-zero in this test).
          let canceling = if statuses.contains_key(&goal_id) {
            statuses.insert(goal_id, GoalStatusEnum::Canceled);
            vec![GoalInfo {
              goal_id,
              stamp: Time::ZERO,
            }]
          } else {
            vec![]
          };
          let code = if canceling.is_empty() {
            CancelGoalResponseEnum::UnknownGoal
          } else {
            CancelGoalResponseEnum::None
          };
          let _ = server.respond_cancel(id, code, canceling);
        }
        // Publish the full status array every tick (as ROS action servers do).
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
        std::thread::sleep(Duration::from_millis(20));
      }
    });

    // Client: send a goal, observe Executing status, cancel it, observe Canceled.
    let deadline = Instant::now() + Duration::from_secs(30);
    let goal_id = loop {
      assert!(Instant::now() < deadline, "goal never accepted");
      match client.send_goal(FibGoal { order: 10 }) {
        Ok((gid, true)) => break gid,
        _ => std::thread::sleep(Duration::from_millis(200)),
      }
    };

    let status_of = |gid: GoalId, want: GoalStatusEnum| -> bool {
      let end = Instant::now() + Duration::from_secs(20);
      while Instant::now() < end {
        if let Some(array) = client.take_status() {
          if array
            .status_list
            .iter()
            .any(|s| s.goal_info.goal_id == gid && s.status == want)
          {
            return true;
          }
        }
        std::thread::sleep(Duration::from_millis(50));
      }
      false
    };

    assert!(
      status_of(goal_id, GoalStatusEnum::Executing),
      "never saw the goal reach Executing on /status"
    );

    // Cancel and check the response.
    let resp = {
      let end = Instant::now() + Duration::from_secs(20);
      loop {
        assert!(Instant::now() < end, "cancel never answered");
        if let Ok(r) = client.cancel_goal(goal_id) {
          break r;
        }
        std::thread::sleep(Duration::from_millis(100));
      }
    };
    assert_eq!(resp.return_code, CancelGoalResponseEnum::None);
    assert!(resp.goals_canceling.iter().any(|g| g.goal_id == goal_id));

    assert!(
      status_of(goal_id, GoalStatusEnum::Canceled),
      "never saw the goal reach Canceled on /status"
    );

    stop.store(true, Ordering::Relaxed);
    let _ = server_thread.join();
  }
}

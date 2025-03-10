use std::{
  collections::{btree_map::Entry, BTreeMap},
  marker::PhantomData,
  sync::Mutex,
};

#[allow(unused_imports)]
use log::{debug, error, info, warn};
use futures::{pin_mut, stream::StreamExt};
use rustdds::dds::{ReadError, ReadResult, WriteError, WriteResult};
pub use action_msgs::{CancelGoalRequest, GoalId, GoalInfo, GoalStatusEnum};

use crate::{
  action_msgs, builtin_interfaces,
  message::Message,
  names::Name,
  service::{request_id::RmwRequestId, AService, Server},
  Publisher,
};
use super::{
  ActionTypes, FeedbackMessage, GetResultRequest, GetResultResponse, SendGoalRequest,
  SendGoalResponse,
};

/// ROS 2 Action Server - Synchronous version. Please consider using
/// `AsyncActionServer`instead.
pub struct ActionServer<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  pub(crate) my_goal_server: Server<AService<SendGoalRequest<A::GoalType>, SendGoalResponse>>,

  pub(crate) my_cancel_server:
    Server<AService<action_msgs::CancelGoalRequest, action_msgs::CancelGoalResponse>>,

  pub(crate) my_result_server: Server<AService<GetResultRequest, GetResultResponse<A::ResultType>>>,

  pub(crate) my_feedback_publisher: Publisher<FeedbackMessage<A::FeedbackType>>,

  pub(crate) my_status_publisher: Publisher<action_msgs::GoalStatusArray>,

  pub(crate) my_action_name: Name,
}

impl<A> ActionServer<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  pub fn name(&self) -> &Name {
    &self.my_action_name
  }

  pub fn goal_server(
    &mut self,
  ) -> &mut Server<AService<SendGoalRequest<A::GoalType>, SendGoalResponse>> {
    &mut self.my_goal_server
  }
  pub fn cancel_server(
    &mut self,
  ) -> &mut Server<AService<action_msgs::CancelGoalRequest, action_msgs::CancelGoalResponse>> {
    &mut self.my_cancel_server
  }
  pub fn result_server(
    &mut self,
  ) -> &mut Server<AService<GetResultRequest, GetResultResponse<A::ResultType>>> {
    &mut self.my_result_server
  }
  pub fn feedback_publisher(&mut self) -> &mut Publisher<FeedbackMessage<A::FeedbackType>> {
    &mut self.my_feedback_publisher
  }
  pub fn my_status_publisher(&mut self) -> &mut Publisher<action_msgs::GoalStatusArray> {
    &mut self.my_status_publisher
  }

  /// Receive a new goal, if available.
  pub fn receive_goal(&self) -> ReadResult<Option<(RmwRequestId, SendGoalRequest<A::GoalType>)>>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    self.my_goal_server.receive_request()
  }

  /// Send a response for the specified goal request
  pub fn send_goal_response(
    &self,
    req_id: RmwRequestId,
    resp: SendGoalResponse,
  ) -> WriteResult<(), ()>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    self.my_goal_server.send_response(req_id, resp)
  }

  /// Receive a cancel request, if available.
  pub fn receive_cancel_request(
    &self,
  ) -> ReadResult<Option<(RmwRequestId, action_msgs::CancelGoalRequest)>> {
    self.my_cancel_server.receive_request()
  }

  // Respond to a received cancel request
  pub fn send_cancel_response(
    &self,
    req_id: RmwRequestId,
    resp: action_msgs::CancelGoalResponse,
  ) -> WriteResult<(), ()> {
    self.my_cancel_server.send_response(req_id, resp)
  }

  pub fn receive_result_request(&self) -> ReadResult<Option<(RmwRequestId, GetResultRequest)>>
  where
    <A as ActionTypes>::ResultType: 'static,
  {
    self.my_result_server.receive_request()
  }

  pub fn send_result(
    &self,
    result_request_id: RmwRequestId,
    resp: GetResultResponse<A::ResultType>,
  ) -> WriteResult<(), ()>
  where
    <A as ActionTypes>::ResultType: 'static,
  {
    self.my_result_server.send_response(result_request_id, resp)
  }

  pub fn send_feedback(
    &self,
    goal_id: GoalId,
    feedback: A::FeedbackType,
  ) -> WriteResult<(), FeedbackMessage<A::FeedbackType>> {
    self
      .my_feedback_publisher
      .publish(FeedbackMessage { goal_id, feedback })
  }

  // Send the status of all known goals.
  pub fn send_goal_statuses(
    &self,
    goal_statuses: action_msgs::GoalStatusArray,
  ) -> WriteResult<(), action_msgs::GoalStatusArray> {
    self.my_status_publisher.publish(goal_statuses)
  }
} // impl ActionServer

// internal type to keep track of goals executing
#[derive(Debug, Clone)]
struct AsyncGoal<A>
where
  A: ActionTypes,
{
  status: GoalStatusEnum,
  accepted_time: Option<builtin_interfaces::Time>,
  goal: A::GoalType,
}

#[derive(Clone, Copy)]
struct InnerGoalHandle<G> {
  goal_id: GoalId,
  phantom: PhantomData<G>,
}

/// `NewGoalHandle` is received from an action client. It can be either accepted
/// of rejected.
///
/// `NewGoalHandle` , `AcceptedGoalHandle`, and `ExecutingGoalHandle` are
/// different types because they support different operations.
///
/// `AcceptedGoalHandle` is the result of action server accepting a goal. The
/// action server can either start executing, or abort the goal, if it is no
/// longer possible.
///
/// `ExecutingGoalHandle` is the result of starting exeuction on a goal. It
/// supports:
/// * `publish_feedback` - Notify action client that goal makes progress
/// * `send_result_response` - Send the final result of the goal to the client
///   (even if not successful)
/// * `abort_executing_goal` - Abort the goal, if it is no longer executable.
#[derive(Clone, Copy)]
pub struct NewGoalHandle<G> {
  inner: InnerGoalHandle<G>,
  req_id: RmwRequestId,
}

impl<G> NewGoalHandle<G> {
  pub fn goal_id(&self) -> GoalId {
    self.inner.goal_id
  }
}

/// See [`NewGoalHandle`]
#[derive(Clone, Copy)]
pub struct AcceptedGoalHandle<G> {
  inner: InnerGoalHandle<G>,
}

impl<G> AcceptedGoalHandle<G> {
  pub fn goal_id(&self) -> GoalId {
    self.inner.goal_id
  }
}

/// See [`NewGoalHandle`]
#[derive(Clone, Copy)]
pub struct ExecutingGoalHandle<G> {
  inner: InnerGoalHandle<G>,
}

impl<G> ExecutingGoalHandle<G> {
  pub fn goal_id(&self) -> GoalId {
    self.inner.goal_id
  }
}

/// Handle used by action server to receive a goal cancel request from client
/// and respond to it.
pub struct CancelHandle {
  req_id: RmwRequestId,
  goals: Vec<GoalId>,
}

impl CancelHandle {
  pub fn goals(&self) -> impl Iterator<Item = GoalId> + '_ {
    self.goals.iter().cloned()
  }
  pub fn contains_goal(&self, goal_id: &GoalId) -> bool {
    self.goals.contains(goal_id)
  }
}

/// What was the cause of action ending
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalEndStatus {
  Succeeded,
  Aborted,
  Canceled,
}

/// Errors tha may occur in goal operations
#[derive(Debug)]
pub enum GoalError<T> {
  NoSuchGoal,
  WrongGoalState,
  DDSReadError(ReadError),
  DDSWriteError(WriteError<T>),
}

impl<T> From<ReadError> for GoalError<T> {
  fn from(e: ReadError) -> Self {
    GoalError::DDSReadError(e)
  }
}
impl<T> From<WriteError<T>> for GoalError<T> {
  fn from(e: WriteError<T>) -> Self {
    GoalError::DDSWriteError(e)
  }
}

/// ROS 2 Action Server - Asynchronous version.
pub struct AsyncActionServer<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  actionserver: ActionServer<A>,
  // goals and result_requests are protected by _synchronous_ Mutex (not async)
  goals: Mutex<BTreeMap<GoalId, AsyncGoal<A>>>,
  result_requests: Mutex<BTreeMap<GoalId, RmwRequestId>>,
}

impl<A> AsyncActionServer<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  pub fn new(actionserver: ActionServer<A>) -> Self {
    AsyncActionServer::<A> {
      actionserver,
      goals: Mutex::new(BTreeMap::new()),
      result_requests: Mutex::new(BTreeMap::new()),
    }
  }

  pub fn get_new_goal(&self, handle: NewGoalHandle<A::GoalType>) -> Option<A::GoalType> {
    self
      .goals
      .lock()
      .unwrap()
      .get(&handle.inner.goal_id)
      .map(|ag| ag.goal.clone())
  }

  /// Receive a new goal from an action client.
  /// Server should immediately either accept or reject the goal.
  pub async fn receive_new_goal(&self) -> ReadResult<NewGoalHandle<A::GoalType>>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    let (req_id, goal_id) = loop {
      let (req_id, goal_request) = self
        .actionserver
        .my_goal_server
        .async_receive_request()
        .await?;
      match self.goals.lock().unwrap().entry(goal_request.goal_id) {
        e @ Entry::Vacant(_) => {
          e.or_insert(AsyncGoal {
            status: GoalStatusEnum::Unknown,
            goal: goal_request.goal,
            accepted_time: None,
          });
          break (req_id, goal_request.goal_id);
        }
        Entry::Occupied(_) => {
          error!(
            "Received duplicate goal_id {:?} , req_id={:?}",
            goal_request.goal_id, req_id
          );
          continue; // just discard this request
        }
      }
    };
    let inner = InnerGoalHandle {
      goal_id,
      phantom: PhantomData,
    };
    Ok(NewGoalHandle { inner, req_id })
  }

  /// Convert a newly received goal into a accepted goal, i.e. accept it
  /// for execution later. Client will be notified of acceptance.
  /// Note: Once the goal is accepted, the server must eventually call
  /// `.send_result_response()` even if the goal is canceled or aborted.
  pub async fn accept_goal(
    &self,
    handle: NewGoalHandle<A::GoalType>,
  ) -> Result<AcceptedGoalHandle<A::GoalType>, GoalError<()>>
  where
    A::GoalType: 'static,
  {
    let now = builtin_interfaces::Time::now();
    let result = match self.goals.lock().unwrap().entry(handle.inner.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => match o.get() {
        AsyncGoal {
          status: GoalStatusEnum::Unknown,
          ..
        } => {
          let mut_o = o.into_mut();
          mut_o.status = GoalStatusEnum::Accepted;
          mut_o.accepted_time = Some(now);
          Ok(AcceptedGoalHandle {
            inner: handle.inner,
          })
        }
        AsyncGoal {
          status: wrong_status,
          ..
        } => {
          error!(
            "Tried to accept goal {:?} but status was {:?}, expected Unknown.",
            handle.inner.goal_id, wrong_status
          );
          Err(GoalError::WrongGoalState)
        }
      },
    };

    // We do not do any async operations inside the match above,
    // because we are holding lock to self there.
    if result.is_ok() {
      self.publish_statuses().await;
      self.actionserver.my_goal_server.send_response(
        handle.req_id,
        SendGoalResponse {
          accepted: true,
          stamp: now,
        },
      )?;
    }
    result
  }

  /// Reject a received goal. Client will be notified of rejection.
  /// Server should not process the goal further.
  pub async fn reject_goal(&self, handle: NewGoalHandle<A::GoalType>) -> Result<(), GoalError<()>>
  where
    A::GoalType: 'static,
  {
    let result = match self.goals.lock().unwrap().entry(handle.inner.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => {
        match o.get() {
          AsyncGoal {
            status: GoalStatusEnum::Unknown,
            ..
          } => {
            //o.into_mut().0 = GoalStatusEnum::Rejected; -- there is no such state
            //self.publish_statuses().await; -- this is not reported
            Ok(())
          }
          AsyncGoal {
            status: wrong_status,
            ..
          } => {
            error!(
              "Tried to reject goal {:?} but status was {:?}, expected Unknown.",
              handle.inner.goal_id, wrong_status
            );
            Err(GoalError::WrongGoalState)
          }
        }
      }
    };

    if result.is_ok() {
      self.actionserver.my_goal_server.send_response(
        handle.req_id,
        SendGoalResponse {
          accepted: false,
          stamp: builtin_interfaces::Time::now(),
        },
      )?;
    }

    result
  }

  /// Convert an accepted goal into a expecting goal, i.e. start the execution.
  /// Executing goal can publish feedback.
  pub async fn start_executing_goal(
    &self,
    handle: AcceptedGoalHandle<A::GoalType>,
  ) -> Result<ExecutingGoalHandle<A::GoalType>, GoalError<()>> {
    let result = match self.goals.lock().unwrap().entry(handle.inner.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => match o.get() {
        AsyncGoal {
          status: GoalStatusEnum::Accepted,
          ..
        } => {
          o.into_mut().status = GoalStatusEnum::Executing;
          Ok(ExecutingGoalHandle {
            inner: handle.inner,
          })
        }
        AsyncGoal {
          status: wrong_status,
          ..
        } => {
          error!(
            "Tried to execute goal {:?} but status was {:?}, expected Accepted.",
            handle.inner.goal_id, wrong_status
          );
          Err(GoalError::WrongGoalState)
        }
      },
    };

    if result.is_ok() {
      self.publish_statuses().await;
    }
    result
  }

  /// Publish feedback on how the execution is proceeding.
  pub async fn publish_feedback(
    &self,
    handle: ExecutingGoalHandle<A::GoalType>,
    feedback: A::FeedbackType,
  ) -> Result<(), GoalError<FeedbackMessage<A::FeedbackType>>> {
    match self.goals.lock().unwrap().entry(handle.inner.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => match o.get() {
        AsyncGoal {
          status: GoalStatusEnum::Executing,
          ..
        } => {
          self
            .actionserver
            .send_feedback(handle.inner.goal_id, feedback)?;
          Ok(())
        }
        AsyncGoal {
          status: wrong_status,
          ..
        } => {
          error!(
            "Tried publish feedback on goal {:?} but status was {:?}, expected Executing.",
            handle.inner.goal_id, wrong_status
          );
          Err(GoalError::WrongGoalState)
        }
      },
    }
  }

  /// Notify Client that a goal end state was reached and
  /// what was the result of the action.
  /// This async will not resolve until the action client has requested for the
  /// result, but the client should request the result as soon as server
  /// accepts the goal.
  // TODO: It is a bit silly that we have to supply a "result" even though
  // goal got canceled. But we have to send something in the ResultResponse.
  // And where does it say that result is not significant if cancelled or aborted?
  pub async fn send_result_response(
    &self,
    handle: ExecutingGoalHandle<A::GoalType>,
    result_status: GoalEndStatus,
    result: A::ResultType,
  ) -> Result<(), GoalError<()>>
  where
    A::ResultType: 'static,
  {
    // We translate from interface type to internal type to ensure that
    // the end status is an end status and not e.g. "Accepted".
    let result_status = match result_status {
      GoalEndStatus::Succeeded => GoalStatusEnum::Succeeded,
      GoalEndStatus::Aborted => GoalStatusEnum::Aborted,
      GoalEndStatus::Canceled => GoalStatusEnum::Canceled,
    };

    // First, we must get a result request.
    // It may already have been read or not.
    // We will read these into a buffer, because there may be requests for
    // other goals' results also.
    let req_id_opt = self
      .result_requests
      .lock()
      .unwrap()
      .get(&handle.inner.goal_id)
      .cloned();
    // Binding `req_id_opt` above is used to drop the lock immediately.
    let req_id = match req_id_opt {
      Some(req_id) => req_id,
      None => {
        let res_reqs = self.actionserver.my_result_server.receive_request_stream();
        pin_mut!(res_reqs);
        loop {
          // result request was not yet here. Keep receiving until we get it.
          let (req_id, GetResultRequest { goal_id }) = res_reqs.select_next_some().await?;
          if goal_id == handle.inner.goal_id {
            break req_id;
          } else {
            self.result_requests.lock().unwrap().insert(goal_id, req_id);
            debug!(
              "Got result request for goal_id={:?} req_id={:?}",
              goal_id, req_id
            );
            // and loop to wait for the next
          }
        }
      }
    };
    let ret_value = match self.goals.lock().unwrap().entry(handle.inner.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => {
        match o.get() {
          // Accepted, executing, or canceling goal can be canceled or aborted
          // TODO: Accepted goal cannot succeed, it must be executing before success.
          AsyncGoal {
            status: GoalStatusEnum::Accepted,
            ..
          }
          | AsyncGoal {
            status: GoalStatusEnum::Executing,
            ..
          }
          | AsyncGoal {
            status: GoalStatusEnum::Canceling,
            ..
          } => {
            o.into_mut().status = result_status;
            self.actionserver.send_result(
              req_id,
              GetResultResponse {
                status: result_status,
                result,
              },
            )?;
            debug!(
              "Send result for goal_id={:?}  req_id={:?}",
              handle.inner.goal_id, req_id
            );
            Ok(())
          }
          AsyncGoal {
            status: wrong_status,
            ..
          } => {
            error!(
              "Tried to finish goal {:?} but status was {:?}.",
              handle.inner.goal_id, wrong_status
            );
            Err(GoalError::WrongGoalState)
          }
        }
      }
    };
    if ret_value.is_ok() {
      self.publish_statuses().await;
    }
    ret_value
  }

  /// Abort goal execution, because action server has determined it
  /// cannot continue execution.
  pub async fn abort_executing_goal(
    &self,
    handle: ExecutingGoalHandle<A::GoalType>,
  ) -> Result<(), GoalError<()>> {
    self.abort_goal(handle.inner).await
  }
  pub async fn abort_accepted_goal(
    &self,
    handle: AcceptedGoalHandle<A::GoalType>,
  ) -> Result<(), GoalError<()>> {
    self.abort_goal(handle.inner).await
  }

  async fn abort_goal(&self, handle: InnerGoalHandle<A::GoalType>) -> Result<(), GoalError<()>> {
    let abort_result = match self.goals.lock().unwrap().entry(handle.goal_id) {
      Entry::Vacant(_) => Err(GoalError::NoSuchGoal),
      Entry::Occupied(o) => match o.get() {
        AsyncGoal {
          status: GoalStatusEnum::Accepted,
          ..
        }
        | AsyncGoal {
          status: GoalStatusEnum::Executing,
          ..
        } => {
          o.into_mut().status = GoalStatusEnum::Aborted;
          Ok(())
        }
        AsyncGoal {
          status: wrong_status,
          ..
        } => {
          error!(
            "Tried to abort goal {:?} but status was {:?}, expected Accepted or Executing. ",
            handle.goal_id, wrong_status
          );
          Err(GoalError::WrongGoalState)
        }
      },
    };

    if abort_result.is_ok() {
      self.publish_statuses().await;
    }

    abort_result
  }

  /// Receive a set of cancel requests from the action client.
  /// The server should now respond either by accepting (some of) the
  /// cancel requests or rejecting all of them. The GoalIds that are requested
  /// to be cancelled can be currently at either accepted or executing state.
  pub async fn receive_cancel_request(&self) -> ReadResult<CancelHandle> {
    let (req_id, CancelGoalRequest { goal_info }) = self
      .actionserver
      .my_cancel_server
      .async_receive_request()
      .await?;

    #[allow(clippy::type_complexity)] // How would you refactor this type?
    let goal_filter: Box<dyn FnMut(&(&GoalId, &AsyncGoal<A>)) -> bool> = match goal_info {
      GoalInfo {
        goal_id: GoalId::ZERO,
        stamp: builtin_interfaces::Time::ZERO,
      } => Box::new(|(_, _)| true), // cancel all goals

      GoalInfo {
        goal_id: GoalId::ZERO,
        stamp,
      } => Box::new(move |(_, ag)| ag.accepted_time.map(|at| at < stamp).unwrap_or(false)),

      GoalInfo {
        goal_id,
        stamp: builtin_interfaces::Time::ZERO,
      } => Box::new(move |(g_id, _)| goal_id == **g_id),

      GoalInfo { goal_id, stamp } => Box::new(move |(g_id, ag)| {
        goal_id == **g_id || ag.accepted_time.map(move |at| at < stamp).unwrap_or(false)
      }),
    };

    // TODO:
    // Should check if the specified GoalId was unknown to us
    // or already terminated.
    // In those case outright send a negative response and not return to the
    // application.
    let cancel_handle = CancelHandle {
      req_id,
      goals: self
        .goals
        .lock()
        .unwrap()
        .iter()
        // only consider goals with status Executing or Accepted for Cancel
        .filter(|(_, async_goal)| {
          async_goal.status == GoalStatusEnum::Executing
            || async_goal.status == GoalStatusEnum::Accepted
        })
        // and then filter those that were specified by the cancel request
        .filter(goal_filter)
        .map(|p| *p.0)
        .collect(),
    };

    Ok(cancel_handle)
  }

  /// Respond to action client's cancel requests.
  /// The iterator of goals should list those GoalIds that will start canceling.
  /// For the other GoalIds, the cancel is not accepted and they do not change
  /// their state.
  pub async fn respond_to_cancel_requests(
    &self,
    cancel_handle: &CancelHandle,
    goals_to_cancel: impl Iterator<Item = GoalId>,
  ) -> WriteResult<(), ()> {
    let canceling_goals: Vec<GoalInfo> =
      goals_to_cancel
        .filter_map(|goal_id| {
          self.goals.lock().unwrap().get(&goal_id).and_then(
            |AsyncGoal { accepted_time, .. }| {
              accepted_time.map(|stamp| GoalInfo { goal_id, stamp })
            },
          )
        })
        .collect();

    for goal_info in &canceling_goals {
      self
        .goals
        .lock()
        .unwrap()
        .entry(goal_info.goal_id)
        .and_modify(|gg| gg.status = GoalStatusEnum::Canceling);
    }
    self.publish_statuses().await;

    let response = action_msgs::CancelGoalResponse {
      return_code: if canceling_goals.is_empty() {
        action_msgs::CancelGoalResponseEnum::Rejected
      } else {
        action_msgs::CancelGoalResponseEnum::None // i.e. no error
      },
      goals_canceling: canceling_goals,
    };

    self
      .actionserver
      .my_cancel_server
      .async_send_response(cancel_handle.req_id, response)
      .await
  }

  // This function is private, because all status publishing happens automatically
  // via goal status changes.
  async fn publish_statuses(&self) {
    let goal_status_array = action_msgs::GoalStatusArray {
      status_list: self
        .goals
        .lock()
        .unwrap()
        .iter()
        .map(
          |(
            goal_id,
            AsyncGoal {
              status,
              accepted_time,
              ..
            },
          )| action_msgs::GoalStatus {
            status: *status,
            goal_info: GoalInfo {
              goal_id: *goal_id,
              stamp: accepted_time.unwrap_or(builtin_interfaces::Time::ZERO),
            },
          },
        )
        .collect(),
    };
    debug!(
      "Reporting statuses for {:?}",
      goal_status_array
        .status_list
        .iter()
        .map(|gs| gs.goal_info.goal_id)
    );
    self
      .actionserver
      .send_goal_statuses(goal_status_array)
      .unwrap_or_else(|e| error!("AsyncActionServer::publish_statuses: {:?}", e));
  }
}

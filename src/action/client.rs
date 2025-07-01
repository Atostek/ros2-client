#[allow(unused_imports)]
use log::{debug, error, info, warn};
use rustdds::dds::{ReadResult, WriteResult};
pub use action_msgs::{CancelGoalRequest, CancelGoalResponse, GoalId, GoalInfo, GoalStatusEnum};
use builtin_interfaces::Time;
use futures::{
  //pin_mut,
  stream::{FusedStream, StreamExt},
  Future,
};

use crate::{
  action_msgs, builtin_interfaces,
  message::Message,
  names::Name,
  service::{request_id::RmwRequestId, AService, CallServiceError, Client},
  unique_identifier_msgs, Subscription,
};
use super::{
  ActionTypes, FeedbackMessage, GetResultRequest, GetResultResponse, SendGoalRequest,
  SendGoalResponse,
};

/// A client for ROS 2 Actions. Supports both sync and async operation.
pub struct ActionClient<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  pub(crate) my_goal_client: Client<AService<SendGoalRequest<A::GoalType>, SendGoalResponse>>,

  pub(crate) my_cancel_client:
    Client<AService<action_msgs::CancelGoalRequest, action_msgs::CancelGoalResponse>>,

  pub(crate) my_result_client: Client<AService<GetResultRequest, GetResultResponse<A::ResultType>>>,

  pub(crate) my_feedback_subscription: Subscription<FeedbackMessage<A::FeedbackType>>,

  pub(crate) my_status_subscription: Subscription<action_msgs::GoalStatusArray>,

  pub(crate) my_action_name: Name,
}

impl<A> ActionClient<A>
where
  A: ActionTypes,
  A::GoalType: Message + Clone,
  A::ResultType: Message + Clone,
  A::FeedbackType: Message,
{
  pub fn name(&self) -> &Name {
    &self.my_action_name
  }

  pub fn goal_client(
    &mut self,
  ) -> &mut Client<AService<SendGoalRequest<A::GoalType>, SendGoalResponse>> {
    &mut self.my_goal_client
  }
  pub fn cancel_client(
    &mut self,
  ) -> &mut Client<AService<action_msgs::CancelGoalRequest, action_msgs::CancelGoalResponse>> {
    &mut self.my_cancel_client
  }
  pub fn result_client(
    &mut self,
  ) -> &mut Client<AService<GetResultRequest, GetResultResponse<A::ResultType>>> {
    &mut self.my_result_client
  }
  pub fn feedback_subscription(&mut self) -> &mut Subscription<FeedbackMessage<A::FeedbackType>> {
    &mut self.my_feedback_subscription
  }
  pub fn status_subscription(&mut self) -> &mut Subscription<action_msgs::GoalStatusArray> {
    &mut self.my_status_subscription
  }

  /// Returns and id of the Request and id for the Goal.
  /// Request id can be used to recognize correct response from Action Server.
  /// Goal id is later used to communicate Goal status and result.
  pub fn send_goal(&self, goal: A::GoalType) -> WriteResult<(RmwRequestId, GoalId), ()>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    let goal_id = unique_identifier_msgs::UUID::new_random();
    self
      .my_goal_client
      .send_request(SendGoalRequest { goal_id, goal })
      .map(|req_id| (req_id, goal_id))
  }

  /// Receive a response for the specified goal request, or None if response is
  /// not yet available
  pub fn receive_goal_response(&self, req_id: RmwRequestId) -> ReadResult<Option<SendGoalResponse>>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    loop {
      match self.my_goal_client.receive_response() {
        Err(e) => break Err(e),
        Ok(None) => break Ok(None), // not yet
        Ok(Some((incoming_req_id, resp))) if incoming_req_id == req_id =>
        // received the expected answer
        {
          break Ok(Some(resp))
        }
        Ok(Some((incoming_req_id, _resp))) => {
          // got someone else's answer. Try again.
          info!("Goal Response not for us: {incoming_req_id:?} != {req_id:?}");
          continue;
        }
      }
    }
    // We loop here to drain all the answers received so far.
    // The mio .poll() only does not trigger again for the next item, if it has
    // been received already.
  }

  pub async fn async_send_goal(
    &self,
    goal: A::GoalType,
  ) -> Result<(GoalId, SendGoalResponse), CallServiceError<()>>
  where
    <A as ActionTypes>::GoalType: 'static,
  {
    let goal_id = unique_identifier_msgs::UUID::new_random();
    let send_goal_response = self
      .my_goal_client
      .async_call_service(SendGoalRequest { goal_id, goal })
      .await?;
    Ok((goal_id, send_goal_response))
  }

  // From ROS2 docs:
  // https://docs.ros2.org/foxy/api/action_msgs/srv/CancelGoal.html
  //
  // Cancel one or more goals with the following policy:
  // - If the goal ID is zero and timestamp is zero, cancel all goals.
  // - If the goal ID is zero and timestamp is not zero, cancel all goals accepted
  //   at or before the timestamp.
  // - If the goal ID is not zero and timestamp is zero, cancel the goal with the
  //   given ID regardless of the time it was accepted.
  // - If the goal ID is not zero and timestamp is not zero, cancel the goal with
  //   the given ID and all goals accepted at or before the timestamp.

  fn cancel_goal_raw(&self, goal_id: GoalId, timestamp: Time) -> WriteResult<RmwRequestId, ()> {
    let goal_info = GoalInfo {
      goal_id,
      stamp: timestamp,
    };
    self
      .my_cancel_client
      .send_request(CancelGoalRequest { goal_info })
  }

  pub fn cancel_goal(&self, goal_id: GoalId) -> WriteResult<RmwRequestId, ()> {
    self.cancel_goal_raw(goal_id, Time::ZERO)
  }

  pub fn cancel_all_goals_before(&self, timestamp: Time) -> WriteResult<RmwRequestId, ()> {
    self.cancel_goal_raw(GoalId::ZERO, timestamp)
  }

  pub fn cancel_all_goals(&self) -> WriteResult<RmwRequestId, ()> {
    self.cancel_goal_raw(GoalId::ZERO, Time::ZERO)
  }

  pub fn receive_cancel_response(
    &self,
    cancel_request_id: RmwRequestId,
  ) -> ReadResult<Option<CancelGoalResponse>> {
    loop {
      match self.my_cancel_client.receive_response() {
        Err(e) => break Err(e),
        Ok(None) => break Ok(None), // not yet
        Ok(Some((incoming_req_id, resp))) if incoming_req_id == cancel_request_id => {
          break Ok(Some(resp))
        } // received expected answer
        Ok(Some(_)) => continue,    // got someone else's answer. Try again.
      }
    }
  }

  pub fn async_cancel_goal(
    &self,
    goal_id: GoalId,
    timestamp: Time,
  ) -> impl Future<Output = Result<CancelGoalResponse, CallServiceError<()>>> + '_ {
    let goal_info = GoalInfo {
      goal_id,
      stamp: timestamp,
    };
    self
      .my_cancel_client
      .async_call_service(CancelGoalRequest { goal_info })
  }

  pub fn request_result(&self, goal_id: GoalId) -> WriteResult<RmwRequestId, ()>
  where
    <A as ActionTypes>::ResultType: 'static,
  {
    self
      .my_result_client
      .send_request(GetResultRequest { goal_id })
  }

  pub fn receive_result(
    &self,
    result_request_id: RmwRequestId,
  ) -> ReadResult<Option<(GoalStatusEnum, A::ResultType)>>
  where
    <A as ActionTypes>::ResultType: 'static,
  {
    loop {
      match self.my_result_client.receive_response() {
        Err(e) => break Err(e),
        Ok(None) => break Ok(None), // not yet
        Ok(Some((incoming_req_id, GetResultResponse { status, result })))
          if incoming_req_id == result_request_id =>
        {
          break Ok(Some((status, result)))
        } // received expected answer
        Ok(Some(_)) => continue,    // got someone else's answer. Try again.
      }
    }
  }

  /// Asynchronously request goal result.
  /// Result should be requested as soon as a goal is accepted.
  /// Result ia actually received only when Server informs that the goal has
  /// either Succeeded, or has been Canceled or Aborted.
  pub async fn async_request_result(
    &self,
    goal_id: GoalId,
  ) -> Result<(GoalStatusEnum, A::ResultType), CallServiceError<()>>
  where
    <A as ActionTypes>::ResultType: 'static,
  {
    let GetResultResponse { status, result } = self
      .my_result_client
      .async_call_service(GetResultRequest { goal_id })
      .await?;
    Ok((status, result))
  }

  pub fn receive_feedback(&self, goal_id: GoalId) -> ReadResult<Option<A::FeedbackType>>
  where
    <A as ActionTypes>::FeedbackType: 'static,
  {
    loop {
      match self.my_feedback_subscription.take() {
        Err(e) => break Err(e),
        Ok(None) => break Ok(None),
        Ok(Some((fb_msg, _msg_info))) if fb_msg.goal_id == goal_id => {
          break Ok(Some(fb_msg.feedback))
        }
        Ok(Some((fb_msg, _msg_info))) => {
          // feedback on some other goal
          debug!(
            "Feedback on another goal {:?} != {:?}",
            fb_msg.goal_id, goal_id
          )
        }
      }
    }
  }

  /// Receive asynchronous feedback stream of goal progress.
  pub fn feedback_stream(
    &self,
    goal_id: GoalId,
  ) -> impl FusedStream<Item = ReadResult<A::FeedbackType>> + '_
  where
    <A as ActionTypes>::FeedbackType: 'static,
  {
    let expected_goal_id = goal_id; // rename
    self
      .my_feedback_subscription
      .async_stream()
      .filter_map(move |result| async move {
        match result {
          Err(e) => Some(Err(e)),
          Ok((FeedbackMessage { goal_id, feedback }, _msg_info)) => {
            if goal_id == expected_goal_id {
              Some(Ok(feedback))
            } else {
              debug!("Feedback for some other {goal_id:?}.");
              None
            }
          }
        }
      })
  }

  /// Note: This does not take GoalId and will therefore report status of all
  /// Goals.
  pub fn receive_status(&self) -> ReadResult<Option<action_msgs::GoalStatusArray>> {
    self
      .my_status_subscription
      .take()
      .map(|r| r.map(|(gsa, _msg_info)| gsa))
  }

  pub async fn async_receive_status(&self) -> ReadResult<action_msgs::GoalStatusArray> {
    let (m, _msg_info) = self.my_status_subscription.async_take().await?;
    Ok(m)
  }

  /// Async Stream of status updates
  /// Action server send updates containing status of all goals, hence an array.
  pub fn all_statuses_stream(
    &self,
  ) -> impl FusedStream<Item = ReadResult<action_msgs::GoalStatusArray>> + '_ {
    self
      .my_status_subscription
      .async_stream()
      .map(|result| result.map(|(gsa, _mi)| gsa))
  }

  pub fn status_stream(
    &self,
    goal_id: GoalId,
  ) -> impl FusedStream<Item = ReadResult<action_msgs::GoalStatus>> + '_ {
    self
      .all_statuses_stream()
      .filter_map(move |result| async move {
        match result {
          Err(e) => Some(Err(e)),
          Ok(gsa) => gsa
            .status_list
            .into_iter()
            .find(|gs| gs.goal_info.goal_id == goal_id)
            .map(Ok),
        }
      })
  }
} // impl ActionClient

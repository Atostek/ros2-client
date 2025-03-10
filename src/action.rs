use std::marker::PhantomData;

use rustdds::*;
use serde::{Deserialize, Serialize};
pub use action_msgs::{CancelGoalRequest, CancelGoalResponse, GoalId, GoalInfo, GoalStatusEnum};
#[allow(unused_imports)]
use log::{debug, error, info, warn};

use crate::{action_msgs, builtin_interfaces, message::Message};

mod client;
#[doc(inline)]
pub use client::ActionClient;

mod server;
#[doc(inline)]
pub use server::{
  AcceptedGoalHandle, ActionServer, AsyncActionServer, CancelHandle, ExecutingGoalHandle,
  GoalEndStatus, GoalError, NewGoalHandle,
};

/// A trait to define an Action type
pub trait ActionTypes {
  type GoalType: Message + Clone; // Used by client to set a goal for the server
  type ResultType: Message + Clone; // Used by server to report result when action ends
  type FeedbackType: Message; // Used by server to report progress during action execution

  fn goal_type_name(&self) -> &str;
  fn result_type_name(&self) -> &str;
  fn feedback_type_name(&self) -> &str;
}

/// This is used to construct an ActionType implementation from pre-existing
/// component types.
pub struct Action<G, R, F> {
  g: PhantomData<G>,
  r: PhantomData<R>,
  f: PhantomData<F>,
  goal_typename: String,
  result_typename: String,
  feedback_typename: String,
}

impl<G, R, F> Action<G, R, F>
where
  G: Message + Clone,
  R: Message + Clone,
  F: Message,
{
  pub fn new(goal_typename: String, result_typename: String, feedback_typename: String) -> Self {
    Self {
      goal_typename,
      result_typename,
      feedback_typename,
      g: PhantomData,
      r: PhantomData,
      f: PhantomData,
    }
  }
}

impl<G, R, F> ActionTypes for Action<G, R, F>
where
  G: Message + Clone,
  R: Message + Clone,
  F: Message,
{
  type GoalType = G;
  type ResultType = R;
  type FeedbackType = F;

  fn goal_type_name(&self) -> &str {
    &self.goal_typename
  }

  fn result_type_name(&self) -> &str {
    &self.result_typename
  }

  fn feedback_type_name(&self) -> &str {
    &self.feedback_typename
  }
}

//TODO: Make fields private, add constructor and accessors.

/// Collection of QoS policies requires for an Action client
pub struct ActionClientQosPolicies {
  pub goal_service: QosPolicies,
  pub result_service: QosPolicies,
  pub cancel_service: QosPolicies,
  pub feedback_subscription: QosPolicies,
  pub status_subscription: QosPolicies,
}

/// Collection of QoS policies requires for an Action server
pub struct ActionServerQosPolicies {
  pub goal_service: QosPolicies,
  pub result_service: QosPolicies,
  pub cancel_service: QosPolicies,
  pub feedback_publisher: QosPolicies,
  pub status_publisher: QosPolicies,
}

/// Emulating ROS2 IDL code generator: Goal sending/setting service

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SendGoalRequest<G> {
  pub goal_id: GoalId,
  pub goal: G,
}
impl<G: Message> Message for SendGoalRequest<G> {}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SendGoalResponse {
  pub accepted: bool,
  pub stamp: builtin_interfaces::Time,
}
impl Message for SendGoalResponse {}

/// Emulating ROS2 IDL code generator: Result getting service
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GetResultRequest {
  pub goal_id: GoalId,
}
impl Message for GetResultRequest {}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GetResultResponse<R> {
  pub status: GoalStatusEnum, // interpretation same as in GoalStatus message?
  pub result: R,
}
impl<R: Message> Message for GetResultResponse<R> {}

/// Emulating ROS2 IDL code generator: Feedback Topic message type
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FeedbackMessage<F> {
  pub goal_id: GoalId,
  pub feedback: F,
}
impl<F: Message> Message for FeedbackMessage<F> {}

// Example topic names and types at DDS level:

// rq/turtle1/rotate_absolute/_action/send_goalRequest :
// turtlesim::action::dds_::RotateAbsolute_SendGoal_Request_ rr/turtle1/
// rotate_absolute/_action/send_goalReply :
// turtlesim::action::dds_::RotateAbsolute_SendGoal_Response_

// rq/turtle1/rotate_absolute/_action/cancel_goalRequest  :
// action_msgs::srv::dds_::CancelGoal_Request_ rr/turtle1/rotate_absolute/
// _action/cancel_goalReply  : action_msgs::srv::dds_::CancelGoal_Response_

// rq/turtle1/rotate_absolute/_action/get_resultRequest :
// turtlesim::action::dds_::RotateAbsolute_GetResult_Request_ rr/turtle1/
// rotate_absolute/_action/get_resultReply :
// turtlesim::action::dds_::RotateAbsolute_GetResult_Response_

// rt/turtle1/rotate_absolute/_action/feedback :
// turtlesim::action::dds_::RotateAbsolute_FeedbackMessage_

// rt/turtle1/rotate_absolute/_action/status :
// action_msgs::msg::dds_::GoalStatusArray_

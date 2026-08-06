use serde::{Deserialize, Serialize};
pub use action_msgs::{CancelGoalRequest, CancelGoalResponse, GoalId, GoalInfo, GoalStatusEnum};

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

//TODO: Make fields private, add constructor and accessors.

/// Collection of QoS profiles required for an Action client
pub struct ActionClientQosPolicies {
  pub goal_service: crate::qos::QosProfile,
  pub result_service: crate::qos::QosProfile,
  pub cancel_service: crate::qos::QosProfile,
  pub feedback_subscription: crate::qos::QosProfile,
  pub status_subscription: crate::qos::QosProfile,
}

/// Collection of QoS profiles required for an Action server
pub struct ActionServerQosPolicies {
  pub goal_service: crate::qos::QosProfile,
  pub result_service: crate::qos::QosProfile,
  pub cancel_service: crate::qos::QosProfile,
  pub feedback_publisher: crate::qos::QosProfile,
  pub status_publisher: crate::qos::QosProfile,
}

/// Emulating ROS2 IDL code generator: Goal sending/setting service request
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SendGoalRequest<G> {
  pub goal_id: GoalId,
  pub goal: G,
}
impl<G: Message> Message for SendGoalRequest<G> {}

/// Emulating ROS2 IDL code generator: Goal sending/setting service response
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SendGoalResponse {
  pub accepted: bool,
  pub stamp: builtin_interfaces::Time,
}
impl Message for SendGoalResponse {}

/// Emulating ROS2 IDL code generator: Result getting service request
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GetResultRequest {
  pub goal_id: GoalId,
}
impl Message for GetResultRequest {}

/// Emulating ROS2 IDL code generator: Result getting service response
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

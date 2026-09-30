//! Corresponds to package [rcl_interfaces](https://index.ros.org/p/rcl_interfaces/). Defines message types for Parameter manipulation.
//!
//! For logging, see [`log`](crate::log).

use serde::{Deserialize, Serialize};

use crate::{parameters, Message};

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParametersRequest {
  pub prefixes: Vec<String>,
  pub depth: u64, //
}
impl Message for ListParametersRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParametersResult {
  pub names: Vec<String>,
  pub prefixes: Vec<String>,
}
impl Message for ListParametersResult {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParametersResponse {
  pub result: ListParametersResult,
}
impl Message for ListParametersResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParametersRequest {
  pub names: Vec<String>,
}
impl Message for GetParametersRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParametersResponse {
  pub values: Vec<parameters::raw::ParameterValue>,
}
impl Message for GetParametersResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParameterTypesRequest {
  pub names: Vec<String>,
}
impl Message for GetParameterTypesRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParameterTypesResponse {
  pub values: Vec<u8>,
}
impl Message for GetParameterTypesResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetParametersRequest {
  pub parameter: Vec<parameters::raw::Parameter>,
}
impl Message for SetParametersRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetParametersResponse {
  pub results: Vec<parameters::raw::SetParametersResult>,
}
impl Message for SetParametersResponse {}

// https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/srv/SetParametersAtomically.srv
// Note: a single `result`, unlike SetParameters which returns one per parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetParametersAtomicallyResponse {
  pub result: parameters::raw::SetParametersResult,
}
impl Message for SetParametersAtomicallyResponse {}

// https://github.com/ros2/rcl_interfaces/blob/humble/rcl_interfaces/srv/DescribeParameters.srv
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeParametersRequest {
  pub names: Vec<String>,
}
impl Message for DescribeParametersRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeParametersResponse {
  pub values: Vec<parameters::raw::ParameterDescriptor>,
}
impl Message for DescribeParametersResponse {}

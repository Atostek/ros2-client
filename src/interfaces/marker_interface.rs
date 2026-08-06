use serde::{Deserialize, Serialize};

use crate::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerRequest {
  pub marker: String,
}
impl Message for MarkerRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerResponse {
  pub marker: String,
}
impl Message for MarkerResponse {}

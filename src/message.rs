//! Defines [`Message`] trait, which defines data that is to be sent over
//! Topics.

use serde::{Serialize, de::DeserializeOwned};

/// Trait to ensure Messages can be (de)serialized
pub trait Message: Serialize + DeserializeOwned {}

impl Message for () {}
impl Message for String {}

impl Message for i8 {}
impl Message for i16 {}
impl Message for i32 {}
impl Message for i64 {}

impl Message for u8 {}
impl Message for u16 {}
impl Message for u32 {}
impl Message for u64 {}

impl<T: Message> Message for Vec<T> {}

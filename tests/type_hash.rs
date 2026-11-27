//! A node created with a type hash resolver announces the REP-2016 type hash
//! of each of its DDS endpoints in their USER_DATA, as rmw does on Iron and
//! later: `typehash=RIHS01_<hash>;`.
#![cfg(feature = "dds")]

use std::time::{Duration, Instant};

use ros2_client::{
  MessageTypeName, Name, NodeName, NodeOptions, QosProfile, ServiceMapping, ServiceTypeName,
  dds::{
    Context, ContextOptions, Publisher,
    rustdds::{DomainParticipant, DomainParticipantStatusEvent, StatusEvented},
  },
};

const DOMAIN_ID: u16 = 58;

/// A made-up hash per DDS type name, so the test can tell them apart.
fn type_hash(dds_type_name: &str) -> Option<String> {
  match dds_type_name {
    "std_msgs::msg::dds_::String_" => Some("RIHS01_string".to_owned()),
    "rcl_interfaces::msg::dds_::Log_" => Some("RIHS01_log".to_owned()),
    "example_interfaces::srv::dds_::AddTwoInts_Request_" => Some("RIHS01_request".to_owned()),
    "example_interfaces::srv::dds_::AddTwoInts_Response_" => Some("RIHS01_response".to_owned()),
    _ => None,
  }
}

/// The USER_DATA of each remote endpoint on `topics`, by topic.
fn user_data_of(observer: &DomainParticipant, topics: &[&str]) -> Vec<(String, Vec<u8>)> {
  let status = observer.status_listener();
  let mut found: Vec<(String, Vec<u8>)> = Vec::new();
  let deadline = Instant::now() + Duration::from_secs(10);
  while found.len() < topics.len() && Instant::now() < deadline {
    let endpoint = match status.try_recv_status() {
      Some(DomainParticipantStatusEvent::WriterDetected { writer }) => writer,
      Some(DomainParticipantStatusEvent::ReaderDetected { reader }) => reader,
      Some(_) => continue,
      None => {
        std::thread::sleep(Duration::from_millis(20));
        continue;
      }
    };
    if topics.contains(&endpoint.topic_name.as_str())
      && !found.iter().any(|(topic, _)| *topic == endpoint.topic_name)
    {
      let user_data = endpoint
        .qos
        .user_data()
        .map(|u| u.value)
        .unwrap_or_default();
      found.push((endpoint.topic_name, user_data));
    }
  }
  found.sort();
  found
}

#[test]
fn endpoints_announce_their_type_hash() {
  let observer = DomainParticipant::new(DOMAIN_ID).unwrap();
  let context = Context::with_options(ContextOptions::new().domain_id(DOMAIN_ID)).unwrap();
  let mut node = context
    .new_node(
      NodeName::new("/", "type_hash_test").unwrap(),
      NodeOptions::new().type_hash_resolver(type_hash),
    )
    .unwrap();
  let topic = node
    .create_topic(
      &Name::new("/", "chatter").unwrap(),
      MessageTypeName::new("std_msgs", "String"),
      &QosProfile::publisher_default(),
    )
    .unwrap();
  let _publisher: Publisher<String> = node.create_publisher(&topic, None).unwrap();
  let _client = node
    .create_client::<i64, i64>(
      ServiceMapping::Enhanced,
      &Name::new("/", "add_two_ints").unwrap(),
      &ServiceTypeName::new("example_interfaces", "AddTwoInts"),
      QosProfile::publisher_default(),
      QosProfile::publisher_default(),
    )
    .unwrap();

  assert_eq!(
    user_data_of(
      &observer,
      &[
        "rt/chatter",
        "rt/rosout",
        "rq/add_two_intsRequest",
        "rr/add_two_intsReply",
        "rt/parameter_events",
      ],
    ),
    vec![
      (
        "rq/add_two_intsRequest".to_owned(),
        b"typehash=RIHS01_request;".to_vec()
      ),
      (
        "rr/add_two_intsReply".to_owned(),
        b"typehash=RIHS01_response;".to_vec()
      ),
      ("rt/chatter".to_owned(), b"typehash=RIHS01_string;".to_vec()),
      ("rt/parameter_events".to_owned(), Vec::new()),
      ("rt/rosout".to_owned(), b"typehash=RIHS01_log;".to_vec()),
    ]
  );
}

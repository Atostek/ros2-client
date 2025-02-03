use std::{
  collections::{BTreeMap, BTreeSet},
  error::Error,
  fmt,
  pin::Pin,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
  },
};

use futures::{
  pin_mut, stream::FusedStream, task, task::Poll, Future, FutureExt, Stream, StreamExt, stream,
};
use async_channel::Receiver;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::Serialize;
use rustdds::{
  dds::{CreateError, CreateResult},
  *,
};

use crate::{
  action::*,
  builtin_interfaces,
  context::{Context, DEFAULT_SUBSCRIPTION_QOS},
  entities_info::{NodeEntitiesInfo, ParticipantEntitiesInfo},
  gid::Gid,
  log as ros_log,
  log::Log,
  names::*,
  parameters::*,
  pubsub::{Publisher, Subscription},
  rcl_interfaces,
  ros_time::ROSTime,
  service::{Client, Server, Service, ServiceMapping},
};

type ParameterFunc = dyn Fn(&str, &ParameterValue) -> SetParametersResult + Send + Sync;

/// Configuration of [Node]
/// This is a builder-like struct.
///
/// The NodeOptions struct does not contain
/// node_name, context, or namespace, because
/// they ae always needed and have no reasonable default.
#[must_use]
pub struct NodeOptions {
  #[allow(dead_code)]
  cli_args: Vec<String>,
  #[allow(dead_code)]
  use_global_arguments: bool, // process-wide command line args
  enable_rosout: bool, // use rosout topic for logging?
  enable_rosout_reading: bool,
  start_parameter_services: bool,
  declared_parameters: Vec<Parameter>,
  allow_undeclared_parameters: bool,
  parameter_validator: Option<Box<ParameterFunc>>,
  parameter_set_action: Option<Box<ParameterFunc>>,
}

impl NodeOptions {
  /// Get a default NodeOptions
  pub fn new() -> NodeOptions {
    // These defaults are from rclpy reference
    // https://docs.ros2.org/latest/api/rclpy/api/node.html
    NodeOptions {
      cli_args: Vec::new(),
      use_global_arguments: true,
      enable_rosout: true,
      enable_rosout_reading: false,
      start_parameter_services: true,
      declared_parameters: Vec::new(),
      allow_undeclared_parameters: false,
      parameter_validator: None,
      parameter_set_action: None,
    }
  }
  pub fn enable_rosout(self, enable_rosout: bool) -> NodeOptions {
    NodeOptions {
      enable_rosout,
      ..self
    }
  }

  pub fn read_rosout(self, enable_rosout_reading: bool) -> NodeOptions {
    NodeOptions {
      enable_rosout_reading,
      ..self
    }
  }

  pub fn declare_parameter(mut self, name: &str, value: ParameterValue) -> NodeOptions {
    self.declared_parameters.push(Parameter {
      name: name.to_owned(),
      value,
    });
    // TODO: check for duplicate parameter names
    self
  }

  pub fn parameter_validator(mut self, validator: Box<ParameterFunc>) -> NodeOptions {
    self.parameter_validator = Some(validator);
    self
  }

  pub fn parameter_set_action(mut self, action: Box<ParameterFunc>) -> NodeOptions {
    self.parameter_set_action = Some(action);
    self
  }
}

impl Default for NodeOptions {
  fn default() -> Self {
    Self::new()
  }
}
// ----------------------------------------------------------------------------------------------------
// ----------------------------------------------------------------------------------------------------

/// DDS or ROS 2 Discovery events.
#[derive(Clone, Debug)]
pub enum NodeEvent {
  DDS(DomainParticipantStatusEvent),
  ROS(ParticipantEntitiesInfo),
}

struct ParameterServers {
  get_parameters_server: Server<rcl_interfaces::GetParametersService>,
  get_parameter_types_server: Server<rcl_interfaces::GetParameterTypesService>,
  list_parameters_server: Server<rcl_interfaces::ListParametersService>,
  set_parameters_server: Server<rcl_interfaces::SetParametersService>,
  set_parameters_atomically_server: Server<rcl_interfaces::SetParametersAtomicallyService>,
  describe_parameters_server: Server<rcl_interfaces::DescribeParametersService>,
}

// ----------------------------------------------------------------------------------------------------
// ----------------------------------------------------------------------------------------------------
/// Spinner implements Node's background event loop.
///
/// At the moment there are only Discovery (DDS and ROS 2 Graph) event
/// processing, but this would be extended to handle Parameters and other
/// possible background tasks also.
pub struct Spinner {
  ros_context: Context,
  stop_spin_receiver: async_channel::Receiver<()>,

  readers_to_remote_writers: Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>,
  writers_to_remote_readers: Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>,
  // Keep track of ros_discovery_info
  external_nodes: Arc<Mutex<BTreeMap<Gid, Vec<NodeEntitiesInfo>>>>,
  //suppress_node_info_updates: Arc<AtomicBool>, // temporarily suppress sending updates
  status_event_senders: Arc<Mutex<Vec<async_channel::Sender<NodeEvent>>>>,

  use_sim_time: Arc<AtomicBool>,
  sim_time: Arc<Mutex<ROSTime>>,
  clock_topic: Topic,
  allow_undeclared_parameters: bool,

  parameter_servers: Option<ParameterServers>,
  parameter_events_writer: Arc<Publisher<raw::ParameterEvent>>,
  parameters: Arc<Mutex<BTreeMap<String, ParameterValue>>>,
  parameter_validator: Option<Arc<Mutex<Box<ParameterFunc>>>>,
  parameter_set_action: Option<Arc<Mutex<Box<ParameterFunc>>>>,
  fully_qualified_node_name: String,
}

async fn next_if_some<S>(s: &mut Option<S>) -> S::Item
where
  S: Stream + Unpin + FusedStream,
{
  match s.as_mut() {
    Some(stream) => stream.select_next_some().await,
    None => std::future::pending().await,
  }
}

impl Spinner {
  pub async fn spin(self) -> CreateResult<()> {
    let dds_status_listener = self.ros_context.domain_participant().status_listener();
    let dds_status_stream = dds_status_listener.as_async_status_stream();
    pin_mut!(dds_status_stream);

    let ros_discovery_topic = self.ros_context.ros_discovery_topic();
    let ros_discovery_reader = self
      .ros_context
      .create_subscription::<ParticipantEntitiesInfo>(&ros_discovery_topic, None)?;
    let ros_discovery_stream = ros_discovery_reader.async_stream();
    pin_mut!(ros_discovery_stream);

    let ros_clock_reader = self
      .ros_context
      .create_subscription::<builtin_interfaces::Time>(&self.clock_topic, None)?;
    let ros_clock_stream = ros_clock_reader.async_stream();
    pin_mut!(ros_clock_stream);

    // These are Option< impl Stream<_>>
    let mut get_parameters_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.get_parameters_server.receive_request_stream());
    let mut get_parameter_types_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.get_parameter_types_server.receive_request_stream());
    let mut set_parameters_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.set_parameters_server.receive_request_stream());
    let mut set_parameters_atomically_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.set_parameters_atomically_server.receive_request_stream());
    let mut list_parameter_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.list_parameters_server.receive_request_stream());
    let mut describe_parameters_stream_opt = self
      .parameter_servers
      .as_ref()
      .map(|s| s.describe_parameters_server.receive_request_stream());

    loop {
      futures::select! {
        _ = self.stop_spin_receiver.recv().fuse() => {
          break;
        }

        clock_msg = ros_clock_stream.select_next_some() => {
          match clock_msg {
            Ok((time,_msg_info)) => {
              // Simulated time is updated internally unconditionally.
              // The logic in Node decides if it is used.
              *self.sim_time.lock().unwrap() = time.into();
            }
            Err(e) => warn!("Simulated clock receive error {e:?}")
          }
        }


        get_parameters_request = next_if_some(&mut get_parameters_stream_opt).fuse() => {
          match get_parameters_request {
            Ok( (req_id, req) ) => {
              info!("Get parameter request {req:?}");
              let values = {
                let param_db = self.parameters.lock().unwrap();
                req.names.iter()
                  .map(|name| param_db.get(name.as_str())
                    .unwrap_or(&ParameterValue::NotSet))
                  .cloned()
                  .map( raw::ParameterValue::from)
                  .collect()
              };
              info!("Get parameters response: {values:?}");

              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              self.parameter_servers.as_ref().unwrap().get_parameters_server
                .async_send_response(req_id, rcl_interfaces::GetParametersResponse{ values })
                .await
                .unwrap_or_else(|e| warn!("GetParameter response error {e:?}"));
            }
            Err(e) => warn!("GetParameter request error {e:?}"),
          }
        }

        get_parameter_types_request = next_if_some(&mut get_parameter_types_stream_opt).fuse() => {
          match get_parameter_types_request {
            Ok( (req_id, req) ) => {
              warn!("Get parameter types request");
              let values = {
                let param_db = self.parameters.lock().unwrap();
                req.names.iter()
                  .map(|name| param_db.get(name.as_str())
                    .unwrap_or(&ParameterValue::NotSet))
                  .map(ParameterValue::to_parameter_type_raw)
                  .collect()
              };
              info!("Get parameter types response: {values:?}");
              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              self.parameter_servers.as_ref().unwrap().get_parameter_types_server
                .async_send_response(req_id, rcl_interfaces::GetParameterTypesResponse{ values })
                .await
                .unwrap_or_else(|e| warn!("GetParameterTypes response error {e:?}"));
            }
            Err(e) => warn!("GetParameterTypes request error {e:?}"),
          }
        }

        set_parameters_request = next_if_some(&mut set_parameters_stream_opt).fuse() => {
          match set_parameters_request {
            Ok( (req_id, req) ) => {
              info!("Set parameter request {req:?}");
              let results =
                req.parameter.iter()
                  .cloned()
                  .map( Parameter::from ) // convert from "raw::Parameter"
                  .map( |Parameter{name, value}| self.set_parameter(&name,value))
                  .map(|r| r.into()) // to "raw" Result for serialization
                  .collect();
              info!("Set parameters response: {results:?}");
              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              self.parameter_servers.as_ref().unwrap().set_parameters_server
                .async_send_response(req_id, rcl_interfaces::SetParametersResponse{ results })
                .await
                .unwrap_or_else(|e| warn!("SetParameters response error {e:?}"));
            }
            Err(e) => warn!("SetParameters request error {e:?}"),
          }
        }

        set_parameters_atomically_request = next_if_some(&mut set_parameters_atomically_stream_opt).fuse() => {
          match set_parameters_atomically_request {
            Ok( (req_id, req) ) => {
              warn!("Set parameters atomically request {req:?}");
              let results =
                req.parameter.iter()
                  .cloned()
                  .map( Parameter::from ) // convert from "raw::Parameter"
                  .map( |Parameter{ .. } |
                      // TODO: Implement atomic setting.
                      Err("Setting parameters atomically is not implemented.".to_owned())
                    )
                  .map(|r| r.into()) // to "raw" Result for serialization
                  .collect();
              warn!("Set parameters atomically response: {results:?}");
              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              self.parameter_servers.as_ref().unwrap().set_parameters_atomically_server
                .async_send_response(req_id, rcl_interfaces::SetParametersAtomicallyResponse{ results })
                .await
                .unwrap_or_else(|e| warn!("SetParameters response error {e:?}"));
            }
            Err(e) => warn!("SetParametersAtomically request error {e:?}"),
          }
        }

        list_parameter_request = next_if_some(&mut list_parameter_stream_opt).fuse() => {
          match list_parameter_request {
            Ok( (req_id, req) ) => {
              info!("List parameters request");
              let prefixes = req.prefixes;
              // TODO: We only generate the "names" part of the ListParametersResponse
              // What should we put into `prefixes` ?
              let names = {
                let param_db = self.parameters.lock().unwrap();
                param_db.keys()
                  .filter_map(|name|
                    if prefixes.is_empty() ||
                      prefixes.iter().any(|prefix| name.starts_with(prefix))
                    {
                      Some(name.clone())
                    } else { None }
                  )
                  .collect()
              };
              let result = rcl_interfaces::ListParametersResult{ names, prefixes: vec![] };
              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              info!("List parameters response: {result:?}");
              self.parameter_servers.as_ref().unwrap().list_parameters_server
                .async_send_response(req_id, rcl_interfaces::ListParametersResponse{ result })
                .await
                .unwrap_or_else(|e| warn!("ListParameter response error {e:?}"));
            }
            Err(e) => warn!("ListParameter request error {e:?}"),
          }
        }

        describe_parameters_request = next_if_some(&mut describe_parameters_stream_opt).fuse() => {
          match describe_parameters_request {
            Ok( (req_id, req) ) => {
              info!("Describe parameters request {req:?}");
              let values = {
                let parameters = self.parameters.lock().unwrap();
                req.names.iter()
                  .map( |name|
                    {
                      if let Some(value) = parameters.get(name) {
                        ParameterDescriptor::from_value(name, value)
                      } else {
                        ParameterDescriptor::unknown(name)
                      }
                    })
                  .map(|r| r.into()) // to "raw" Result for serialization
                  .collect()
              };
              info!("Describe parameters response: {values:?}");
              // .unwrap() below should be safe, as we would not be here if the Server did not exist
              self.parameter_servers.as_ref().unwrap().describe_parameters_server
                .async_send_response(req_id, rcl_interfaces::DescribeParametersResponse{ values })
                .await
                .unwrap_or_else(|e| warn!("DescribeParameters response error {e:?}"));
            }
            Err(e) => warn!("DescribeParameters request error {e:?}"),
          }
        }

        participant_info_update = ros_discovery_stream.select_next_some() => {
          //println!("{:?}", participant_info_update);
          match participant_info_update {
            Ok((part_update, _msg_info)) => {
              // insert to Node-local ros_discovery_info bookkeeping
              let mut info_map = self.external_nodes.lock().unwrap();
              info_map.insert( part_update.gid, part_update.node_entities_info_seq.clone());
              // also notify any status listeneners
              self.send_status_event( &NodeEvent::ROS(part_update) );
            }
            Err(e) => {
              warn!("ros_discovery_info error {e:?}");
            }
          }
        }

        dp_status_event = dds_status_stream.select_next_some() => {
          //println!("{:?}", dp_status_event );

          // update remote reader/writer databases
          match dp_status_event {
            DomainParticipantStatusEvent::RemoteReaderMatched { local_writer, remote_reader } => {
              self.writers_to_remote_readers.lock().unwrap()
                .entry(local_writer)
                .and_modify(|s| {s.insert(remote_reader);} )
                .or_insert(BTreeSet::from([remote_reader]));
            }
            DomainParticipantStatusEvent::RemoteWriterMatched { local_reader, remote_writer } => {
              self.readers_to_remote_writers.lock().unwrap()
                .entry(local_reader)
                .and_modify(|s| {s.insert(remote_writer);} )
                .or_insert(BTreeSet::from([remote_writer]));
            }
            DomainParticipantStatusEvent::ReaderLost {guid, ..} => {
              for ( _local, readers)
              in self.writers_to_remote_readers.lock().unwrap().iter_mut() {
                readers.remove(&guid);
              }
            }
            DomainParticipantStatusEvent::WriterLost {guid, ..} => {
              for ( _local, writers)
              in self.readers_to_remote_writers.lock().unwrap().iter_mut() {
                writers.remove(&guid);
              }
            }

            _ => {}
          }

          // also notify any status listeneners
          self.send_status_event( &NodeEvent::DDS(dp_status_event) );
        }
      }
    }
    info!("Spinner exiting .spin()");
    Ok(())
    //}
  } // fn

  fn send_status_event(&self, event: &NodeEvent) {
    let mut closed = Vec::new();
    let mut sender_array = self.status_event_senders.lock().unwrap();
    for (i, sender) in sender_array.iter().enumerate() {
      match sender.try_send(event.clone()) {
        Ok(()) => {
          // expected result
        }
        Err(async_channel::TrySendError::Closed(_)) => {
          // trace!("Closing {i}");
          closed.push(i) // mark for deletion
        }
        Err(e) => {
          debug!("send_status_event: Send error for {i}: {e:?}");
          // We do not do anything about the error. It may be that the receiver
          // is not interested and the channel is full.
        }
      }
    }

    // remove senders that reported they were closed
    for c in closed.iter().rev() {
      sender_array.swap_remove(*c);
    }
  }

  // Keep this function in sync with the same function in Node.
  fn validate_parameter_on_set(&self, name: &str, value: &ParameterValue) -> SetParametersResult {
    match name {
      // built-in parameter check
      "use_sim_time" => match value {
        ParameterValue::Boolean(_) => Ok(()),
        _ => Err("Parameter'use_sim_time' must be Boolean.".to_owned()),
      },
      // application-defined parameters
      _ => {
        match self.parameter_validator {
          Some(ref v) => v.lock().unwrap()(name, value), // ask the validator to judge
          None => Ok(()),                                // no validator defined, always accept
        }
      }
    }
  }

  // Keep this function in sync with the same function in Node.
  fn execute_parameter_set_actions(
    &self,
    name: &str,
    value: &ParameterValue,
  ) -> SetParametersResult {
    match name {
      "use_sim_time" => match value {
        ParameterValue::Boolean(s) => {
          self.use_sim_time.store(*s, Ordering::SeqCst);
          Ok(())
        }
        _ => Err("Parameter 'use_sim_time' must be Boolean.".to_owned()),
      },
      _ => {
        match self.parameter_set_action {
          Some(ref v) => v.lock().unwrap()(name, value), // execute custom action
          None => Ok(()),                                // no action defined, always accept
        }
      }
    }
  }

  /// Sets a parameter value. Parameter must be declared before setting.
  pub fn set_parameter(&self, name: &str, value: ParameterValue) -> Result<(), String> {
    let already_set = self.parameters.lock().unwrap().contains_key(name);
    if self.allow_undeclared_parameters || already_set {
      self.validate_parameter_on_set(name, &value)?;
      self.execute_parameter_set_actions(name, &value)?;

      // no errors, prepare for sending notificaiton
      let p = raw::Parameter {
        name: name.to_string(),
        value: value.clone().into(),
      };
      let (new_parameters, changed_parameters) = if already_set {
        (vec![], vec![p])
      } else {
        (vec![p], vec![])
      };

      // actually set the parameter
      self
        .parameters
        .lock()
        .unwrap()
        .insert(name.to_owned(), value);
      // and notify
      self
        .parameter_events_writer
        .publish(raw::ParameterEvent {
          timestamp: rustdds::Timestamp::now(), // differs from version in Node!!!
          node: self.fully_qualified_node_name.clone(),
          new_parameters,
          changed_parameters,
          deleted_parameters: vec![],
        })
        .unwrap_or_else(|e| warn!("undeclare_parameter: {e:?}"));
      Ok(())
    } else {
      Err("Setting undeclared parameter '".to_owned() + name + "' is not allowed.")
    }
  }
} // impl Spinner

// ----------------------------------------------------------------------------------------------------
// ----------------------------------------------------------------------------------------------------

/// What went wrong in `Node` creation
#[derive(Debug)]
pub enum NodeCreateError {
  DDS(CreateError),
  BadParameter(String),
}

impl From<CreateError> for NodeCreateError {
  fn from(c: CreateError) -> NodeCreateError {
    NodeCreateError::DDS(c)
  }
}

impl fmt::Display for NodeCreateError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Self::DDS(create_error) => write!(f, "NodeCreateError::DDS : {create_error}"),
      Self::BadParameter(s) => write!(f, "NodeCreateError::BadParameter : {s}"),
    }
  }
}

impl Error for NodeCreateError {
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    match self {
      Self::DDS(create_error) => Some(create_error),
      Self::BadParameter(_) => None,
    }
  }
}

/// Error when setting `Parameter`s
pub enum ParameterError {
  AlreadyDeclared,
  InvalidName,
}

/// Node in ROS2 network. Holds necessary readers and writers for rosout and
/// parameter events topics internally.
///
/// These are produced by a [`Context`].

// TODO: We should notify ROS discovery when readers or writers are removed, but
// now we do not do that.
pub struct Node {
  node_name: NodeName,
  options: NodeOptions,

  pub(crate) ros_context: Context,

  // sets of Readers and Writers belonging to ( = created via) this Node
  // These indicate what has been created locally.
  readers: BTreeSet<Gid>,
  writers: BTreeSet<Gid>,

  suppress_node_info_updates: Arc<AtomicBool>,
  // temporarily suppress sending updates
  // to prevent flood of messages. TODO: not shared: need not be atomic or Arc.

  // Keep track of who is matched via DDS Discovery
  // Map keys are lists of local Subscriptions and Publishers.
  // Map values are lists of matched Publishers / Subscriptions.
  readers_to_remote_writers: Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>,
  writers_to_remote_readers: Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>,

  // Keep track of ros_discovery_info
  external_nodes: Arc<Mutex<BTreeMap<Gid, Vec<NodeEntitiesInfo>>>>,
  stop_spin_sender: Option<async_channel::Sender<()>>,

  // Channels to report discovery events to
  status_event_senders: Arc<Mutex<Vec<async_channel::Sender<NodeEvent>>>>,

  // builtin writers and readers
  rosout_writer: Option<Publisher<Log>>,
  rosout_reader: Option<Subscription<Log>>,

  // Parameter events (rcl_interfaces)
  // Parameter Services are inside Spinner
  parameter_events_writer: Arc<Publisher<raw::ParameterEvent>>,

  // Parameter store
  parameters: Arc<Mutex<BTreeMap<String, ParameterValue>>>,
  // allow_undeclared_parameters: bool, // this is inside "options"
  parameter_validator: Option<Arc<Mutex<Box<ParameterFunc>>>>,
  parameter_set_action: Option<Arc<Mutex<Box<ParameterFunc>>>>,

  // simulated ROSTime
  use_sim_time: Arc<AtomicBool>,
  sim_time: Arc<Mutex<ROSTime>>,
}

impl Node {
  pub(crate) fn new(
    node_name: NodeName,
    mut options: NodeOptions,
    ros_context: Context,
  ) -> Result<Node, NodeCreateError> {
    let paramtopic = ros_context.get_parameter_events_topic();
    let rosout_topic = ros_context.get_rosout_topic();

    let enable_rosout = options.enable_rosout;
    let rosout_reader = options.enable_rosout_reading;

    let parameter_events_writer = ros_context.create_publisher(&paramtopic, None)?;

    // TODO: If there are duplicates, the later one will overwrite the earlier, but
    // there is no warning or error.
    options.declared_parameters.push(Parameter {
      name: "use_sim_time".to_string(),
      value: ParameterValue::Boolean(false),
    });
    let parameters = options
      .declared_parameters
      .iter()
      .cloned()
      .map(|Parameter { name, value }| (name, value))
      .collect::<BTreeMap<String, ParameterValue>>();

    let parameter_validator = options
      .parameter_validator
      .take()
      .map(|b| Arc::new(Mutex::new(b)));
    let parameter_set_action = options
      .parameter_set_action
      .take()
      .map(|b| Arc::new(Mutex::new(b)));

    let mut node = Node {
      node_name,
      options,
      ros_context,
      readers: BTreeSet::new(),
      writers: BTreeSet::new(),
      readers_to_remote_writers: Arc::new(Mutex::new(BTreeMap::new())),
      writers_to_remote_readers: Arc::new(Mutex::new(BTreeMap::new())),
      external_nodes: Arc::new(Mutex::new(BTreeMap::new())),
      suppress_node_info_updates: Arc::new(AtomicBool::new(false)),
      stop_spin_sender: None,
      status_event_senders: Arc::new(Mutex::new(Vec::new())),
      rosout_writer: None, // Set below
      rosout_reader: None,
      parameter_events_writer: Arc::new(parameter_events_writer),
      parameters: Arc::new(Mutex::new(parameters)),
      parameter_validator,
      parameter_set_action,
      use_sim_time: Arc::new(AtomicBool::new(false)),
      sim_time: Arc::new(Mutex::new(ROSTime::ZERO)),
    };

    node.suppress_node_info_updates(true);

    node.rosout_writer = if enable_rosout {
      Some(
        // topic already has QoS defined
        node.create_publisher(&rosout_topic, None)?,
      )
    } else {
      None
    };
    node.rosout_reader = if rosout_reader {
      Some(node.create_subscription(&rosout_topic, None)?)
    } else {
      None
    };

    // returns `Err` if some parameter does not validate.
    node
      .parameters
      .lock()
      .unwrap()
      .iter()
      .try_for_each(|(name, value)| {
        node.validate_parameter_on_set(name, value)?;
        node.execute_parameter_set_actions(name, value)?;
        Ok(())
      })
      .map_err(NodeCreateError::BadParameter)?;

    node.suppress_node_info_updates(false);

    Ok(node)
  }

  /// Return the ROSTime
  ///
  /// It is either the system clock time
  pub fn time_now(&self) -> ROSTime {
    if self.use_sim_time.load(Ordering::SeqCst) {
      *self.sim_time.lock().unwrap()
    } else {
      ROSTime::now()
    }
  }

  pub fn time_now_not_simulated(&self) -> ROSTime {
    ROSTime::now()
  }

  /// Create a Spinner object to execute Node backround tasks.
  ///
  /// An async task should then be created to run the `.spin()` function of
  /// `Spinner`.
  ///
  /// E.g. `executor.spawn(node.spinner().spin())`
  ///
  /// The `.spin()` task runs until `Node` is dropped.
  pub fn spinner(&mut self) -> CreateResult<Spinner> {
    if self.stop_spin_sender.is_some() {
      panic!("Attempted to crate a second spinner.");
    }
    let (stop_spin_sender, stop_spin_receiver) = async_channel::bounded(1);
    self.stop_spin_sender = Some(stop_spin_sender);

    //TODO: Check QoS policies against ROS 2 specs or some refernce.
    let service_qos = QosPolicyBuilder::new()
      .reliability(policy::Reliability::Reliable {
        max_blocking_time: Duration::from_millis(100),
      })
      .history(policy::History::KeepLast { depth: 1 })
      .build();

    let node_name = self.node_name.fully_qualified_name();

    self.suppress_node_info_updates(true);

    let parameter_servers = if self.options.start_parameter_services {
      let service_mapping = ServiceMapping::Enhanced; //TODO: parameterize
      let get_parameters_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "get_parameters").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "GetParameters"),
        service_qos.clone(),
        service_qos.clone(),
      )?;
      let get_parameter_types_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "get_parameter_types").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "GetParameterTypes"),
        service_qos.clone(),
        service_qos.clone(),
      )?;
      let set_parameters_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "set_parameters").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "SetParameters"),
        service_qos.clone(),
        service_qos.clone(),
      )?;
      let set_parameters_atomically_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "set_parameters_atomically").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "SetParametersAtomically"),
        service_qos.clone(),
        service_qos.clone(),
      )?;
      let list_parameters_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "list_parameters").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "ListParameters"),
        service_qos.clone(),
        service_qos.clone(),
      )?;
      let describe_parameters_server = self.create_server(
        service_mapping,
        &Name::new(&node_name, "describe_parameters").unwrap(),
        &ServiceTypeName::new("rcl_interfaces", "DescribeParameters"),
        service_qos.clone(),
        service_qos.clone(),
      )?;

      Some(ParameterServers {
        get_parameters_server,
        get_parameter_types_server,
        list_parameters_server,
        set_parameters_server,
        set_parameters_atomically_server,
        describe_parameters_server,
      })
    } else {
      None // No parameter services
    };

    let clock_topic = self.create_topic(
      &Name::new("/", "clock").unwrap(),
      MessageTypeName::new("builtin_interfaces", "Time"),
      &DEFAULT_SUBSCRIPTION_QOS,
    )?;

    self.suppress_node_info_updates(false);

    Ok(Spinner {
      ros_context: self.ros_context.clone(),
      stop_spin_receiver,
      readers_to_remote_writers: Arc::clone(&self.readers_to_remote_writers),
      writers_to_remote_readers: Arc::clone(&self.writers_to_remote_readers),
      external_nodes: Arc::clone(&self.external_nodes),
      status_event_senders: Arc::clone(&self.status_event_senders),
      use_sim_time: Arc::clone(&self.use_sim_time),
      sim_time: Arc::clone(&self.sim_time),
      clock_topic,
      parameter_servers,
      parameter_events_writer: Arc::clone(&self.parameter_events_writer),
      parameters: Arc::clone(&self.parameters),
      allow_undeclared_parameters: self.options.allow_undeclared_parameters,
      parameter_validator: self.parameter_validator.as_ref().map(Arc::clone),
      parameter_set_action: self.parameter_set_action.as_ref().map(Arc::clone),
      fully_qualified_node_name: self.fully_qualified_name(),
    })
  }

  /// A heuristic to detect if a spinner has been created.
  /// But this does still not guarantee that it is running, i.e.
  /// an async excutor is runnning spinner.spin(), but this is the best we can
  /// do.
  pub fn have_spinner(&self) -> bool {
    self.stop_spin_sender.is_some()
  }

  // Generates ROS2 node info from added readers and writers.
  fn generate_node_info(&self) -> NodeEntitiesInfo {
    let mut node_info = NodeEntitiesInfo::new(self.node_name.clone());

    node_info.add_writer(Gid::from(self.parameter_events_writer.guid()));
    if let Some(row) = &self.rosout_writer {
      node_info.add_writer(Gid::from(row.guid()));
    }

    for reader in &self.readers {
      node_info.add_reader(*reader);
    }

    for writer in &self.writers {
      node_info.add_writer(*writer);
    }

    node_info
  }

  fn suppress_node_info_updates(&mut self, suppress: bool) {
    self
      .suppress_node_info_updates
      .store(suppress, Ordering::SeqCst);

    // Send updates when suppression ends
    if !suppress {
      self.ros_context.update_node(self.generate_node_info());
    }
  }

  fn add_reader(&mut self, reader: Gid) {
    self.readers.insert(reader);
    if !self.suppress_node_info_updates.load(Ordering::SeqCst) {
      self.ros_context.update_node(self.generate_node_info());
    }
  }

  fn add_writer(&mut self, writer: Gid) {
    self.writers.insert(writer);
    if !self.suppress_node_info_updates.load(Ordering::SeqCst) {
      self.ros_context.update_node(self.generate_node_info());
    }
  }

  pub fn base_name(&self) -> &str {
    self.node_name.base_name()
  }

  pub fn namespace(&self) -> &str {
    self.node_name.namespace()
  }

  pub fn fully_qualified_name(&self) -> String {
    self.node_name.fully_qualified_name()
  }

  pub fn options(&self) -> &NodeOptions {
    &self.options
  }

  pub fn domain_id(&self) -> u16 {
    self.ros_context.domain_id()
  }

  // ///////////////////////////////////////////////
  // Parameters

  pub fn undeclare_parameter(&self, name: &str) {
    let prev_value = self.parameters.lock().unwrap().remove(name);

    if let Some(deleted_param) = prev_value {
      // a parameter was actually undeclared. Let others know.
      self
        .parameter_events_writer
        .publish(raw::ParameterEvent {
          timestamp: self.time_now().into(),
          node: self.fully_qualified_name(),
          new_parameters: vec![],
          changed_parameters: vec![],
          deleted_parameters: vec![raw::Parameter {
            name: name.to_string(),
            value: deleted_param.into(),
          }],
        })
        .unwrap_or_else(|e| warn!("undeclare_parameter: {e:?}"));
    }
  }

  /// Does the parameter exist?
  pub fn has_parameter(&self, name: &str) -> bool {
    self.parameters.lock().unwrap().contains_key(name)
  }

  /// Sets a parameter value. Parameter must be declared before setting.
  //
  // TODO: This code is duplicated in Spinner. Not good.
  // Find a way to de-duplicate.
  // Same for validate_parameter_on_set and execute_parameter_set_actions.
  // TODO: This does not account for built-in parameters e.g. "use_sim_time".
  // It thinks they are new on first set.
  // TODO: Setting Parameter to type NotSet counts as parameter deletion. Maybe
  // that needs special handling? At least for notifications.
  pub fn set_parameter(&self, name: &str, value: ParameterValue) -> Result<(), String> {
    let already_set = self.parameters.lock().unwrap().contains_key(name);
    if self.options.allow_undeclared_parameters || already_set {
      self.validate_parameter_on_set(name, &value)?;
      self.execute_parameter_set_actions(name, &value)?;

      // no errors, prepare for sending notificaiton
      let p = raw::Parameter {
        name: name.to_string(),
        value: value.clone().into(),
      };
      let (new_parameters, changed_parameters) = if already_set {
        (vec![], vec![p])
      } else {
        (vec![p], vec![])
      };

      // actually set the parameter
      self
        .parameters
        .lock()
        .unwrap()
        .insert(name.to_owned(), value);
      // and notify
      self
        .parameter_events_writer
        .publish(raw::ParameterEvent {
          timestamp: self.time_now().into(),
          node: self.fully_qualified_name(),
          new_parameters,
          changed_parameters,
          deleted_parameters: vec![],
        })
        .unwrap_or_else(|e| warn!("undeclare_parameter: {e:?}"));
      Ok(())
    } else {
      Err("Setting undeclared parameter '".to_owned() + name + "' is not allowed.")
    }
  }

  pub fn allow_undeclared_parameters(&self) -> bool {
    self.options.allow_undeclared_parameters
  }

  /// Gets the value of a parameter, or None is there is no such Parameter.
  pub fn get_parameter(&self, name: &str) -> Option<ParameterValue> {
    self
      .parameters
      .lock()
      .unwrap()
      .get(name)
      .map(|p| p.to_owned())
  }

  pub fn list_parameters(&self) -> Vec<String> {
    self
      .parameters
      .lock()
      .unwrap()
      .keys()
      .map(move |k| k.to_owned())
      .collect::<Vec<_>>()
  }

  // Keep this function in sync with the same function in Spinner.
  // TODO: This should refuse to change parameter type, unless
  // there is a ParamaterDescription defined and it allows
  // changing type.
  // TODO: Setting Parameter to type NotSet counts as parameter deletion. Maybe
  // that needs special handling?
  fn validate_parameter_on_set(&self, name: &str, value: &ParameterValue) -> SetParametersResult {
    match name {
      // built-in parameter check
      "use_sim_time" => match value {
        ParameterValue::Boolean(_) => Ok(()),
        _ => Err("Parameter'use_sim_time' must be Boolean.".to_owned()),
      },
      // application-defined parameters
      _ => {
        match self.parameter_validator {
          Some(ref v) => v.lock().unwrap()(name, value), // ask the validator to judge
          None => Ok(()),                                // no validator defined, always accept
        }
      }
    }
  }

  // Keep this function in sync with the same function in Spinner.
  fn execute_parameter_set_actions(
    &self,
    name: &str,
    value: &ParameterValue,
  ) -> SetParametersResult {
    match name {
      "use_sim_time" => match value {
        ParameterValue::Boolean(s) => {
          self.use_sim_time.store(*s, Ordering::SeqCst);
          Ok(())
        }
        _ => Err("Parameter 'use_sim_time' must be Boolean.".to_owned()),
      },
      _ => {
        match self.parameter_set_action {
          Some(ref v) => v.lock().unwrap()(name, value), // execute custom action
          None => Ok(()),                                // no action defined, always accept
        }
      }
    }
  }

  // ///////////////////////////////////////////////////

  /// Get an async Receiver for discovery events.
  ///
  /// There must be an async task executing `spin` to get any data.
  /// This function may panic if there is no Spinner running.
  pub fn status_receiver(&self) -> Receiver<NodeEvent> {
    if self.have_spinner() {
      let (status_event_sender, status_event_receiver) = async_channel::bounded(8);
      self
        .status_event_senders
        .lock()
        .unwrap()
        .push(status_event_sender);
      status_event_receiver
    } else {
      panic!("status_receiver() cannot set up a receiver, because no Spinner is running.")
    }
  }

  // reader waits for at least one writer to be present
  pub(crate) fn wait_for_writer(&self, reader: GUID) -> impl Future<Output = ()> {
    // TODO: This may contain some synchrnoization hazard
    let status_receiver = self.status_receiver();

    let already_present = self
      .readers_to_remote_writers
      .lock()
      .unwrap()
      .get(&reader)
      .map(|writers| !writers.is_empty()) // there is someone matched
      .unwrap_or(false); // we do not even know the reader

    if already_present {
      WriterWait::Ready
    } else {
      WriterWait::Wait {
        this_reader: reader,
        status_event_stream: Box::pin(status_receiver),
      }
    }
  }

  pub(crate) fn wait_for_reader(&self, writer: GUID) -> impl Future<Output = ()> {
    // TODO: This may contain some synchrnoization hazard.
    let status_receiver = self.status_receiver();

    let already_present = self
      .writers_to_remote_readers
      .lock()
      .unwrap()
      .get(&writer)
      .map(|readers| !readers.is_empty()) // there is someone matched
      .unwrap_or(false); // we do not even know who is asking

    // TODO: Is is possible to miss reader events if they appear after the check
    // above, but do not somehow end up in the status_receiver stream?

    if already_present {
      info!("wait_for_reader: Already have matched a reader.");
      ReaderWait::Ready
    } else {
      ReaderWait::Wait {
        this_writer: writer,
        status_event_stream: Box::pin(status_receiver),
      }
    }
  }

  pub(crate) fn get_publisher_count(&self, subscription_guid: GUID) -> usize {
    self
      .readers_to_remote_writers
      .lock()
      .unwrap()
      .get(&subscription_guid)
      .map(BTreeSet::len)
      .unwrap_or_else(|| {
        error!("get_publisher_count: Subscriber {subscription_guid:?} not known to node.");
        0
      })
  }

  pub(crate) fn get_subscription_count(&self, publisher_guid: GUID) -> usize {
    self
      .writers_to_remote_readers
      .lock()
      .unwrap()
      .get(&publisher_guid)
      .map(BTreeSet::len)
      .unwrap_or_else(|| {
        error!("get_subscription_count: Publisher {publisher_guid:?} not known to node.");
        0
      })
  }

  /// Borrow the Subscription to our ROSOut Reader.
  ///
  /// Availability depends on Node configuration.
  pub fn rosout_subscription(&self) -> Option<&Subscription<Log>> {
    self.rosout_reader.as_ref()
  }

  #[allow(clippy::too_many_arguments)]
  pub fn rosout_raw(
    &self,
    timestamp: Timestamp,
    level: crate::ros2::LogLevel,
    log_name: &str,
    log_msg: &str,
    source_file: &str,
    source_function: &str,
    source_line: u32,
  ) {
    match &self.rosout_writer {
      None => debug!("Rosout not enabled. msg: {log_msg}"),
      Some(writer) => {
        writer
          .publish(ros_log::Log {
            timestamp,
            level: level as u8,
            name: log_name.to_string(),
            msg: log_msg.to_string(),
            file: source_file.to_string(),
            function: source_function.to_string(),
            line: source_line,
          })
          .unwrap_or_else(|e| debug!("Rosout publish failed: {e:?}"));
      }
    };

    let tracing_msg = format!("[rosout] ({log_name}) {log_msg}");

    // make a span to filter out all the other stuff this lib puts out
    let span = tracing::span!(tracing::Level::ERROR, "rosout_raw");
    let _guard = span.enter();

    match level {
      ros_log::LogLevel::Fatal | ros_log::LogLevel::Error => tracing::error!("{tracing_msg}"),
      ros_log::LogLevel::Warn => tracing::warn!("{tracing_msg}"),
      ros_log::LogLevel::Info => tracing::info!("{tracing_msg}"),
      ros_log::LogLevel::Debug => tracing::debug!("{tracing_msg}"),
    };
  }

  /// Creates ROS2 topic and handles necessary conversions from DDS to ROS2
  ///
  /// # Arguments
  ///
  /// * `domain_participant` -
  ///   [DomainParticipant](../dds/struct.DomainParticipant.html)
  /// * `name` - Name of the topic
  /// * `type_name` - What type the topic holds in string form
  /// * `qos` - Quality of Service parameters for the topic (not restricted only
  ///   to ROS2)
  ///
  ///  
  ///   [summary of all rules for topic and service names in ROS 2](https://design.ros2.org/articles/topic_and_service_names.html)
  ///   (as of Dec 2020)
  ///
  /// * must not be empty
  /// * may contain alphanumeric characters ([0-9|a-z|A-Z]), underscores (_), or
  ///   forward slashes (/)
  /// * may use balanced curly braces ({}) for substitutions
  /// * may start with a tilde (~), the private namespace substitution character
  /// * must not start with a numeric character ([0-9])
  /// * must not end with a forward slash (/)
  /// * must not contain any number of repeated forward slashes (/)
  /// * must not contain any number of repeated underscores (_)
  /// * must separate a tilde (~) from the rest of the name with a forward slash
  ///   (/), i.e. ~/foo not ~foo
  /// * must have balanced curly braces ({}) when used, i.e. {sub}/foo but not
  ///   {sub/foo nor /foo}
  pub fn create_topic(
    &self,
    topic_name: &Name,
    type_name: MessageTypeName,
    qos: &QosPolicies,
  ) -> CreateResult<Topic> {
    let dds_name = topic_name.to_dds_name("rt", &self.node_name, "");
    self.ros_context.create_topic(dds_name, type_name, qos)
  }

  /// Creates ROS2 Subscriber
  ///
  /// # Arguments
  ///
  /// * `topic` - Reference to topic created with `create_ros_topic`.
  /// * `qos` - Should take [QOS](../dds/qos/struct.QosPolicies.html) and use if
  ///   it's compatible with topics QOS. `None` indicates the use of Topics QOS.
  pub fn create_subscription<D: 'static>(
    &mut self,
    topic: &Topic,
    qos: Option<QosPolicies>,
  ) -> CreateResult<Subscription<D>> {
    let sub = self.ros_context.create_subscription(topic, qos)?;
    self.add_reader(sub.guid().into());
    Ok(sub)
  }

  /// Creates ROS2 Publisher
  ///
  /// # Arguments
  ///
  /// * `topic` - Reference to topic created with `create_ros_topic`.
  /// * `qos` - Should take [QOS](../dds/qos/struct.QosPolicies.html) and use it
  ///   if it's compatible with topics QOS. `None` indicates the use of Topics
  ///   QOS.
  pub fn create_publisher<D: Serialize>(
    &mut self,
    topic: &Topic,
    qos: Option<QosPolicies>,
  ) -> CreateResult<Publisher<D>> {
    let p = self.ros_context.create_publisher(topic, qos)?;
    self.add_writer(p.guid().into());
    Ok(p)
  }

  pub(crate) fn create_simpledatareader<D, DA>(
    &mut self,
    topic: &Topic,
    qos: Option<QosPolicies>,
  ) -> CreateResult<no_key::SimpleDataReader<D, DA>>
  where
    D: 'static,
    DA: rustdds::no_key::DeserializerAdapter<D> + 'static,
  {
    let r = self.ros_context.create_simpledatareader(topic, qos)?;
    self.add_reader(r.guid().into());
    Ok(r)
  }

  pub(crate) fn create_datawriter<D, SA>(
    &mut self,
    topic: &Topic,
    qos: Option<QosPolicies>,
  ) -> CreateResult<no_key::DataWriter<D, SA>>
  where
    SA: rustdds::no_key::SerializerAdapter<D>,
  {
    let w = self.ros_context.create_datawriter(topic, qos)?;
    self.add_writer(w.guid().into());
    Ok(w)
  }

  /// Creates ROS2 Service Client
  ///
  /// # Arguments
  ///
  /// * `service_mapping` - ServiceMapping to be used
  /// * `service_name` -
  /// * `qos`-
  pub fn create_client<S>(
    &mut self,
    service_mapping: ServiceMapping,
    service_name: &Name,
    service_type_name: &ServiceTypeName,
    request_qos: QosPolicies,
    response_qos: QosPolicies,
  ) -> CreateResult<Client<S>>
  where
    S: Service + 'static,
    S::Request: Clone,
  {
    // Add rq/ and rr/ prefixes as documented in
    // https://design.ros2.org/articles/topic_and_service_names.html
    // Where are the suffixes documented?
    // And why "Reply" and not "Response" ?

    let rq_topic = self.ros_context.domain_participant().create_topic(
      service_name.to_dds_name("rq", &self.node_name, "Request"),
      //rq_name,
      service_type_name.dds_request_type(),
      &request_qos,
      TopicKind::NoKey,
    )?;
    let rs_topic = self.ros_context.domain_participant().create_topic(
      service_name.to_dds_name("rr", &self.node_name, "Reply"),
      //rs_name,
      service_type_name.dds_response_type(),
      &response_qos,
      TopicKind::NoKey,
    )?;

    let c = Client::<S>::new(
      service_mapping,
      self,
      &rq_topic,
      &rs_topic,
      Some(request_qos),
      Some(response_qos),
    )?;

    Ok(c)
  }

  /// Creates ROS2 Service Server
  ///
  /// # Arguments
  ///
  /// * `service_mapping` - ServiceMapping to be used. See
  ///   [`Self.create_client`].
  /// * `service_name` -
  /// * `qos`-
  pub fn create_server<S>(
    &mut self,
    service_mapping: ServiceMapping,
    service_name: &Name,
    service_type_name: &ServiceTypeName,
    request_qos: QosPolicies,
    response_qos: QosPolicies,
  ) -> CreateResult<Server<S>>
  where
    S: Service + 'static,
    S::Request: Clone,
  {
    // let rq_name = Self::check_name_and_add_prefix("rq/",
    // &(service_name.to_owned() + "Request"))?; let rs_name =
    // Self::check_name_and_add_prefix("rr/", &(service_name.to_owned() +
    // "Reply"))?;

    let rq_topic = self.ros_context.domain_participant().create_topic(
      //rq_name,
      service_name.to_dds_name("rq", &self.node_name, "Request"),
      service_type_name.dds_request_type(),
      &request_qos,
      TopicKind::NoKey,
    )?;
    let rs_topic = self.ros_context.domain_participant().create_topic(
      service_name.to_dds_name("rr", &self.node_name, "Reply"),
      service_type_name.dds_response_type(),
      &response_qos,
      TopicKind::NoKey,
    )?;

    let s = Server::<S>::new(
      service_mapping,
      self,
      &rq_topic,
      &rs_topic,
      Some(request_qos),
      Some(response_qos),
    )?;

    Ok(s)
  }

  pub fn create_action_client<A>(
    &mut self,
    service_mapping: ServiceMapping,
    action_name: &Name,
    action_type_name: &ActionTypeName,
    action_qos: ActionClientQosPolicies,
  ) -> CreateResult<ActionClient<A>>
  where
    A: ActionTypes + 'static,
  {
    // action name is e.g. "/turtle1/rotate_absolute"
    // action type name is e.g. "turtlesim/action/RotateAbsolute"
    let services_base_name = action_name.push("_action");

    //let goal_service_name = action_name.to_owned() + "/_action/send_goal";
    let goal_service_type = action_type_name.dds_action_service("_SendGoal");
    let my_goal_client = self.create_client(
      service_mapping,
      //&goal_service_name,
      &services_base_name.push("send_goal"),
      &goal_service_type,
      action_qos.goal_service.clone(),
      action_qos.goal_service,
    )?;

    //let cancel_service_name = action_name.to_owned() + "/_action/cancel_goal";
    let cancel_goal_type = ServiceTypeName::new("action_msgs", "CancelGoal");
    let my_cancel_client = self.create_client(
      service_mapping,
      //&cancel_service_name,
      &services_base_name.push("cancel_goal"),
      &cancel_goal_type,
      action_qos.cancel_service.clone(),
      action_qos.cancel_service,
    )?;

    //let result_service_name = action_name.to_owned() + "/_action/get_result";
    let result_service_type = action_type_name.dds_action_service("_GetResult");
    let my_result_client = self.create_client(
      service_mapping,
      //&result_service_name,
      &services_base_name.push("get_result"),
      &result_service_type,
      action_qos.result_service.clone(),
      action_qos.result_service,
    )?;

    let action_topic_namespace = action_name.push("_action");

    let feedback_topic_type = action_type_name.dds_action_topic("_FeedbackMessage");
    let feedback_topic = self.create_topic(
      &action_topic_namespace.push("feedback"),
      feedback_topic_type,
      &action_qos.feedback_subscription,
    )?;
    let my_feedback_subscription =
      self.create_subscription(&feedback_topic, Some(action_qos.feedback_subscription))?;

    //let status_topic_type = ;
    let status_topic = self.create_topic(
      &action_topic_namespace.push("status"),
      MessageTypeName::new("action_msgs", "GoalStatusArray"),
      &action_qos.status_subscription,
    )?;
    let my_status_subscription =
      self.create_subscription(&status_topic, Some(action_qos.status_subscription))?;

    Ok(ActionClient {
      my_goal_client,
      my_cancel_client,
      my_result_client,
      my_feedback_subscription,
      my_status_subscription,
      my_action_name: action_name.clone(),
    })
  }

  pub fn create_action_server<A>(
    &mut self,
    service_mapping: ServiceMapping,
    action_name: &Name,
    action_type_name: &ActionTypeName,
    action_qos: ActionServerQosPolicies,
  ) -> CreateResult<ActionServer<A>>
  where
    A: ActionTypes + 'static,
  {
    let services_base_name = action_name.push("_action");

    //let goal_service_name = action_name.to_owned() + "/_action/send_goal";
    let goal_service_type = action_type_name.dds_action_service("_SendGoal");
    let my_goal_server = self.create_server(
      service_mapping,
      //&goal_service_name,
      &services_base_name.push("send_goal"),
      &goal_service_type,
      action_qos.goal_service.clone(),
      action_qos.goal_service,
    )?;

    //let cancel_service_name = action_name.to_owned() + "/_action/cancel_goal";
    let cancel_service_type = ServiceTypeName::new("action_msgs", "CancelGoal");
    let my_cancel_server = self.create_server(
      service_mapping,
      //&cancel_service_name,
      &services_base_name.push("cancel_goal"),
      &cancel_service_type,
      action_qos.cancel_service.clone(),
      action_qos.cancel_service,
    )?;

    //let result_service_name = action_name.to_owned() + "/_action/get_result";
    let result_service_type = action_type_name.dds_action_service("_GetResult");
    let my_result_server = self.create_server(
      service_mapping,
      //&result_service_name,
      &services_base_name.push("get_result"),
      &result_service_type,
      action_qos.result_service.clone(),
      action_qos.result_service,
    )?;

    let action_topic_namespace = action_name.push("_action");

    let feedback_topic_type = action_type_name.dds_action_topic("_FeedbackMessage");
    let feedback_topic = self.create_topic(
      &action_topic_namespace.push("feedback"),
      feedback_topic_type,
      &action_qos.feedback_publisher,
    )?;
    let my_feedback_publisher =
      self.create_publisher(&feedback_topic, Some(action_qos.feedback_publisher))?;

    let status_topic_type = MessageTypeName::new("action_msgs", "GoalStatusArray");
    let status_topic = self.create_topic(
      &action_topic_namespace.push("status"),
      status_topic_type,
      &action_qos.status_publisher,
    )?;
    let my_status_publisher =
      self.create_publisher(&status_topic, Some(action_qos.status_publisher))?;

    Ok(ActionServer {
      my_goal_server,
      my_cancel_server,
      my_result_server,
      my_feedback_publisher,
      my_status_publisher,
      my_action_name: action_name.clone(),
    })
  }
} // impl Node

impl Drop for Node {
  fn drop(&mut self) {
    if let Some(ref stop_spin_sender) = self.stop_spin_sender {
      stop_spin_sender
        .try_send(())
        .unwrap_or_else(|e| error!("Cannot notify spin task to stop: {e:?}"));
    }

    self
      .ros_context
      .remove_node(self.fully_qualified_name().as_str());
  }
}

/// Macro for writing to [rosout](https://wiki.ros.org/rosout) topic.
///
/// # Example
///
/// ```
/// # use ros2_client::*;
/// #
/// # let context = Context::new().unwrap();
/// # let mut node = context
/// #     .new_node(
/// #       NodeName::new("/", "some_node").unwrap(),
/// #       NodeOptions::new().enable_rosout(true),
/// #     )
/// #     .unwrap();
/// let kind = "silly";
///
/// rosout!(node, ros2::LogLevel::Info, "A {} event was seen.", kind);
/// ```
#[macro_export]
macro_rules! rosout {
    // rosout!(node, Level::Info, "a {} event", event.kind);

    ($node:expr, $lvl:expr, $($arg:tt)+) => (
        $node.rosout_raw(
            $crate::ros2::Timestamp::now(),
            $lvl,
            $node.base_name(),
            &std::format!($($arg)+), // msg
            std::file!(),
            "<unknown_func>", // is there a macro to get current function name? (Which may be undefined)
            std::line!(),
        );
    );
}

/// Future type for waiting Readers to appear over ROS2 Topic.
///
/// Produced by `node.wait_for_reader(writer_guid)`
//
// This is implemented as a separate struct instead of just async function in
// Node so that it does not borrow the node and thus can be Send.
//use pin_project::pin_project;
pub enum ReaderWait<'a> {
  // We need to wait for an event that is for us
  Wait {
    this_writer: GUID, // Writer who is waiting for Readers to appear
    status_event_stream: stream::BoxStream<'a, NodeEvent>,
  },
  // No need to wait, can resolve immediately.
  Ready,
}

impl Future for ReaderWait<'_> {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
    match *self {
      ReaderWait::Ready => Poll::Ready(()),

      ReaderWait::Wait {
        this_writer,
        ref mut status_event_stream,
      } => {
        debug!("wait_for_reader: Waiting for a reader.");
        loop {
          match status_event_stream.poll_next_unpin(cx) {
            // Check if we have RemoteReaderMatched event and it is for this_writer
            Poll::Ready(Some(NodeEvent::DDS(
              DomainParticipantStatusEvent::RemoteReaderMatched {
                local_writer,
                remote_reader,
              },
            )))
              if local_writer == this_writer =>
            {
              debug!("wait_for_reader: Matched remote reader {remote_reader:?}.");
              return Poll::Ready(());
            }

            Poll::Ready(_) => {
              // Received something else, such as other event or error
              debug!("wait_for_reader: other event. Continue polling.");
              // So we do nothing but go to the next iteration.
            }

            Poll::Pending => return Poll::Pending,
          }
        }
      }
    }
  }
}

/// Future type for waiting Writers to appear over ROS2 Topic.
///
/// Produced by `node.wait_for_writer(writer_guid)`
//
// This is implemented as a separate struct instead of just async function in
// Node so that it does not borrow the node and thus can be Send.
pub enum WriterWait<'a> {
  // We need to wait for an event that is for us
  Wait {
    this_reader: GUID,
    status_event_stream: stream::BoxStream<'a, NodeEvent>,
  },
  // No need to wait, can resolve immediately.
  Ready,
}

impl Future for WriterWait<'_> {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
    match *self {
      WriterWait::Ready => Poll::Ready(()),

      WriterWait::Wait {
        this_reader,
        ref mut status_event_stream,
      } => {
        debug!("wait_for_writer: Waiting for a writer.");
        loop {
          // We loop to pump events out of the stream until we get the desired event
          // or "Pending". If we stop at the first event, then there is no waker
          // installed and we are stuck.
          match status_event_stream.poll_next_unpin(cx) {
            // Check if we have RemoteWriterMatched event and it is for this_writer
            Poll::Ready(Some(NodeEvent::DDS(
              DomainParticipantStatusEvent::RemoteWriterMatched {
                local_reader,
                remote_writer,
              },
            )))
              if local_reader == this_reader =>
            {
              debug!("wait_for_writer: Matched remote writer {remote_writer:?}.");
              return Poll::Ready(());
            }

            Poll::Ready(_) => {
              // Received something else, such as other event or error
              trace!("=== other writer. Continue polling.");
              // No return, go to next iteration.
            }

            Poll::Pending => return Poll::Pending,
          }
        }
      }
    }
  }
}

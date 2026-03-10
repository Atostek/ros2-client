use std::{cell::Cell, io};

use async_ctrlc::CtrlC;
use futures::{channel::oneshot, StreamExt};
use log::{error, info};
use ros2_client::{
  action::{self, ActionClient},
  ros2, rosout,
  service::CallServiceError,
  AService, Action, ActionTypeName, Client, Context, Message, MessageTypeName, Name, Node,
  NodeName, NodeOptions, Publisher, ServiceMapping, ServiceTypeName, Subscription,
};
use rustdds::{policy, QosPolicyBuilder};
use serde::{Deserialize, Serialize};
use smol::{channel, pin, LocalExecutor};
use tokio::select;

mod ui;

fn main() {
  // Here is a fixed path, so this example must be started from repository root
  // directory.
  log4rs::init_file("examples/turtle_teleop/log4rs.yaml", Default::default())
    .expect("example was not started from the project root directory");

  let mut app = App::new();
  let exec = LocalExecutor::new();

  smol::block_on(exec.run(async {
    select! {
      _ = app.run(&exec) => {
        info!("process finished");
      }
      _ = CtrlC::new().unwrap() => {
        info!("process terminated");
      }
    };
  }));
}

struct App {
  requester: Requester,
  turtle_cmd_vel_reader: Subscription<Twist>,
  turtle_pose_reader: Subscription<Pose>,
  messages_receiver: channel::Receiver<String>,
  terminal: ratatui::DefaultTerminal,
  display: ui::Display,
}

impl App {
  fn new() -> Self {
    let topic_qos = {
      QosPolicyBuilder::new()
        .durability(policy::Durability::Volatile)
        .liveliness(policy::Liveliness::Automatic {
          lease_duration: ros2::Duration::INFINITE,
        })
        .reliability(policy::Reliability::Reliable {
          max_blocking_time: ros2::Duration::from_millis(100),
        })
        .history(policy::History::KeepLast { depth: 1 })
        .build()
    };

    let ctx = Context::new().unwrap();

    let mut node = ctx
      .new_node(
        NodeName::new("/ros2_demo", "turtle_teleop").unwrap(),
        NodeOptions::new().enable_rosout(true),
      )
      .unwrap();

    let turtle_cmd_vel_topic = node
      .create_topic(
        &Name::new("/turtle1", "cmd_vel").unwrap(),
        MessageTypeName::new("geometry_msgs", "Twist"),
        &topic_qos,
      )
      .unwrap();

    // But here is how to read it also, if anyone is interested.
    // This should show what is the turtle command in case someone else is
    // also issuing commands, i.e. there are two turtle controllers running.
    let turtle_cmd_vel_reader = node
      .create_subscription::<Twist>(&turtle_cmd_vel_topic, None)
      .unwrap();

    let turtle_pose_topic = node
      .create_topic(
        &Name::new("/turtle1", "pose").unwrap(),
        MessageTypeName::new("turtlesim", "Pose"),
        &topic_qos,
      )
      .unwrap();

    let turtle_pose_reader = node
      .create_subscription::<Pose>(&turtle_pose_topic, None)
      .unwrap();

    let (messages_sender, messages_receiver) = channel::bounded(16);

    Self {
      requester: Requester::new(node, messages_sender, &topic_qos, &turtle_cmd_vel_topic),
      turtle_cmd_vel_reader,
      turtle_pose_reader,
      messages_receiver,
      terminal: ratatui::init(),
      display: ui::Display::default(),
    }
  }

  async fn run<'a>(&'a mut self, exec: &LocalExecutor<'a>) -> io::Result<()> {
    // event loop

    info!("Entering event_loop");

    // Example of how to write to "rosout" log
    rosout!(
      self.requester.node,
      ros2::LogLevel::Info,
      "initialized, entering event loop"
    );

    let events = ui::events().fuse();
    pin!(events);
    loop {
      let display = &self.display;
      self.terminal.draw(|frame| display.draw(frame))?;

      select! {
        Some(Ok(event)) = events.next() => {
          match event {
            ui::Event::StopEventLoop => {
              info!("Stopping main event loop");
              break;
            }
            ui::Event::TurtleCmdVel { twist } => {
              exec.spawn(self.requester.publish_turtle_cmd_vel(twist)).detach();
            }
            ui::Event::Reset => {
              exec.spawn(self.requester.reset()).detach();
            }
            ui::Event::SetPen(pen_request) => {
              exec.spawn(self.requester.set_pen(pen_request)).detach();
            }
            ui::Event::Spawn(name) => {
              exec.spawn(self.requester.spawn(name)).detach();
            }
            ui::Event::Kill(name) => {
              exec.spawn(self.requester.kill(name)).detach();
            }
            ui::Event::RotateAbsolute { heading } => {
              exec.spawn(self.requester.rotate_absolute(heading)).detach();
            }
            ui::Event::CancelRotateAbsolute => {
              self.requester.cancel_rotate_absolute()
            }
            ui::Event::ChooseTurtle { id } => {
              self.requester.set_controlled_turtle_id(id);
            }
          }
        }
        Ok(message) = self.messages_receiver.recv() => {
          self.display.add_message(message);
        }
        Ok((cmd_vel, _)) = self.turtle_cmd_vel_reader.async_take() => {
          self.display.set_cmd_vel(cmd_vel);
        }
        Ok((pose, _)) = self.turtle_pose_reader.async_take() => {
          self.display.set_pose(pose);
        }
      }
    }

    ratatui::restore();
    Ok(())
  }
}

struct Requester {
  node: Node,
  messages_sender: channel::Sender<String>,
  turtle_cmd_vel_writer: Publisher<Twist>,
  turtle_cmd_vel_writer2: Publisher<Twist>,
  reset_client: Client<AService<EmptyMessage, EmptyMessage>>,
  set_pen_client: Client<AService<PenRequest, ()>>,
  spawn_client: Client<SpawnService>,
  kill_client: Client<KillService>,
  rotate_action_client: ActionClient<RotateAbsoluteAction>,
  cancel_rotate: Cell<Option<oneshot::Sender<()>>>,
  turtle_id: Cell<i32>,
}

impl Requester {
  fn new(
    mut node: Node,
    messages_sender: channel::Sender<String>,
    topic_qos: &ros2::QosPolicies,
    turtle_cmd_vel_topic: &rustdds::Topic,
  ) -> Self {
    // The point here is to publish Twist for the turtle
    let turtle_cmd_vel_writer = node
      .create_publisher::<Twist>(turtle_cmd_vel_topic, None)
      .unwrap();

    // Prepare for controlling 2nd turtle
    let turtle2_cmd_vel_topic = node
      .create_topic(
        &Name::new("/turtle2", "cmd_vel").unwrap(),
        MessageTypeName::new("geometry_msgs", "Twist"),
        topic_qos,
      )
      .unwrap();
    let turtle_cmd_vel_writer2 = node
      .create_publisher::<Twist>(&turtle2_cmd_vel_topic, None)
      .unwrap();

    // Turtle has services, let's construct some clients.

    let service_qos = QosPolicyBuilder::new()
      .reliability(policy::Reliability::Reliable {
        max_blocking_time: ros2::Duration::from_millis(100),
      })
      .history(policy::History::KeepLast { depth: 1 })
      .build();

    // create_client cyclone version tested against ROS2 Galactic. Obviously with
    // CycloneDDS. Seems to work on the same host only.
    //
    // create_client enhanced version tested against
    // * ROS2 Foxy with eProsima DDS. Works to another host also.
    // * ROS2 Galactic with RTI Connext (rmw_connextdds, not rmw_connext_cpp)
    //   Environment variable RMW_CONNEXT_REQUEST_REPLY_MAPPING=extended Works to
    //   another host also.
    //
    // * create_client basic version is untested.

    let reset_client = node
      .create_client::<AService<EmptyMessage, EmptyMessage>>(
        ServiceMapping::Enhanced,
        &Name::new("/", "reset").unwrap(),
        &ServiceTypeName::new("std_srvs", "Empty"),
        service_qos.clone(),
        service_qos.clone(),
      )
      .unwrap();

    // another client

    // from https://docs.ros2.org/foxy/api/turtlesim/srv/SetPen.html
    let set_pen_client = node
      .create_client::<AService<PenRequest, ()>>(
        ServiceMapping::Enhanced,
        &Name::new("/turtle1", "set_pen").unwrap(),
        &ServiceTypeName::new("turtlesim", "SetPen"),
        service_qos.clone(),
        service_qos.clone(),
      )
      .unwrap();

    // third client
    let spawn_srv_type = ServiceTypeName::new("turtlesim", "Spawn");
    let spawn_client = node
      .create_client::<SpawnService>(
        ServiceMapping::Enhanced,
        &Name::new("/", "spawn").unwrap(),
        &spawn_srv_type,
        service_qos.clone(),
        service_qos.clone(),
      )
      .unwrap();

    // kill service client
    let kill_srv_type = ServiceTypeName::new("turtlesim", "Kill");
    let kill_client = node
      .create_client::<KillService>(
        ServiceMapping::Enhanced,
        &Name::new("/", "kill").unwrap(),
        &kill_srv_type,
        service_qos.clone(),
        service_qos.clone(),
      )
      .unwrap();

    // Try an Action
    //TODO: There should be an easier way to do this.
    let rotate_action_qos = action::ActionClientQosPolicies {
      goal_service: service_qos.clone(),
      result_service: service_qos.clone(),
      cancel_service: service_qos.clone(),
      feedback_subscription: service_qos.clone(),
      status_subscription: service_qos,
    };

    let rotate_action_client = node
      .create_action_client::<RotateAbsoluteAction>(
        ServiceMapping::Enhanced,
        &Name::new("/turtle1", "rotate_absolute").unwrap(),
        &ActionTypeName::new("turtlesim", "RotateAbsolute"),
        rotate_action_qos,
      )
      .unwrap();

    Self {
      node,
      messages_sender,
      turtle_cmd_vel_writer,
      turtle_cmd_vel_writer2,
      reset_client,
      set_pen_client,
      spawn_client,
      kill_client,
      rotate_action_client,
      turtle_id: Cell::new(1),
      cancel_rotate: Cell::default(),
    }
  }

  async fn publish_turtle_cmd_vel(&self, twist: Twist) {
    let writer = match self.turtle_id.get() {
      1 => &self.turtle_cmd_vel_writer,
      2 => &self.turtle_cmd_vel_writer2,
      _ => return,
    };
    match writer.async_publish(twist).await {
      Ok(()) => info!(
        "cmd_vel {twist:?} for turtle {id} has been published",
        id = self.turtle_id.get()
      ),
      Err(err) => error!("failed to write to turtle writer: {err}"),
    }
  }

  async fn reset(&self) {
    match self
      .reset_client
      .async_call_service(EmptyMessage::new())
      .await
    {
      Ok(EmptyMessage { .. }) => {
        rosout!(self.node, ros2::LogLevel::Info, "Requested turtlesim reset");
        let msg = "reset request sent";
        info!("{msg}");
        let _ = self.messages_sender.send(msg.to_owned()).await;
      }
      Err(err) => {
        error!(
          "failed to reset turtlesim: {err}",
          err = DisplayCallServiceError(err)
        );
      }
    }
  }

  async fn set_pen(&self, pen_request: PenRequest) {
    match self.set_pen_client.async_call_service(pen_request).await {
      Ok(()) => {
        let msg = format!("pen {pen_request:?} has been set");
        info!("{msg}");
        let _ = self.messages_sender.send(msg).await;
      }
      Err(err) => error!(
        "error setting pen: {err}",
        err = DisplayCallServiceError(err)
      ),
    }
  }

  async fn spawn(&self, name: String) {
    match self
      .spawn_client
      .async_call_service(SpawnRequest {
        x: 1.0,
        y: 1.0,
        theta: 0.0,
        name,
      })
      .await
    {
      Ok(SpawnResponse { name }) => info!("a turtle has been spawned with name {name:?}"),
      Err(err) => error!(
        "failed to spawn a turtle: {err}",
        err = DisplayCallServiceError(err)
      ),
    }
  }

  async fn kill(&self, name: String) {
    match self
      .kill_client
      .async_call_service(KillRequest { name: name.clone() })
      .await
    {
      Ok(EmptyMessage { .. }) => {
        let msg = format!("turtle {name:?} has been killed");
        info!("{msg}");
        let _ = self.messages_sender.send(msg).await;
      }
      Err(err) => error!(
        "failed to kill a turtle: {err}",
        err = DisplayCallServiceError(err)
      ),
    }
  }

  async fn rotate_absolute(&self, heading: f32) {
    match self
      .rotate_action_client
      .async_send_goal(RotateAbsoluteGoal { theta: heading })
      .await
    {
      Ok((goal_id, action::SendGoalResponse { accepted, .. })) => {
        if !accepted {
          error!("rotate_absolute goal has been rejected (goal_id={goal_id:?})");
          return;
        }

        let msg = format!("rotate_absolute goal has been accepted (goal_id={goal_id:?})");
        info!("{msg}");
        rosout!(self.node, ros2::LogLevel::Info, "{msg}");
        let _ = self.messages_sender.send(msg).await;

        let (cancel_sender, cancel_receiver) = oneshot::channel();
        self.cancel_rotate.replace(Some(cancel_sender));
        select! {
          Ok(()) = cancel_receiver => {
            match self.rotate_action_client.async_cancel_goal(goal_id, self.node.time_now().into()).await {
              Ok(action::CancelGoalResponse { return_code, .. }) => {
                let msg = format!("rotate_absolute cancellation returned code {return_code:?}");
                info!("{msg}");
                let _ = self.messages_sender.send(msg).await;
              }
              Err(err) => {
                error!(
                  "failed to cancel rotate_absolute goal {goal_id:?}: {err}",
                  err = DisplayCallServiceError(err)
                )
              }
            }
          }
          res = self.rotate_action_client.async_request_result(goal_id) => {
            match res {
              Ok((status, RotateAbsoluteResult { delta })) => {
                let msg = format!("rotate_absolute has terminated with status {status:?} and delta {delta}");
                info!("{msg}");
                let _ = self.messages_sender.send(msg).await;
              },
              Err(err) => {
                error!("failed to receive rotate_absolute result: {err}", err = DisplayCallServiceError(err));
              }
            }
          }
        }
      }
      Err(err) => {
        error!(
          "failed to send rotate_absolute action goal: {err}",
          err = DisplayCallServiceError(err)
        );
      }
    }
  }

  fn cancel_rotate_absolute(&self) {
    if let Some(cancel) = self.cancel_rotate.take() {
      let _res = cancel.send(());
    }
  }

  fn set_controlled_turtle_id(&self, id: i32) {
    self.turtle_id.replace(id);
  }
}

// This corresponds to ROS2 message type
// https://github.com/ros2/common_interfaces/blob/master/geometry_msgs/msg/Twist.msg
//
// The struct definition must have a layout corresponding to the
// ROS2 msg definition to get compatible serialization.
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Twist {
  pub linear: Vector3,
  pub angular: Vector3,
}

// https://docs.ros2.org/foxy/api/turtlesim/msg/Pose.html
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pose {
  pub x: f32,
  pub y: f32,
  pub theta: f32,
  pub linear_velocity: f32,
  pub angular_velocity: f32,
}

// This corresponds to ROS2 message type
// https://github.com/ros2/common_interfaces/blob/master/geometry_msgs/msg/Vector3.msg
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Vector3 {
  pub x: f64,
  pub y: f64,
  pub z: f64,
}

impl Vector3 {
  pub const ZERO: Vector3 = Vector3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
  };
}

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PenRequest {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub width: u8,
  pub off: u8,
}

impl Message for PenRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyMessage {
  // ROS2 Foxy with eProsima DDS crashes if the EmptyMessage is really empty,
  // so we put in a dummy byte.
  dummy: u8,
}
impl EmptyMessage {
  pub fn new() -> EmptyMessage {
    EmptyMessage { dummy: 1 }
  }
}

impl Default for EmptyMessage {
  fn default() -> Self {
    Self::new()
  }
}

impl Message for EmptyMessage {}

// from https://docs.ros2.org/foxy/api/turtlesim/srv/Spawn.html
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
  pub x: f32,
  pub y: f32,
  pub theta: f32,
  pub name: String,
}
impl Message for SpawnRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResponse {
  pub name: String,
}
impl Message for SpawnResponse {}

type SpawnService = AService<SpawnRequest, SpawnResponse>;

// from https://docs.ros2.org/foxy/api/turtlesim/srv/Spawn.html
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillRequest {
  pub name: String,
}
impl Message for KillRequest {}

type KillService = AService<KillRequest, EmptyMessage>;

// https://docs.ros.org/en/humble/Tutorials/Beginner-CLI-Tools/Understanding-ROS2-Actions/Understanding-ROS2-Actions.html
//
//
// Note: The action component types could be named anything.
// The field naming is also arbitrary.
// The important thing is that the types serialize to/from the same as the
// definition at the other end of the wire. In this case, a simple "f32" would
// do.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RotateAbsoluteGoal {
  theta: f32,
}
impl Message for RotateAbsoluteGoal {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RotateAbsoluteResult {
  delta: f32,
}
impl Message for RotateAbsoluteResult {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RotateAbsoluteFeedback {
  remaining: f32,
}
impl Message for RotateAbsoluteFeedback {}

type RotateAbsoluteAction =
  Action<RotateAbsoluteGoal, RotateAbsoluteResult, RotateAbsoluteFeedback>;

#[derive(Debug)]
struct DisplayCallServiceError<T>(CallServiceError<T>);

impl<T> std::fmt::Display for DisplayCallServiceError<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match &self.0 {
      CallServiceError::WriteError(err) => err.fmt(f),
      CallServiceError::ReadError(err) => err.fmt(f),
    }
  }
}

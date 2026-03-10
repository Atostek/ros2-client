use std::{collections::VecDeque, f32::consts::PI, io, iter::FromIterator};

use crossterm::event::{EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{Stream, TryStreamExt};
use ratatui::{
  layout::{Constraint, Layout},
  prelude::{Buffer, Rect},
  style::Stylize,
  symbols::border,
  text::{Line, Text},
  widgets::{Block, List, ListDirection, Paragraph, Widget},
  Frame,
};

use crate::{PenRequest, Pose, Twist, Vector3};

#[derive(Debug)]
pub enum Event {
  StopEventLoop,
  ChooseTurtle { id: i32 },
  TurtleCmdVel { twist: Twist },
  Reset,
  SetPen(PenRequest),
  Spawn(String),
  Kill(String),
  RotateAbsolute { heading: f32 },
  CancelRotateAbsolute,
}

// Define turtle movement commands as Twist values
const MOVE_FORWARD: Twist = Twist {
  linear: Vector3 {
    x: 2.0,
    ..Vector3::ZERO
  },
  angular: Vector3::ZERO,
};

const MOVE_BACKWARD: Twist = Twist {
  linear: Vector3 {
    x: -2.0,
    ..Vector3::ZERO
  },
  angular: Vector3::ZERO,
};

const ROTATE_LEFT: Twist = Twist {
  linear: Vector3::ZERO,
  angular: Vector3 {
    z: 2.0,
    ..Vector3::ZERO
  },
};

const ROTATE_RIGHT: Twist = Twist {
  linear: Vector3::ZERO,
  angular: Vector3 {
    z: -2.0,
    ..Vector3::ZERO
  },
};

const PEN_REQUESTS: [PenRequest; 5] = [
  PenRequest {
    r: 255,
    b: 0,
    g: 0,
    width: 3,
    off: 0,
  },
  PenRequest {
    r: 255,
    b: 0,
    g: 200,
    width: 5,
    off: 0,
  },
  PenRequest {
    r: 250,
    b: 250,
    g: 250,
    width: 2,
    off: 1,
  },
  PenRequest {
    r: 0,
    b: 0,
    g: 250,
    width: 1,
    off: 0,
  },
  PenRequest {
    r: 0,
    b: 0,
    g: 0,
    width: 1,
    off: 0,
  },
];

const MESSAGES_MAX_LEN: usize = 20;

#[derive(Default, Debug)]
pub struct Display {
  cmd_vel: Twist,
  pose: Pose,
  messages: VecDeque<String>,
}

impl Display {
  pub fn draw(&self, frame: &mut Frame) {
    frame.render_widget(self, frame.area())
  }

  pub fn set_cmd_vel(&mut self, twist: Twist) {
    self.cmd_vel = twist;
  }

  pub fn set_pose(&mut self, pose: Pose) {
    self.pose = pose;
  }

  pub fn add_message(&mut self, msg: String) {
    self.messages.push_front(msg);
    self.messages.truncate(MESSAGES_MAX_LEN);
  }
}

impl Widget for &Display {
  fn render(self, area: Rect, buf: &mut Buffer)
  where
    Self: Sized,
  {
    let main_block = Block::bordered()
      .title(Line::from(" Turle Teleop ".bold()).centered())
      .title_bottom(Line::from(vec![" Quit ".into(), "<q> / <Ctrl-C> ".blue().bold()]).centered())
      .border_set(border::THICK);

    let vlayout = Layout::vertical(vec![Constraint::Fill(1), Constraint::Fill(1)])
      .split(main_block.inner(area));

    let hlayout =
      Layout::horizontal(vec![Constraint::Fill(2), Constraint::Fill(1)]).split(vlayout[0]);

    Paragraph::new(vec![
      Line::from("cmd_vel: ".bold()),
      Line::from(vec![" - ".into(), "linear: ".bold()]),
      Line::from(vec![
        "   - ".into(),
        "x: ".bold(),
        format!("{}", self.cmd_vel.linear.x).into(),
      ]),
      Line::from(vec![
        "   - ".into(),
        "y: ".bold(),
        format!("{}", self.cmd_vel.linear.y).into(),
      ]),
      Line::from(vec![
        "   - ".into(),
        "z: ".bold(),
        format!("{}", self.cmd_vel.linear.z).into(),
      ]),
      Line::from(vec![" - ".into(), "angular: ".bold()]),
      Line::from(vec![
        "   - ".into(),
        "x: ".bold(),
        format!("{}", self.cmd_vel.angular.x).into(),
      ]),
      Line::from(vec![
        "   - ".into(),
        "y: ".bold(),
        format!("{}", self.cmd_vel.angular.y).into(),
      ]),
      Line::from(vec![
        "   - ".into(),
        "z: ".bold(),
        format!("{}", self.cmd_vel.angular.z).into(),
      ]),
      Line::default(),
      "pose: ".bold().into(),
      Line::from(vec![
        " - ".into(),
        "x: ".bold(),
        format!("{}", self.pose.x).into(),
      ]),
      Line::from(vec![
        " - ".into(),
        "y: ".bold(),
        format!("{}", self.pose.y).into(),
      ]),
      Line::from(vec![
        " - ".into(),
        "theta: ".bold(),
        format!("{}", self.pose.theta).into(),
      ]),
      Line::from(vec![
        " - ".into(),
        "linear velocity: ".bold(),
        format!("{}", self.pose.linear_velocity).into(),
      ]),
      Line::from(vec![
        " - ".into(),
        "angular velocity: ".bold(),
        format!("{}", self.pose.angular_velocity).into(),
      ]),
    ])
    .block(Block::bordered().title("Turtle 1"))
    .render(hlayout[0], buf);

    const INSTRUCTIONS: [(&str, &str); 15] = [
      ("a", "Spawn 1"),
      ("A", "Kill 1"),
      ("b", "Spawn 2"),
      ("B", "Kill 2"),
      ("1", "Control Turtle 1"),
      ("2", "Control Turtle 2"),
      ("d", "Rotate West "),
      ("g", "Rotate East"),
      ("f", "Cancel Rotate"),
      ("Up", "Move Forward"),
      ("Down", "Move Backward"),
      ("Left", "Move Left "),
      ("Right", "Move Right"),
      ("p", "Set pen"),
      ("r", "Reset"),
    ];

    Paragraph::new(
      INSTRUCTIONS
        .iter()
        .map(|(shortcut, descr)| [format!("<{shortcut}>: ").blue().bold(), (*descr).into()])
        .map(Line::from_iter)
        .collect::<Text>(),
    )
    .block(Block::bordered().title("Instructions"))
    .render(hlayout[1], buf);

    List::new(self.messages.iter().map(|s| s.as_str()))
      .direction(ListDirection::TopToBottom)
      .block(Block::bordered().title("Messages"))
      .render(vlayout[1], buf);

    main_block.render(area, buf);
  }
}

fn into_event(event: crossterm::event::Event, pen_index: &mut usize) -> Option<Event> {
  event.as_key_press_event().and_then(
    |KeyEvent {
       code, modifiers, ..
     }| match code {
      KeyCode::Char('q') => Some(Event::StopEventLoop),
      KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => Some(Event::StopEventLoop),
      KeyCode::Char('r') => Some(Event::Reset),
      KeyCode::Char('p') => {
        let pen = PEN_REQUESTS[*pen_index];
        *pen_index = (*pen_index + 1) % PEN_REQUESTS.len();
        Some(Event::SetPen(pen))
      }
      KeyCode::Char(c @ ('a' | 'b')) => {
        let turtle = if c == 'a' { "turtle1" } else { "turtle2" };
        Some(Event::Spawn(turtle.to_owned()))
      }
      KeyCode::Char(c @ ('A' | 'B')) => {
        let turtle = if c == 'A' { "turtle1" } else { "turtle2" };
        Some(Event::Kill(turtle.to_owned()))
      }
      KeyCode::Char('1') => Some(Event::ChooseTurtle { id: 1 }),
      KeyCode::Char('2') => Some(Event::ChooseTurtle { id: 2 }),
      KeyCode::Char('d') => Some(Event::RotateAbsolute { heading: PI }),
      KeyCode::Char('g') => Some(Event::RotateAbsolute { heading: 0. }),
      KeyCode::Char('f') => Some(Event::CancelRotateAbsolute),
      KeyCode::Up => Some(Event::TurtleCmdVel {
        twist: MOVE_FORWARD,
      }),
      KeyCode::Down => Some(Event::TurtleCmdVel {
        twist: MOVE_BACKWARD,
      }),
      KeyCode::Right => Some(Event::TurtleCmdVel {
        twist: ROTATE_RIGHT,
      }),
      KeyCode::Left => Some(Event::TurtleCmdVel { twist: ROTATE_LEFT }),
      _ => None,
    },
  )
}

pub fn events() -> impl Stream<Item = io::Result<Event>> {
  let mut pen_index = 0;
  EventStream::new().try_filter_map(move |event| {
    let event = into_event(event, &mut pen_index);
    async move { Ok(event) }
  })
}

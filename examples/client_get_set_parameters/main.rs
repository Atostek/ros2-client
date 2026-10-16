//! Call a remote node's `get_parameters` or `set_parameters` service.
//!
//! ```text
//! client_get_set_parameters <node> get <name>...
//! client_get_set_parameters <node> set <name>=<value>...
//! ```
//!
//! Values: `true`/`false` are Boolean, an `i64` literal is Integer, an `f64`
//! literal (`1.0`) is Double, `[a,b,...]` is an array of one element type, and
//! anything else is a String. Empty `[]` is an error. Byte arrays are not
//! accepted.

use std::{env, time::Duration};

use futures::TryFutureExt;
use ros2_client::{
  Context, Message, Name, Node, NodeName, NodeOptions, Parameter, ParameterValue, QosProfile,
  ServiceMapping, ServiceTypeName,
  qos::History,
  rcl_interfaces::{
    GetParametersRequest, GetParametersResponse, SetParametersRequest, SetParametersResponse,
  },
  ros2::WriteError,
  service::{CallServiceError, Client},
};
use smol::future::FutureExt;

fn main() {
  log4rs::init_file("log4rs.yaml", Default::default()).unwrap();

  let args: Vec<String> = env::args().skip(1).collect();
  let Some((target_node, command)) = args.split_first() else {
    usage();
    return;
  };
  let Some((verb, rest)) = command.split_first() else {
    usage();
    return;
  };

  let mut node = create_node();
  smol::spawn(node.spinner().unwrap().spin()).detach();
  let service_qos = create_qos();

  match verb.as_str() {
    "get" => {
      if rest.is_empty() {
        usage();
        return;
      }
      let client = node
        .create_client::<GetParametersRequest, GetParametersResponse>(
          ServiceMapping::Enhanced,
          &Name::new(target_node, "get_parameters").unwrap(),
          &ServiceTypeName::new("rcl_interfaces", "GetParameters"),
          service_qos.clone(),
          service_qos,
        )
        .unwrap();
      let request = GetParametersRequest {
        names: rest.to_vec(),
      };
      smol::block_on(async {
        match call_service(&node, &client, request).await {
          Ok(response) => {
            println!(
              "<<< response parameters: {:?}",
              response
                .values
                .iter()
                .cloned()
                .map(ParameterValue::from)
                .collect::<Vec<ParameterValue>>()
            );
          }
          Err(e) => println!("<<< response error {e:?}"),
        }
      });
    }
    "set" => {
      let assignments = match rest
        .iter()
        .map(|item| parse_assignment(item))
        .collect::<Result<Vec<_>, _>>()
      {
        Ok(items) if !items.is_empty() => items,
        Ok(_) => {
          usage();
          return;
        }
        Err(e) => {
          println!("{e}");
          usage();
          return;
        }
      };
      let client = node
        .create_client::<SetParametersRequest, SetParametersResponse>(
          ServiceMapping::Enhanced,
          &Name::new(target_node, "set_parameters").unwrap(),
          &ServiceTypeName::new("rcl_interfaces", "SetParameters"),
          service_qos.clone(),
          service_qos,
        )
        .unwrap();
      let request = SetParametersRequest {
        parameter: assignments
          .into_iter()
          .map(|(name, value)| Parameter { name, value }.into())
          .collect(),
      };
      smol::block_on(async {
        match call_service(&node, &client, request).await {
          Ok(response) => {
            for result in response.results {
              println!(
                "<<< successful={} reason={}",
                result.successful, result.reason
              );
            }
          }
          Err(e) => println!("<<< response error {e:?}"),
        }
      });
    }
    _ => usage(),
  }
}

fn usage() {
  println!(
    "Usage:\n  \
     client_get_set_parameters <node> get <name>...\n  \
     client_get_set_parameters <node> set <name>=<value>...\n\
     Values: true/false Boolean, i64 Integer, f64 Double (1.0), \
     [a,b,...] one-element-type array, otherwise String. Empty [] is an error."
  );
}

fn create_qos() -> QosProfile {
  QosProfile::publisher_default().history(History::KeepLast { depth: 1 })
}

fn create_node() -> Node {
  let context = Context::new().unwrap();
  context
    .new_node(
      NodeName::new("/rustdds", "get_set_parameters_client").unwrap(),
      NodeOptions::new().enable_rosout(true),
    )
    .unwrap()
}

async fn call_service<Req, Resp>(
  node: &Node,
  client: &Client<Req, Resp>,
  request: Req,
) -> Result<Resp, CallServiceError<()>>
where
  Req: Message + 'static,
  Resp: Message + 'static,
{
  println!(">>> Waiting for server to appear.");
  client.wait_for_service(node).await;
  println!(">>> Connected to server.");

  let req_id = client.async_send_request(request).await?;
  println!(">>> request sent {req_id:?}");
  client
    .async_receive_response(req_id)
    .map_err(CallServiceError::<()>::from)
    .or(async {
      smol::Timer::after(Duration::from_secs(10)).await;
      println!(">>> Response timeout!!");
      Err(WriteError::WouldBlock { data: () }.into())
    })
    .await
}

fn parse_assignment(text: &str) -> Result<(String, ParameterValue), String> {
  let Some((name, value)) = text.split_once('=') else {
    return Err(format!("expected name=value, got '{text}'"));
  };
  if name.is_empty() {
    return Err(format!("missing parameter name in '{text}'"));
  }
  Ok((name.to_string(), parse_value(value)?))
}

fn parse_value(text: &str) -> Result<ParameterValue, String> {
  if let Some(inner) = text
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
  {
    parse_array(inner)
  } else {
    Ok(parse_scalar(text))
  }
}

fn parse_array(inner: &str) -> Result<ParameterValue, String> {
  let inner = inner.trim();
  if inner.is_empty() {
    return Err("empty [] is not allowed".to_string());
  }
  let elements: Vec<ParameterValue> = inner
    .split(',')
    .map(|part| parse_scalar(part.trim()))
    .collect();
  let kind = std::mem::discriminant(&elements[0]);
  if elements
    .iter()
    .any(|element| std::mem::discriminant(element) != kind)
  {
    return Err(format!("array elements must share one type: {inner}"));
  }
  Ok(match &elements[0] {
    ParameterValue::Boolean(_) => ParameterValue::BooleanArray(
      elements
        .into_iter()
        .map(|value| match value {
          ParameterValue::Boolean(b) => b,
          _ => unreachable!(),
        })
        .collect(),
    ),
    ParameterValue::Integer(_) => ParameterValue::IntegerArray(
      elements
        .into_iter()
        .map(|value| match value {
          ParameterValue::Integer(i) => i,
          _ => unreachable!(),
        })
        .collect(),
    ),
    ParameterValue::Double(_) => ParameterValue::DoubleArray(
      elements
        .into_iter()
        .map(|value| match value {
          ParameterValue::Double(d) => d,
          _ => unreachable!(),
        })
        .collect(),
    ),
    ParameterValue::String(_) => ParameterValue::StringArray(
      elements
        .into_iter()
        .map(|value| match value {
          ParameterValue::String(s) => s,
          _ => unreachable!(),
        })
        .collect(),
    ),
    other => return Err(format!("unsupported array element {other:?}")),
  })
}

fn parse_scalar(text: &str) -> ParameterValue {
  match text {
    "true" => ParameterValue::Boolean(true),
    "false" => ParameterValue::Boolean(false),
    _ => {
      if let Ok(integer) = text.parse::<i64>() {
        ParameterValue::Integer(integer)
      } else if (text.contains('.') || text.contains('e') || text.contains('E'))
        && let Ok(double) = text.parse::<f64>()
      {
        ParameterValue::Double(double)
      } else {
        ParameterValue::String(text.to_string())
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_scalars_and_arrays() {
    assert_eq!(parse_value("true").unwrap(), ParameterValue::Boolean(true));
    assert_eq!(
      parse_value("false").unwrap(),
      ParameterValue::Boolean(false)
    );
    assert_eq!(parse_value("10").unwrap(), ParameterValue::Integer(10));
    assert_eq!(parse_value("-1").unwrap(), ParameterValue::Integer(-1));
    assert_eq!(parse_value("1.0").unwrap(), ParameterValue::Double(1.0));
    assert_eq!(
      parse_value("hello").unwrap(),
      ParameterValue::String("hello".into())
    );
    assert_eq!(
      parse_value("[a,b]").unwrap(),
      ParameterValue::StringArray(vec!["a".into(), "b".into()])
    );
    assert_eq!(
      parse_value("[1,2]").unwrap(),
      ParameterValue::IntegerArray(vec![1, 2])
    );
    assert!(parse_value("[]").is_err());
    assert!(parse_value("[1,1.0]").is_err());
    assert!(parse_assignment("rate_hz=10").is_ok());
    assert!(parse_assignment("novalue").is_err());
  }
}

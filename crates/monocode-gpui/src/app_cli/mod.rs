//! `monocode-gpui app`: the command an agent session runs to call MonoCode,
//! whether or not its provider takes MCP servers. One run is one call on
//! the window's control socket, as the session the environment names (the
//! relay's credential, `mcp_command::session`), on its own connection,
//! before any GUI starts. ACTION is a control operation, or one of the old
//! dotted actions (`legacy`); the output is one JSON line, and the exit code
//! is 1 when it is not `ok`.
use crate::mcp_command::session;
use monocode_control::{
    Client, ClientInfo, Command, ControlError, REQUEST_ID, frame, hints_of, session_caller_name,
};
use monocode_core::app_mcp::{ENV_ROLE, ROLE_MONO};
use serde_json::{Map, Value, json};
use std::io::Read;

mod help;
pub(crate) mod legacy;
#[cfg(test)]
mod tests;

/// The most input `--input` reads, as before.
const MAX_INPUT: usize = 1_048_576;

/// What a run outside one of MonoCode's sessions is told.
pub(crate) const OUTSIDE: &str =
    "MonoCode app access is unavailable outside a MonoCode agent session";

/// What one run prints, and its exit code.
#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) output: Value,
    pub(crate) code: i32,
}

/// The call one run's arguments ask for.
#[derive(Debug, PartialEq)]
struct Parsed {
    action: String,
    input: Value,
    request_id: Option<String>,
}

/// Runs `monocode-gpui app ARGS…`: prints its line and returns the exit
/// code.
pub(crate) fn run(args: &[String]) -> i32 {
    let outcome = execute(
        args,
        |key| std::env::var(key).ok(),
        &mut std::io::stdin().lock(),
    );
    println!("{}", outcome.output);
    outcome.code
}

/// One run with arguments `args`, environment `var` and standard input
/// `stdin`: what it prints, and its exit code.
pub(crate) fn execute(
    args: &[String],
    var: impl Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Outcome {
    // A hint for the help and the old input grammar; it authorizes nothing.
    let mono = var(ENV_ROLE).as_deref() == Some(ROLE_MONO);
    if args
        .first()
        .is_none_or(|arg| matches!(arg.as_str(), "help" | "--help" | "-h"))
    {
        return Outcome {
            output: json!({ "help": help::text(mono) }),
            code: 0,
        };
    }
    let parsed = match parse(args, stdin) {
        Ok(parsed) => parsed,
        Err(error) => return client_failure(&error),
    };
    let relay = match session::from_env(var) {
        None => return client_failure(OUTSIDE),
        Some(Err(error)) => return failure(&error, None),
        Some(Ok(relay)) => relay,
    };
    if !mono && let Err(error) = legacy::operator_grammar(&parsed.action, &parsed.input) {
        return client_failure(&error);
    }
    let (command, input_id) = match command(&parsed.action, parsed.input) {
        Ok(call) => call,
        Err(error) => return client_failure(&error),
    };
    // Every run has an ID, and a call that changes something always sends
    // it, so running it again with `--request-id` cannot act twice.
    let request_id = parsed
        .request_id
        .or(input_id)
        .unwrap_or_else(monocode_core::mono::new_id);
    let sent = (!command.hints().read_only).then(|| sent_request_id(&request_id));
    let info = ClientInfo {
        name: session_caller_name(relay.credential.id),
        version: env!("CARGO_PKG_VERSION").into(),
    };
    let client = Client::new(relay.endpoint, info).with_session(relay.credential);
    let result = if parsed.action == "sessions.list" {
        every_session(&client, command)
    } else {
        client.send(sent, command)
    };
    match result {
        Ok(result) => Outcome {
            output: json!({ "ok": true, "result": result, "requestId": request_id }),
            code: 0,
        },
        Err(error) => failure(&error, Some(&request_id)),
    }
}

/// `ACTION` and its input and request ID, read as the old CLI read them.
fn parse(args: &[String], stdin: &mut dyn Read) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        action: args[0].clone(),
        input: json!({}),
        request_id: None,
    };
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => parsed.input = json(rest.next().ok_or("--json needs JSON")?)?,
            "--input" => {
                let path = rest.next().ok_or("--input needs a path")?;
                let mut text = String::new();
                if path == "-" {
                    Read::take(&mut *stdin, MAX_INPUT as u64 + 1)
                        .read_to_string(&mut text)
                        .map_err(|e| e.to_string())?;
                } else {
                    text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                }
                if text.len() > MAX_INPUT {
                    return Err("Input too large".into());
                }
                parsed.input = json(&text)?;
            }
            "--request-id" => {
                let id = rest.next().ok_or("--request-id needs an ID")?;
                parsed.request_id = Some(id.clone());
            }
            text if text.starts_with('{') => parsed.input = json(text)?,
            _ => return Err(format!("Unknown app argument: {arg}")),
        }
    }
    Ok(parsed)
}

fn json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// The command `action` with `input` is, and the `requestId` an
/// operation's input names. An operation takes its input as its MCP tool
/// does; an old action is translated (`legacy`).
fn command(action: &str, input: Value) -> Result<(Command, Option<String>), String> {
    let (wire, request_id) = match hints_of(action) {
        Some(hints) => {
            let Value::Object(mut input) = input else {
                return Err("Input must be an object".into());
            };
            let request_id = if hints.read_only {
                None
            } else {
                request_id(&mut input)?
            };
            (json!({ "operation": action, "input": input }), request_id)
        }
        None => (legacy::translate(action, &input)?, None),
    };
    let command =
        serde_json::from_value(wire).map_err(|e| format!("Invalid arguments for {action}: {e}"))?;
    Ok((command, request_id))
}

/// Takes `requestId` out of an operation's input.
fn request_id(input: &mut Map<String, Value>) -> Result<Option<String>, String> {
    match input.remove(REQUEST_ID) {
        None => Ok(None),
        Some(Value::String(id)) => Ok(Some(id)),
        Some(_) => Err("requestId must be text".into()),
    }
}

/// The request ID a call goes out with for the ID the person gave: the old
/// CLI took any, the control pattern fewer (`frame::valid_request_id`). One
/// outside the pattern goes as `cli-` and 32 hex digits of its SHA-256, so
/// a retry with the same ID still acts once.
fn sent_request_id(given: &str) -> String {
    if frame::valid_request_id(given) {
        return given.to_owned();
    }
    let hash = monocode_core::mono::files::content_hash(given);
    format!("cli-{}", &hash[..32])
}

/// The old `sessions.list`, which returned every session in one list:
/// each page of `command`, merged, in the shape of the last page.
fn every_session(client: &Client, command: Command) -> Result<Value, ControlError> {
    let Command::SessionList(mut input) = command else {
        return client.send(None, command);
    };
    let mut sessions = Vec::new();
    loop {
        let mut page = client.send(None, Command::SessionList(input.clone()))?;
        if let Some(Value::Array(listed)) = page.get_mut("sessions").map(Value::take) {
            sessions.extend(listed);
        }
        let next = page
            .as_object_mut()
            .and_then(|page| page.remove("nextCursor"))
            .and_then(|next| next.as_str().map(str::to_owned));
        match next {
            Some(next) if input.cursor.as_ref() != Some(&next) => input.cursor = Some(next),
            _ => {
                page["sessions"] = Value::Array(sessions);
                return Ok(page);
            }
        }
    }
}

/// A failure before anything was sent, in the old CLI's shape: there is no
/// request ID to retry with.
fn client_failure(error: &str) -> Outcome {
    Outcome {
        output: json!({ "ok": false, "error": error }),
        code: 1,
    }
}

/// A typed failure, with the request ID the call went out with, if it did.
fn failure(error: &ControlError, request_id: Option<&str>) -> Outcome {
    let mut output = json!({
        "ok": false,
        "error": error.message,
        "code": error.code,
        "retryable": error.retryable,
    });
    if let Some(id) = request_id {
        output["requestId"] = json!(id);
    }
    Outcome { output, code: 1 }
}

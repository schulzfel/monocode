//! The local-socket transport and the blocking client against a fake app.
//! Real endpoints live under the checkout's `target/c/`, inside the
//! checkout; on Windows the folder only names the pipe.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Client, ControlError, Endpoint, ErrorCode, Grant, PROTOCOL_VERSION, Scope,
    frame::{
        self, Call, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply,
        ServerFrame, Welcome,
    },
    ops::project::{ProjectList, ProjectListInput, ProjectListOutput},
    pipe_name, socket_path,
    transport::{Connection, Listener},
};
use serde_json::json;
use std::{io::BufReader, path::Path, thread::JoinHandle};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn welcome(version: u32) -> ServerFrame {
        ServerFrame::Welcome(Welcome {
            version,
            app_version: "0.16.0".into(),
            grant: Grant {
                scopes: vec![Scope::Read],
            },
            push: false,
        })
    }

    pub(super) fn projects() -> ProjectListOutput {
        ProjectListOutput {
            projects: Vec::new(),
            total: 0,
            next_cursor: None,
        }
    }

    /// Serves one connection: answers Hello with `greeting`, then every call
    /// with `answer`, until the client hangs up or `calls` calls were served.
    /// Returns the frames it received.
    pub(super) fn serve(
        listener: Listener,
        greeting: ServerFrame,
        calls: usize,
        answer: fn(&Call) -> Outcome,
    ) -> JoinHandle<(Listener, Vec<ClientFrame>)> {
        std::thread::spawn(move || {
            let connection = listener.accept().unwrap();
            let mut reader = BufReader::new(&connection);
            let mut seen = Vec::new();
            let hello: ClientFrame = frame::read_frame(&mut reader, MAX_REQUEST_BYTES)
                .unwrap()
                .unwrap();
            seen.push(hello);
            let refused = matches!(greeting, ServerFrame::Error(_));
            frame::write_frame(&mut &connection, &greeting, MAX_RESPONSE_BYTES).unwrap();
            if refused {
                return (listener, seen);
            }
            while seen.len() <= calls {
                // A hang-up may read as an error on Windows pipes.
                let Ok(Some(next)) =
                    frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
                else {
                    break;
                };
                if let ClientFrame::Call(call) = &next {
                    let reply = ServerFrame::Reply(Reply {
                        id: call.id,
                        result: answer(call),
                    });
                    frame::write_frame(&mut &connection, &reply, MAX_RESPONSE_BYTES).unwrap();
                }
                seen.push(next);
            }
            drop(reader);
            drop(connection);
            (listener, seen)
        })
    }

    pub(super) fn answer_projects(_: &Call) -> Outcome {
        Outcome::Ok(serde_json::to_value(projects()).unwrap())
    }
}
use common::{info, scratch};
use support::{answer_projects, projects, serve, welcome};

/// Who the tests call as.
const CLIENT: &str = "transport test";

#[test]
fn a_typed_call_reaches_the_app_and_decodes_its_output() {
    let endpoint = Endpoint::for_data_dir(&scratch("t1")).unwrap();
    let server = serve(
        Listener::bind(&endpoint).unwrap(),
        welcome(PROTOCOL_VERSION),
        1,
        answer_projects,
    );
    let client = Client::new(endpoint, info(CLIENT));
    assert!(
        client.welcome().is_none(),
        "nothing connects before the first call"
    );
    let output = client
        .call::<ProjectList>(ProjectListInput {
            cursor: None,
            limit: Some(3),
        })
        .unwrap();
    assert_eq!(output, projects());
    assert_eq!(client.welcome().unwrap().grant.scopes, [Scope::Read]);
    client.disconnect();
    let (_, seen) = server.join().unwrap();
    let ClientFrame::Hello(hello) = &seen[0] else {
        unreachable!("the first frame is hello: {seen:?}")
    };
    assert_eq!(hello.version, PROTOCOL_VERSION);
    assert_eq!(hello.client, info(CLIENT));
    let ClientFrame::Call(call) = &seen[1] else {
        unreachable!("the second frame is the call: {seen:?}")
    };
    assert_eq!(call.command.name(), "project_list");
    assert_eq!(call.request_id, None);
}

#[test]
fn an_app_error_is_returned_typed() {
    let endpoint = Endpoint::for_data_dir(&scratch("t2")).unwrap();
    let server = serve(
        Listener::bind(&endpoint).unwrap(),
        welcome(PROTOCOL_VERSION),
        1,
        |_| Outcome::Error(ControlError::new(ErrorCode::ScopeDenied, "not granted")),
    );
    let client = Client::new(endpoint, info(CLIENT));
    let error = client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ScopeDenied);
    client.disconnect();
    server.join().unwrap();
}

#[test]
fn a_refused_hello_is_returned_typed() {
    let endpoint = Endpoint::for_data_dir(&scratch("t3")).unwrap();
    let server = serve(
        Listener::bind(&endpoint).unwrap(),
        ServerFrame::Error(ControlError::control_disabled()),
        0,
        answer_projects,
    );
    let client = Client::new(endpoint, info(CLIENT));
    let error = client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlDisabled);
    server.join().unwrap();
}

#[test]
fn another_protocol_major_version_is_refused() {
    let endpoint = Endpoint::for_data_dir(&scratch("t4")).unwrap();
    let server = serve(
        Listener::bind(&endpoint).unwrap(),
        welcome(PROTOCOL_VERSION + 1),
        0,
        answer_projects,
    );
    let client = Client::new(endpoint, info(CLIENT));
    let error = client.connect().unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("protocol 2"), "{}", error.message);
    drop(client);
    server.join().unwrap();
}

#[test]
fn nothing_listening_is_app_not_running() {
    let endpoint = Endpoint::for_data_dir(&scratch("t5")).unwrap();
    let client = Client::new(endpoint, info(CLIENT));
    let error = client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::AppNotRunning);
    assert!(error.retryable);
}

#[test]
fn a_bad_request_id_never_leaves_the_client() {
    let endpoint = Endpoint::for_data_dir(&scratch("t6")).unwrap();
    let client = Client::new(endpoint, info(CLIENT));
    let error = client
        .send(
            Some("not valid!".into()),
            monocode_control::Command::ProjectList(ProjectListInput::default()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

/// Unix only: on Windows the relay's open handle keeps the old pipe name
/// alive, so the second app cannot listen while the client is connected.
#[cfg(unix)]
#[test]
fn a_read_reconnects_once_after_the_app_restarts() {
    let endpoint = Endpoint::for_data_dir(&scratch("t7")).unwrap();
    let first = serve(
        Listener::bind(&endpoint).unwrap(),
        welcome(PROTOCOL_VERSION),
        1,
        answer_projects,
    );
    let client = Client::new(endpoint.clone(), info(CLIENT));
    client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap();
    // The first app served its one call and hung up; a new one takes over.
    let (listener, _) = first.join().unwrap();
    drop(listener);
    let second = serve(
        Listener::bind(&endpoint).unwrap(),
        welcome(PROTOCOL_VERSION),
        1,
        answer_projects,
    );
    let output = client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap();
    assert_eq!(output, projects());
    client.disconnect();
    second.join().unwrap();
}

#[test]
fn pipe_names_derive_from_the_folder_case_insensitively() {
    let a = pipe_name(Path::new(
        r"C:\Users\Ada\AppData\Local\Monocode Development",
    ));
    let b = pipe_name(Path::new(
        r"c:\users\ada\appdata\local\monocode development\",
    ));
    let c = pipe_name(Path::new(
        r"C:\Users\Ada\AppData\Roaming\com.monocode.desktop",
    ));
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a.starts_with("monocode-control-"));
    assert_eq!(a.len(), "monocode-control-".len() + 16);
    assert!(a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
}

#[test]
fn a_socket_path_over_the_platform_limit_is_refused_clearly() {
    let long = Path::new("/").join("d".repeat(200));
    let error = socket_path(&long).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("MONOCODE_NATIVE_DATA_DIR"));
    assert!(socket_path(Path::new("/short")).is_ok());
}

#[test]
fn connection_reads_and_writes_frames_through_shared_references() {
    let endpoint = Endpoint::for_data_dir(&scratch("t8")).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    let echo = std::thread::spawn(move || {
        let connection = listener.accept().unwrap();
        let mut reader = BufReader::new(&connection);
        let line = frame::read_line(&mut reader, 64).unwrap().unwrap();
        frame::write_frame(
            &mut &connection,
            &json!({"echo": String::from_utf8(line).unwrap()}),
            64,
        )
        .unwrap();
    });
    let connection = Connection::connect(&endpoint).unwrap();
    frame::write_frame(&mut &connection, &"ping", 64).unwrap();
    let mut reader = BufReader::new(&connection);
    let back: serde_json::Value = frame::read_frame(&mut reader, 64).unwrap().unwrap();
    assert_eq!(back, json!({"echo": "\"ping\""}));
    echo.join().unwrap();
}

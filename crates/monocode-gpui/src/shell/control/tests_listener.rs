//! The control socket listener on its own, and the module's file size limit.
use super::tests::scratch;
use super::*;
use monocode_control::{
    ClientInfo, PROTOCOL_VERSION, Scope,
    frame::{self, ClientFrame, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame},
    ops::project::ProjectListInput,
    transport::Connection,
};
use std::{
    io::BufReader,
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
};

/// The listener on its own: hello and calls arrive as requests and the
/// answers go back over the real socket.
#[test]
fn the_listener_relays_hello_and_calls_over_the_socket() {
    let endpoint = Endpoint::for_data_dir(&scratch("g2")).unwrap();
    let (server, requests) = listener::Server::start(
        Listener::bind(&endpoint).unwrap(),
        &endpoint,
        "9.9.9".into(),
    )
    .unwrap();
    let client_endpoint = endpoint.clone();
    let client = std::thread::spawn(move || {
        let client = monocode_control::Client::new(
            client_endpoint,
            ClientInfo {
                name: "Listener test".into(),
                version: "1".into(),
            },
        );
        let output =
            client.call::<monocode_control::ops::project::ProjectList>(ProjectListInput::default());
        (client.welcome(), output)
    });
    let Ok(Request::Hello {
        client: info,
        reply,
        ..
    }) = requests.recv_blocking()
    else {
        unreachable!("hello comes first")
    };
    assert_eq!(info.name, "Listener test");
    let grant = Grant {
        scopes: vec![Scope::Read],
    };
    reply.send(Ok(grant.clone())).unwrap();
    let Ok(Request::Call {
        caller: name,
        command,
        reply,
        ..
    }) = requests.recv_blocking()
    else {
        unreachable!("then the call")
    };
    assert_eq!(
        (name, command.name()),
        (CallerKind::External("Listener test".into()), "project_list")
    );
    reply
        .send(Ok(serde_json::json!({"projects": [], "total": 0})))
        .unwrap();
    let (welcome, output) = client.join().unwrap();
    let welcome = welcome.unwrap();
    assert_eq!(
        (welcome.app_version.as_str(), welcome.grant),
        ("9.9.9", grant)
    );
    assert_eq!(output.unwrap().total, 0);
    drop(server);
    assert!(
        Connection::connect(&endpoint).is_err(),
        "dropping the server stops it"
    );
}

fn raw_exchange(endpoint: &Endpoint, frames: &[serde_json::Value]) -> Vec<ServerFrame> {
    let connection = Connection::connect(endpoint).unwrap();
    for frame in frames {
        frame::write_frame(&mut &connection, frame, MAX_REQUEST_BYTES).unwrap();
    }
    let mut reader = BufReader::new(&connection);
    let mut seen = Vec::new();
    while let Ok(Some(frame)) = frame::read_frame(&mut reader, MAX_RESPONSE_BYTES) {
        seen.push(frame);
    }
    seen
}

#[test]
fn an_old_relay_or_a_call_before_hello_is_refused_typed() {
    let endpoint = Endpoint::for_data_dir(&scratch("g3")).unwrap();
    let (_server, _requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let old = serde_json::to_value(ClientFrame::Hello(Hello {
        version: PROTOCOL_VERSION + 1,
        client: ClientInfo {
            name: "Future relay".into(),
            version: "1".into(),
        },
        session: None,
    }))
    .unwrap();
    let seen = raw_exchange(&endpoint, &[old]);
    let [ServerFrame::Error(error)] = seen.as_slice() else {
        unreachable!("one error frame, then the app hangs up: {seen:?}")
    };
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("protocol 2"));
    let early = serde_json::json!({"type": "call", "id": 1, "command": {"operation": "app_info", "input": {}}});
    let seen = raw_exchange(&endpoint, &[early]);
    assert!(matches!(seen.as_slice(), [ServerFrame::Error(e)] if e.message == "Say hello first"));
}

/// Plays the shell for a listener: grants the hello, then hands each call
/// to the test, with a deadline so a listener that serialized calls fails
/// instead of hanging.
fn next_call(
    requests: &async_channel::Receiver<Request>,
) -> (String, SyncSender<Result<Value, ControlError>>) {
    let requests = requests.clone();
    let (found, call) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(request) = requests.recv_blocking() {
            match request {
                Request::Hello { reply, .. } => {
                    let _ = reply.send(Ok(Grant {
                        scopes: vec![Scope::Read, Scope::Dispatch],
                    }));
                }
                Request::Call { command, reply, .. } => {
                    let _ = found.send((command.name().to_owned(), reply));
                    return;
                }
                Request::Subscribe { .. }
                | Request::Unsubscribe { .. }
                | Request::Closed { .. } => {
                    unreachable!("these tests only call")
                }
            }
        }
    });
    call.recv_timeout(std::time::Duration::from_secs(20))
        .expect("the listener passes the call on while another is running")
}

fn shared_client(endpoint: &Endpoint) -> std::sync::Arc<monocode_control::Client> {
    std::sync::Arc::new(monocode_control::Client::new(
        endpoint.clone(),
        ClientInfo {
            name: "Concurrency test".into(),
            version: "1".into(),
        },
    ))
}

fn wait_command() -> monocode_control::Command {
    monocode_control::Command::SessionWait(monocode_control::ops::dispatch::SessionWaitInput {
        session_id: 7,
        timeout_ms: Some(60_000),
    })
}

/// A pending `session_wait` does not delay a `project_list` sent on the same
/// connection after it.
#[test]
fn a_pending_session_wait_does_not_delay_a_concurrent_project_list() {
    let endpoint = Endpoint::for_data_dir(&scratch("g4")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let client = shared_client(&endpoint);
    let waiting = client.clone();
    let wait = std::thread::spawn(move || waiting.send(None, wait_command()));
    let (name, wait_reply) = next_call(&requests);
    assert_eq!(name, "session_wait");
    let listing = client.clone();
    let list = std::thread::spawn(move || {
        listing.send(
            None,
            monocode_control::Command::ProjectList(ProjectListInput::default()),
        )
    });
    let (name, list_reply) = next_call(&requests);
    assert_eq!(name, "project_list", "sent while session_wait is pending");
    list_reply
        .send(Ok(serde_json::json!({"projects": [], "total": 0})))
        .unwrap();
    assert_eq!(list.join().unwrap().unwrap()["total"], 0);
    assert!(!wait.is_finished(), "the wait is still pending");
    wait_reply
        .send(Ok(serde_json::json!({"sessionId": 7})))
        .unwrap();
    assert_eq!(wait.join().unwrap().unwrap()["sessionId"], 7);
}

/// Past the per-connection cap a call is refused at once, retryably, and
/// the connection keeps working.
#[test]
fn calls_over_the_per_connection_cap_are_refused_retryably() {
    let endpoint = Endpoint::for_data_dir(&scratch("g5")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let client = shared_client(&endpoint);
    let mut waits = Vec::new();
    let mut replies = Vec::new();
    for _ in 0..listener::MAX_CALLS {
        let waiting = client.clone();
        waits.push(std::thread::spawn(move || {
            waiting.send(None, wait_command())
        }));
        replies.push(next_call(&requests).1);
    }
    let refused = client.send(None, wait_command()).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Internal);
    assert!(refused.retryable, "{refused:?}");
    assert!(refused.message.contains("8 calls"), "{}", refused.message);
    for reply in replies {
        reply.send(Ok(serde_json::json!({"sessionId": 7}))).unwrap();
    }
    for wait in waits {
        assert!(wait.join().unwrap().is_ok());
    }
    let again = client.clone();
    let list = std::thread::spawn(move || {
        again.send(
            None,
            monocode_control::Command::ProjectList(ProjectListInput::default()),
        )
    });
    next_call(&requests)
        .1
        .send(Ok(serde_json::json!({"total": 0})))
        .unwrap();
    assert!(
        list.join().unwrap().is_ok(),
        "a slot frees once a call ends"
    );
}

#[test]
fn no_shell_file_for_external_control_is_over_500_lines() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shell/control"),
        &mut files,
    );
    assert!(files.len() >= 8, "the walk found the module: {files:?}");
    let long: Vec<_> = files
        .iter()
        .map(|p| (p, std::fs::read_to_string(p).unwrap().lines().count()))
        .filter(|(_, lines)| *lines > 500)
        .collect();
    assert!(long.is_empty(), "over 500 lines: {long:?}");
}

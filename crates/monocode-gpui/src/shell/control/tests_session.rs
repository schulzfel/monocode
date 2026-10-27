//! Agent sessions MonoCode runs reach the control socket whatever the
//! external switch says; external clients keep the opt-in model. Real
//! sockets under `target/c/`, with the test passing the listener's requests
//! to the shell.
use super::tests::{approve, call, hello, scratch, sessions, setup};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    AccessMode, ClientInfo, Command, PROTOCOL_VERSION, SessionCredential,
    frame::{self, ClientFrame, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame},
    ops::{
        configure::SessionConfigureInput,
        dispatch::{SessionStartInput, SessionWaitInput},
        organize::{SessionArchiveInput, SessionRenameInput},
        project::ProjectListInput,
        session::{SessionInput, SessionListInput},
    },
    session_caller_name,
    transport::Connection,
};
use std::{io::BufReader, time::Duration};

fn projects() -> Command {
    Command::ProjectList(ProjectListInput::default())
}

fn app_info() -> Command {
    Command::AppInfo(Default::default())
}

/// Says `hello` on a fresh connection to `endpoint`, hands the listener's
/// request to the shell, and returns what the app answered.
fn say_hello(
    endpoint: &Endpoint,
    hello: &Hello,
    requests: &async_channel::Receiver<Request>,
    window: WindowHandle<Shell>,
    cx: &mut TestAppContext,
) -> ServerFrame {
    let connection = Connection::connect(endpoint).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let frame = ClientFrame::Hello(hello.clone());
    frame::write_frame(&mut &connection, &frame, MAX_REQUEST_BYTES).unwrap();
    let request = requests.recv_blocking().unwrap();
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(request, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    let mut reader = BufReader::new(&connection);
    frame::read_frame(&mut reader, MAX_RESPONSE_BYTES)
        .unwrap()
        .expect("the app answers hello")
}

fn external(name: &str) -> Hello {
    Hello {
        version: PROTOCOL_VERSION,
        client: ClientInfo {
            name: name.into(),
            version: "1".into(),
        },
        session: None,
    }
}

#[gpui::test]
fn the_app_listens_while_external_control_is_off_and_refuses_external_hellos(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    let endpoint = Endpoint::for_data_dir(&scratch("s1")).unwrap();
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.control.policy.enabled, "control is off by default");
            shell.control.endpoint = Some(Ok(endpoint.clone()));
            shell.apply_control(cx);
            assert!(
                shell.control.server.is_some(),
                "the app listens for its own sessions while external control is off"
            );
        })
        .unwrap();
    assert!(Connection::connect(&endpoint).is_ok(), "the socket answers");
    let refused = hello(window, "Claude Code", cx).try_recv().unwrap();
    assert_eq!(refused.unwrap_err().code, ErrorCode::ControlDisabled);

    // The same over a real connection, as an external relay sees it.
    let other = Endpoint::for_data_dir(&scratch("s2")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&other).unwrap(), &other, "1".into()).unwrap();
    let answer = say_hello(&other, &external("Claude Code"), &requests, window, cx);
    let ServerFrame::Error(error) = answer else {
        unreachable!("refused, not {answer:?}")
    };
    assert_eq!(error.code, ErrorCode::ControlDisabled, "{}", error.message);
}

/// Session `id`'s credential in this run, as its relay gets it.
fn credential(id: i64) -> SessionCredential {
    SessionCredential {
        id,
        token: monocode_core::app_mcp::token(id),
    }
}

fn session_hello(
    window: WindowHandle<Shell>,
    credential: SessionCredential,
    cx: &mut TestAppContext,
) -> Result<Grant, ControlError> {
    let (reply, grant) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let hello = Request::Hello {
                client: external("Claude Code").client,
                session: Some(credential),
                reply,
            };
            shell.handle_control(hello, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    grant
        .try_recv()
        .expect("a session's hello is answered at once")
}

#[gpui::test]
fn a_session_with_its_token_gets_every_scope_without_a_prompt_while_control_is_off(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let grant = session_hello(window, credential(b), cx).unwrap();
    assert_eq!(grant.scopes, Scope::ALL.to_vec(), "every scope");
    assert!(cx.pending_prompt().is_none(), "nobody is asked");
    let caller = session_caller_name(b);
    let info = call(window, &caller, None, app_info(), cx).unwrap();
    assert_eq!(info["scopes"].as_array().unwrap().len(), Scope::ALL.len());
    assert_eq!(info["controlEnabled"], false, "external control stays off");
    let listed = call(window, &caller, None, projects(), cx).unwrap();
    assert!(listed["total"].as_u64().unwrap() >= 2);
    let settings = Command::SettingsRead(Default::default());
    assert!(call(window, &caller, None, settings, cx).is_ok());
}

#[gpui::test]
fn a_wrong_token_or_an_unknown_session_is_refused_typed_and_never_asked(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    // An approved external client of the same name does not help.
    approve(window, &[("Claude Code", &Scope::ALL)], cx);
    let borrowed = SessionCredential {
        id: b,
        token: monocode_core::app_mcp::token(a),
    };
    let refused = session_hello(window, borrowed, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied, "{}", refused.message);
    let empty = SessionCredential {
        id: b,
        token: String::new(),
    };
    let refused = session_hello(window, empty, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied);
    let unknown = session_hello(window, credential(9_999), cx).unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound, "{}", unknown.message);
    let gone = call(window, &session_caller_name(9_999), None, projects(), cx);
    assert_eq!(gone.unwrap_err().code, ErrorCode::NotFound);
    assert!(cx.pending_prompt().is_none(), "no approval prompt either");
    // An external client cannot take a session's name.
    let named = hello(window, &session_caller_name(b), cx)
        .try_recv()
        .unwrap();
    assert_eq!(named.unwrap_err().code, ErrorCode::InvalidInput);
}

#[gpui::test]
fn an_external_hello_still_asks_the_person_while_control_is_on(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[], cx);
    let answer = hello(window, "Cursor", cx);
    let (message, _) = cx.pending_prompt().expect("the person is asked");
    assert!(
        message.contains("Allow “Cursor” to control MonoCode?"),
        "{message}"
    );
    assert!(answer.try_recv().is_err(), "waiting on the person");
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    let declined = answer.try_recv().unwrap().unwrap_err();
    assert_eq!(declined.code, ErrorCode::UserDeclined);
}

#[gpui::test]
fn a_session_relay_calls_as_its_session_over_the_socket(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let endpoint = Endpoint::for_data_dir(&scratch("s3")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let mut hello = external("Claude Code");
    hello.session = Some(credential(b));
    let connection = Connection::connect(&endpoint).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    frame::write_frame(
        &mut &connection,
        &ClientFrame::Hello(hello),
        MAX_REQUEST_BYTES,
    )
    .unwrap();
    let request = requests.recv_blocking().unwrap();
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(request, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    let mut reader = BufReader::new(&connection);
    let welcome = frame::read_frame(&mut reader, MAX_RESPONSE_BYTES).unwrap();
    let Some(ServerFrame::Welcome(welcome)) = welcome else {
        unreachable!("welcomed, not {welcome:?}")
    };
    assert_eq!(welcome.grant.scopes, Scope::ALL.to_vec());
    let call = frame::Call {
        id: 1,
        request_id: None,
        command: projects(),
    };
    frame::write_frame(
        &mut &connection,
        &ClientFrame::Call(call),
        MAX_REQUEST_BYTES,
    )
    .unwrap();
    let request = requests.recv_blocking().unwrap();
    let Request::Call { caller, .. } = &request else {
        unreachable!("the call")
    };
    assert_eq!(*caller, CallerKind::Session(b), "its session's own");
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(request, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    let reply = frame::read_frame(&mut reader, MAX_RESPONSE_BYTES).unwrap();
    let Some(ServerFrame::Reply(reply)) = reply else {
        unreachable!("answered, not {reply:?}")
    };
    assert!(matches!(reply.result, frame::Outcome::Ok(_)), "{reply:?}");
}

#[gpui::test]
fn a_session_cannot_stop_archive_delete_configure_or_wait_on_itself(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    let caller = session_caller_name(b);
    let own = SessionInput { session_id: b };
    let wait = |session_id| {
        Command::SessionWait(SessionWaitInput {
            session_id,
            timeout_ms: Some(0),
        })
    };
    let configure = Command::SessionConfigure(SessionConfigureInput {
        session_id: b,
        access: Some(AccessMode::ReadOnly),
        ..SessionConfigureInput::default()
    });
    for command in [
        Command::SessionStop(own.clone()),
        Command::SessionArchive(SessionArchiveInput {
            session_id: b,
            stop: None,
        }),
        Command::SessionDelete(own),
        configure,
        wait(b),
    ] {
        let name = command.name();
        let refused = call(window, &caller, None, command, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{name}");
        assert!(
            refused.message.contains("on itself"),
            "{name}: {}",
            refused.message
        );
    }
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries.contains_key(&b))
        })
        .unwrap();
    assert!(
        call(window, &caller, None, wait(a), cx).is_ok(),
        "another session"
    );
}

#[gpui::test]
fn a_sessions_changes_are_logged_under_its_title_and_its_launches_are_its_own(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    let caller = session_caller_name(b);
    let rename = |session_id, title: &str| {
        Command::SessionRename(SessionRenameInput {
            session_id,
            title: title.into(),
        })
    };
    call(window, &caller, None, rename(b, "Fix login"), cx).unwrap();
    call(window, &caller, None, rename(a, "Review docs"), cx).unwrap();
    let start = Command::SessionStart(SessionStartInput {
        prompt: "Check the tests".into(),
        project: Some("/work/another-project".into()),
        draft: Some(true),
        ..SessionStartInput::default()
    });
    let started = call(window, &caller, None, start, cx).unwrap();
    let id = started["sessionId"].as_i64().unwrap();
    window
        .update(cx, |shell, _, _| {
            let log: Vec<_> = shell
                .control
                .activity
                .entries()
                .map(|e| (e.client.clone(), e.operation.clone(), e.outcome.clone()))
                .collect();
            let by = |op: &str| ("Fix login".to_owned(), op.to_owned(), "ok".to_owned());
            assert_eq!(
                log,
                [
                    by("session_rename"),
                    by("session_rename"),
                    by("session_start")
                ]
            );
            assert_eq!(
                shell.started_by.get(&id),
                Some(&b),
                "started by the session"
            );
            assert!(!shell.control.started.contains_key(&id), "not by a client");
        })
        .unwrap();
    let list = call(
        window,
        &caller,
        None,
        Command::SessionList(SessionListInput {
            project: Some("/work/another-project".into()),
            ..SessionListInput::default()
        }),
        cx,
    )
    .unwrap();
    let listed = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap();
    assert_eq!(listed["startedBy"], caller);
}

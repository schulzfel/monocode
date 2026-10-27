//! Capacity and availability: connection slots by kind over real sockets
//! under `target/c/`, the subscriptions of a deleted session, and a socket
//! that cannot be served (told once, and no session gets the relay).
use super::tests::{scratch, sessions, setup};
use super::*;
use crate::runtime::app_control::served;
use gpui::TestAppContext;
use monocode_control::{
    ClientInfo, PROTOCOL_VERSION, Scope, SessionCredential, Topic,
    frame::{self, ClientFrame, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame},
    transport::Connection,
};
use std::{io::BufReader, sync::mpsc, time::Duration};

/// Plays the shell for a listener: grants every hello.
fn grant_hellos(requests: &async_channel::Receiver<Request>) {
    let requests = requests.clone();
    std::thread::spawn(move || {
        while let Ok(request) = requests.recv_blocking() {
            if let Request::Hello { reply, .. } = request {
                let _ = reply.send(Ok(Grant {
                    scopes: vec![Scope::Read],
                }));
            }
        }
    });
}

/// Reads what the app answered first on `connection`.
fn answer(connection: &Connection) -> ServerFrame {
    connection
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    frame::read_frame(&mut BufReader::new(connection), MAX_RESPONSE_BYTES)
        .unwrap()
        .expect("the app answers")
}

/// Says hello on a new connection, as session `session`'s relay when it
/// names one, else as an external client; returns the open connection and
/// what the app answered.
fn open(endpoint: &Endpoint, session: Option<i64>) -> (Connection, ServerFrame) {
    let connection = Connection::connect(endpoint).unwrap();
    let hello = ClientFrame::Hello(Hello {
        version: PROTOCOL_VERSION,
        client: ClientInfo {
            name: "Capacity test".into(),
            version: "1".into(),
        },
        session: session.map(|id| SessionCredential {
            id,
            token: "token".into(),
        }),
    });
    frame::write_frame(&mut &connection, &hello, MAX_REQUEST_BYTES).unwrap();
    let answered = answer(&connection);
    (connection, answered)
}

fn assert_busy(answered: &ServerFrame) {
    let ServerFrame::Error(error) = answered else {
        unreachable!("refused, not {answered:?}")
    };
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(error.retryable, "{error:?}");
    assert_eq!(
        error.message,
        "MonoCode is serving too many control connections"
    );
}

/// Opens `count` connections of one kind, each welcomed.
fn welcomed(endpoint: &Endpoint, count: usize, session: bool) -> Vec<Connection> {
    (0..count)
        .map(|n| {
            let id = session.then(|| i64::try_from(n).unwrap() + 1);
            let (connection, answered) = open(endpoint, id);
            assert!(
                matches!(answered, ServerFrame::Welcome(_)),
                "connection {n} (session {session}): {answered:?}"
            );
            connection
        })
        .collect()
}

#[test]
fn sessions_are_served_beside_every_external_client() {
    let endpoint = Endpoint::for_data_dir(&scratch("c1")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    grant_hellos(&requests);
    let clients = welcomed(&endpoint, monocode_control::frame::MAX_CONNECTIONS, false);
    let (_refused, answered) = open(&endpoint, None);
    assert_busy(&answered);
    let sessions = welcomed(
        &endpoint,
        monocode_control::frame::MAX_CONNECTIONS + 4,
        true,
    );
    assert_eq!((clients.len(), sessions.len()), (16, 20));
}

#[test]
fn each_kind_and_the_hellos_have_their_own_cap() {
    let endpoint = Endpoint::for_data_dir(&scratch("c2")).unwrap();
    let caps = listener::Caps {
        hellos: 2,
        external: 1,
        app: 3,
    };
    let (_server, requests) = listener::Server::start_capped(
        Listener::bind(&endpoint).unwrap(),
        &endpoint,
        "1".into(),
        caps,
    )
    .unwrap();
    grant_hellos(&requests);
    let _sessions = welcomed(&endpoint, 3, true);
    let (_refused, answered) = open(&endpoint, Some(9));
    assert_busy(&answered);
    let _client = welcomed(&endpoint, 1, false);
    // Connections that say nothing hold the hello slots until they do.
    let silent: Vec<Connection> = (0..2)
        .map(|_| Connection::connect(&endpoint).unwrap())
        .collect();
    let late = Connection::connect(&endpoint).unwrap();
    assert_busy(&answer(&late));
    drop(silent);
}

#[gpui::test]
fn deleting_a_session_ends_what_its_relay_follows(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    let follow = |connection, id, cx: &mut TestAppContext| {
        let (events, heard) = mpsc::sync_channel(hub::QUEUE);
        let (reply, answer) = mpsc::sync_channel(1);
        let request = Request::Subscribe {
            connection,
            caller: CallerKind::Session(id),
            topics: vec![Topic::Workspace, Topic::Projects],
            events,
            reply,
        };
        window
            .update(cx, |shell, window, cx| {
                shell.handle_control(request, window, cx)
            })
            .unwrap();
        answer.try_recv().unwrap().unwrap();
        heard
    };
    let deleted = follow(1, a, cx);
    let kept = follow(2, b, cx);
    window
        .update(cx, |shell, window, cx| shell.delete_session(a, window, cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, _| {
            assert!(!shell.session_summaries.contains_key(&a), "deleted");
            assert_eq!(shell.control.hub.followers(), 1, "only its own end");
        })
        .unwrap();
    let told: Vec<ControlError> = deleted
        .try_iter()
        .filter_map(|push| match push {
            hub::Push::Lost(error) => Some(error),
            hub::Push::Event(_) | hub::Push::End => None,
        })
        .collect();
    let [told] = told.as_slice() else {
        unreachable!("told once why, not {told:?}")
    };
    assert_eq!(told.code, ErrorCode::NotFound, "{told}");
    assert!(
        !kept.try_iter().any(|p| matches!(p, hub::Push::Lost(_))),
        "another session keeps following"
    );
}

#[gpui::test]
fn a_socket_that_cannot_be_served_is_told_once_and_gives_no_relay(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            let reason = ControlError::invalid_input("The control socket path is too long");
            shell.control.endpoint = Some(Err(reason));
            shell.apply_control(cx);
            assert_eq!(
                shell.error.as_deref(),
                Some("MonoCode tools are unavailable: The control socket path is too long")
            );
            shell.error = None;
            shell.apply_control(cx);
            assert_eq!(shell.error, None, "told once, not on every apply");
        })
        .unwrap();

    let taken = Endpoint::for_data_dir(&scratch("c3")).unwrap();
    let _other_window = Listener::bind(&taken).unwrap();
    let window = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.control.endpoint = Some(Ok(taken.clone()));
            shell.apply_control(cx);
            assert!(shell.control.server.is_none());
            let error = shell.error.clone().unwrap_or_default();
            assert!(
                error.starts_with("MonoCode tools are unavailable: Another MonoCode window"),
                "{error}"
            );
        })
        .unwrap();
    assert!(
        !served(&taken),
        "a socket that failed to bind is not served"
    );

    let endpoint = Endpoint::for_data_dir(&scratch("c4")).unwrap();
    let window = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.control.endpoint = Some(Ok(endpoint.clone()));
            shell.apply_control(cx);
            assert!(shell.control.server.is_some());
            assert_eq!(shell.error, None);
        })
        .unwrap();
    assert!(served(&endpoint), "sessions get the relay while it serves");
    window
        .update(cx, |shell, _, _| shell.control.server = None)
        .unwrap();
    assert!(!served(&endpoint), "and not once it stops");
}

/// R17: the banner saying the tools are unavailable goes once the socket is
/// served after all, unless another message replaced it meanwhile.
#[gpui::test]
fn the_unavailable_banner_clears_once_the_socket_is_served(cx: &mut TestAppContext) {
    for (name, replaced) in [("c5", false), ("c6", true)] {
        let endpoint = Endpoint::for_data_dir(&scratch(name)).unwrap();
        let other_window = Listener::bind(&endpoint).unwrap();
        let window = setup(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.control.endpoint = Some(Ok(endpoint.clone()));
                shell.apply_control(cx);
                let error = shell.error.clone().unwrap_or_default();
                assert!(
                    error.starts_with("MonoCode tools are unavailable:"),
                    "{error}"
                );
                if replaced {
                    shell.error = Some("Could not save the layout".into());
                }
            })
            .unwrap();
        drop(other_window);
        window
            .update(cx, |shell, _, cx| {
                shell.apply_control(cx);
                assert!(shell.control.server.is_some(), "served now");
                let expected = replaced.then(|| "Could not save the layout".to_owned());
                assert_eq!(shell.error, expected);
            })
            .unwrap();
    }
}

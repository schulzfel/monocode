//! Pushed changes against the fixture shell: who may follow what, the
//! interval that gathers changes, revocation while following, clean-up when
//! a connection closes, and the listener's side over a real socket.
use super::tests::{approve, scratch, sessions, set_busy, setup};
use super::*;
use crate::settings::ExternalControlEdit;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    ClientInfo, PROTOCOL_VERSION, Scope, Topic,
    frame::{
        self, ClientFrame, Event, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame,
        Subscription,
    },
    transport::Connection,
};
use std::{
    io::BufReader,
    sync::mpsc::{self, Receiver, TrySendError},
};

const CLIENT: &str = "Follower";

type Heard = Receiver<hub::Push>;

/// Follows `topics` as connection `connection`; returns the answer and
/// where its events arrive.
fn follow(
    window: WindowHandle<Shell>,
    connection: u64,
    topics: &[Topic],
    cx: &mut TestAppContext,
) -> (Result<Value, ControlError>, Heard) {
    let (events, heard) = mpsc::sync_channel(hub::QUEUE);
    let (reply, answer) = mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let request = Request::Subscribe {
                connection,
                caller: CLIENT.into(),
                topics: topics.to_vec(),
                events,
                reply,
            };
            shell.handle_control(request, window, cx);
        })
        .unwrap();
    (answer.try_recv().unwrap(), heard)
}

fn heard(events: &Heard) -> Vec<(Topic, u64)> {
    events
        .try_iter()
        .filter_map(|push| match push {
            hub::Push::Event(e) => Some((e.topic, e.revision)),
            hub::Push::Lost(_) | hub::Push::End => None,
        })
        .collect()
}

/// Lets the interval pass so gathered changes are sent.
fn flush(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(hub::INTERVAL);
    cx.run_until_parked();
}

/// Switches the current project, which `workspace_read` shows.
fn switch_project(window: WindowHandle<Shell>, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            let other = shell
                .workspace
                .projects
                .iter()
                .find(|p| **p != shell.workspace.project)
                .cloned()
                .unwrap();
            shell.workspace.project = other;
            shell.control_workspace_changed(cx);
        })
        .unwrap();
}

#[gpui::test]
fn following_needs_the_read_scope_and_something_that_exists(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Navigate])], cx);
    let (denied, _) = follow(window, 1, &[Topic::Workspace], cx);
    let denied = denied.unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    assert!(denied.message.contains("read scope"), "{denied}");
    approve(window, &[(CLIENT, &[Scope::Read])], cx);
    let (missing, _) = follow(window, 1, &[Topic::Session { id: 999_999 }], cx);
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
    let project = Topic::ProjectSessions {
        project: "/nowhere".into(),
    };
    let (unknown, _) = follow(window, 1, &[Topic::Projects, project], cx);
    assert_eq!(unknown.unwrap_err().code, ErrorCode::ProjectNotOpen);
    let following = window
        .update(cx, |shell, _, _| shell.control.hub.is_empty())
        .unwrap();
    assert!(following, "a refused subscription follows nothing at all");
    let (ok, _) = follow(window, 1, &[Topic::Workspace, Topic::Projects], cx);
    assert_eq!(ok.unwrap()["topics"].as_array().unwrap().len(), 2);
}

#[gpui::test]
fn changes_in_one_interval_are_one_event_and_unchanged_reads_none(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read])], cx);
    let (a, _) = sessions(window, cx);
    let project = window
        .update(cx, |shell, _, cx| {
            shell.session_metadata(a, cx).summary.cwd.clone()
        })
        .unwrap();
    let topics = [Topic::Session { id: a }, Topic::ProjectSessions { project }];
    let (ok, events) = follow(window, 1, &topics, cx);
    ok.unwrap();
    for busy in [true, false, true] {
        set_busy(window, a, busy, cx);
    }
    cx.run_until_parked();
    assert!(
        heard(&events).is_empty(),
        "nothing before the interval ends"
    );
    flush(cx);
    let first = heard(&events);
    assert_eq!(first.len(), 2, "one event per topic: {first:?}");
    assert!(first.iter().all(|(_, revision)| *revision == 1));
    // A redraw that changes nothing a client reads sends nothing.
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&a].update(cx, |_, cx| cx.notify())
        })
        .unwrap();
    flush(cx);
    assert!(heard(&events).is_empty());
    set_busy(window, a, false, cx);
    flush(cx);
    assert_eq!(
        heard(&events),
        [(topics[1].clone(), 2), (Topic::Session { id: a }, 2)],
        "the revision only grows"
    );
}

#[gpui::test]
fn a_revoked_client_stops_receiving(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(
        window,
        &[(CLIENT, &[Scope::Read]), ("Other", &[Scope::Read])],
        cx,
    );
    let (ok, events) = follow(window, 1, &[Topic::Workspace], cx);
    ok.unwrap();
    switch_project(window, cx);
    flush(cx);
    assert_eq!(heard(&events), [(Topic::Workspace, 1)]);
    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&ExternalControlEdit::Revoke(CLIENT.into()), cx);
        })
        .unwrap();
    switch_project(window, cx);
    flush(cx);
    assert!(
        heard(&events).is_empty(),
        "a revoked client hears nothing more"
    );
    let forgotten = window
        .update(cx, |shell, _, _| shell.control.hub.is_empty())
        .unwrap();
    assert!(forgotten, "and its topics are forgotten");
    // Losing only the read scope mid-subscription stops events the same way.
    approve(window, &[(CLIENT, &[Scope::Read])], cx);
    let (ok, events) = follow(window, 2, &[Topic::Workspace], cx);
    ok.unwrap();
    window
        .update(cx, |shell, _, cx| {
            let edit = ExternalControlEdit::Scope {
                client: CLIENT.into(),
                scope: Scope::Read,
                on: false,
            };
            shell.edit_control(&edit, cx);
        })
        .unwrap();
    switch_project(window, cx);
    flush(cx);
    assert!(heard(&events).is_empty());
}

#[gpui::test]
fn a_closed_connection_is_forgotten_with_the_chats_it_watched(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read])], cx);
    let (a, b) = sessions(window, cx);
    let unwatched = window
        .update(cx, |shell, _, _| shell.control.watched.is_empty())
        .unwrap();
    assert!(unwatched, "nobody follows, so no chat is watched");
    let topics = [Topic::SessionTranscript { id: a }, Topic::Session { id: b }];
    let (ok, events) = follow(window, 7, &topics, cx);
    ok.unwrap();
    let watched: Vec<i64> = window
        .update(cx, |shell, _, _| {
            let mut ids: Vec<_> = shell.control.watched.keys().copied().collect();
            ids.sort_unstable();
            ids
        })
        .unwrap();
    assert_eq!(watched, [a, b]);
    // The transcript observer marks its topic; a turn changes what it shows.
    set_busy(window, a, true, cx);
    flush(cx);
    assert_eq!(heard(&events), [(Topic::SessionTranscript { id: a }, 1)]);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(Request::Closed { connection: 7 }, window, cx);
            assert!(shell.control.hub.is_empty());
            assert!(shell.control.watched.is_empty(), "its chats are let go");
        })
        .unwrap();
    set_busy(window, b, true, cx);
    flush(cx);
    assert!(heard(&events).is_empty());
}

fn write(connection: &Connection, frame: &ClientFrame) {
    frame::write_frame(&mut &*connection, frame, MAX_REQUEST_BYTES).unwrap();
}

#[test]
fn the_listener_carries_subscriptions_and_events_and_reports_the_close() {
    let endpoint = Endpoint::for_data_dir(&scratch("g9")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let connection = Connection::connect(&endpoint).unwrap();
    write(
        &connection,
        &ClientFrame::Hello(Hello {
            version: PROTOCOL_VERSION,
            client: ClientInfo {
                name: CLIENT.into(),
                version: "1".into(),
            },
            session: None,
        }),
    );
    let Ok(Request::Hello { reply, .. }) = requests.recv_blocking() else {
        unreachable!("hello first")
    };
    let grant = Grant {
        scopes: vec![Scope::Read],
    };
    reply.send(Ok(grant)).unwrap();
    let mut reader = BufReader::new(&connection);
    let mut read = || frame::read_frame::<_, ServerFrame>(&mut reader, MAX_RESPONSE_BYTES);
    let Ok(Some(ServerFrame::Welcome(welcome))) = read() else {
        unreachable!("welcomed")
    };
    assert!(welcome.push, "this app pushes changes");
    let subscription = Subscription {
        id: 3,
        topics: vec![Topic::Projects],
    };
    write(&connection, &ClientFrame::Subscribe(subscription));
    let Ok(Request::Subscribe {
        connection: id,
        events,
        reply,
        topics,
        ..
    }) = requests.recv_blocking()
    else {
        unreachable!("the shell is asked")
    };
    assert_eq!(topics, [Topic::Projects]);
    reply
        .send(Ok(serde_json::json!({"topics": topics})))
        .unwrap();
    let Ok(Some(ServerFrame::Reply(answer))) = read() else {
        unreachable!("answered")
    };
    assert_eq!(answer.id, 3);
    let event = Event {
        topic: Topic::Projects,
        revision: 1,
    };
    events.try_send(hub::Push::Event(event.clone())).unwrap();
    assert_eq!(read().unwrap(), Some(ServerFrame::Event(event)));
    // A client that stops reading: the queue fills and refuses; nothing waits.
    let mut refused = false;
    for revision in 2..200_000 {
        let event = Event {
            topic: Topic::Projects,
            revision,
        };
        if let Err(TrySendError::Full(_)) = events.try_send(hub::Push::Event(event)) {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "a slow client's queue fills instead of blocking the sender"
    );
    connection.shutdown().unwrap();
    drop(reader);
    drop(connection);
    let closed = requests.recv_blocking();
    assert!(
        matches!(closed, Ok(Request::Closed { connection }) if connection == id),
        "the shell hears that the connection that followed topics closed"
    );
}

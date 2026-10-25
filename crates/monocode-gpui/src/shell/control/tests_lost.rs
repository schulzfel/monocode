//! A connection that follows topics is told at once when it may no longer
//! receive events (its client was revoked or lost the read scope, or control
//! was turned off) and is then closed, so a relay notices and follows again
//! later instead of waiting forever. Real sockets under `target/c/`, with
//! the test passing the listener's requests to the shell.
use super::tests::{approve, scratch, setup};
use super::*;
use crate::settings::ExternalControlEdit;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    ClientInfo, PROTOCOL_VERSION, Scope, Topic,
    frame::{
        self, ClientFrame, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame, Subscription,
    },
    transport::Connection,
};
use std::{io::BufReader, time::Duration};

/// Passes the listener's requests to the shell, in order, up to and
/// including the first one `wanted` picks (a closed connection's `Closed`
/// may come first).
fn pass_on(
    requests: &async_channel::Receiver<Request>,
    wanted: fn(&Request) -> bool,
    window: WindowHandle<Shell>,
    cx: &mut TestAppContext,
) {
    loop {
        let request = requests.recv_blocking().unwrap();
        let done = wanted(&request);
        window
            .update(cx, |shell, window, cx| {
                shell.handle_control(request, window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        if done {
            return;
        }
    }
}

fn write(connection: &Connection, frame: &ClientFrame) {
    frame::write_frame(&mut &*connection, frame, MAX_REQUEST_BYTES).unwrap();
}

/// Connects as `client` and follows the workspace.
fn follower(
    endpoint: &Endpoint,
    client: &str,
    requests: &async_channel::Receiver<Request>,
    window: WindowHandle<Shell>,
    cx: &mut TestAppContext,
) -> Connection {
    let connection = Connection::connect(endpoint).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let hello = Hello {
        version: PROTOCOL_VERSION,
        client: ClientInfo {
            name: client.into(),
            version: "1".into(),
        },
        session: None,
    };
    write(&connection, &ClientFrame::Hello(hello));
    pass_on(requests, |r| matches!(r, Request::Hello { .. }), window, cx);
    let subscription = Subscription {
        id: 1,
        topics: vec![Topic::Workspace],
    };
    write(&connection, &ClientFrame::Subscribe(subscription));
    pass_on(
        requests,
        |r| matches!(r, Request::Subscribe { .. }),
        window,
        cx,
    );
    let mut reader = BufReader::new(&connection);
    let Ok(Some(ServerFrame::Welcome(_))) = frame::read_frame(&mut reader, MAX_RESPONSE_BYTES)
    else {
        unreachable!("welcomed")
    };
    let answered = frame::read_frame(&mut reader, MAX_RESPONSE_BYTES);
    let Ok(Some(ServerFrame::Reply(reply))) = answered else {
        unreachable!("following, not {answered:?}")
    };
    assert!(matches!(reply.result, frame::Outcome::Ok(_)));
    drop(reader);
    connection
}

/// The connection says why it ends, and then ends.
fn told(connection: &Connection, code: ErrorCode) {
    let mut reader = BufReader::new(connection);
    let said = frame::read_frame::<_, ServerFrame>(&mut reader, MAX_RESPONSE_BYTES);
    let Ok(Some(ServerFrame::Error(error))) = said else {
        unreachable!("told why ({code:?}), not {said:?}")
    };
    assert_eq!(error.code, code, "{}", error.message);
    let after = frame::read_frame::<_, ServerFrame>(&mut reader, MAX_RESPONSE_BYTES);
    assert!(matches!(after, Ok(None)), "then closed: {after:?}");
}

#[gpui::test]
fn a_follower_that_may_no_longer_follow_is_told_and_closed(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(
        window,
        &[
            ("Revoked", &[Scope::Read]),
            ("Reader", &[Scope::Read]),
            ("Other", &[Scope::Read]),
        ],
        cx,
    );
    let endpoint = Endpoint::for_data_dir(&scratch("x5")).unwrap();
    let (_server, requests) =
        listener::Server::start(Listener::bind(&endpoint).unwrap(), &endpoint, "1".into()).unwrap();
    let edit = |edit: ExternalControlEdit, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, cx| shell.edit_control(&edit, cx))
            .unwrap();
        cx.run_until_parked();
    };
    let revoked = follower(&endpoint, "Revoked", &requests, window, cx);
    edit(ExternalControlEdit::Revoke("Revoked".into()), cx);
    told(&revoked, ErrorCode::ScopeDenied);
    let reader = follower(&endpoint, "Reader", &requests, window, cx);
    let other = follower(&endpoint, "Other", &requests, window, cx);
    edit(
        ExternalControlEdit::Scope {
            client: "Reader".into(),
            scope: Scope::Read,
            on: false,
        },
        cx,
    );
    told(&reader, ErrorCode::ScopeDenied);
    edit(ExternalControlEdit::Enabled(false), cx);
    told(&other, ErrorCode::ControlDisabled);
    let forgotten = window
        .update(cx, |shell, _, _| shell.control.hub.is_empty())
        .unwrap();
    assert!(forgotten, "nobody follows anything any more");
}

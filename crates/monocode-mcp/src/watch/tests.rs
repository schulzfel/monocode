//! Following again after the app hangs up: a refusal (the person said no,
//! took the scope away or turned control off) stops the retries, so the app
//! is not asked again every few seconds; the next tool call tries again.
use super::*;
use crate::{Options, identity, socket::Socket, tools::Request};
use monocode_control::{
    Command, Endpoint, ErrorCode, Grant, PROTOCOL_VERSION, Scope,
    frame::{
        self, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame,
        Welcome,
    },
    ops::app::AppInfoInput,
    socket_path,
    transport::Listener,
};
use std::{
    io::BufReader,
    path::{Path, PathBuf},
    sync::mpsc,
};

fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    assert!(cfg!(windows) || socket_path(&dir).is_ok());
    dir
}

/// A fake app: the first connection is welcomed, follows its topics and is
/// hung up; every later hello is refused with `code` and reported.
fn app(endpoint: &Endpoint, code: ErrorCode) -> mpsc::Receiver<()> {
    let listener = Listener::bind(endpoint).unwrap();
    let (heard, hellos) = mpsc::channel();
    std::thread::spawn(move || {
        let first = listener.accept().unwrap();
        let mut reader = BufReader::new(&first);
        let _hello: ClientFrame = frame::read_frame(&mut reader, MAX_REQUEST_BYTES)
            .unwrap()
            .unwrap();
        let welcome = ServerFrame::Welcome(Welcome {
            version: PROTOCOL_VERSION,
            app_version: "0.16.0".into(),
            grant: Grant {
                scopes: vec![Scope::Read],
            },
            push: true,
        });
        frame::write_frame(&mut &first, &welcome, MAX_RESPONSE_BYTES).unwrap();
        let Some(ClientFrame::Subscribe(subscription)) =
            frame::read_frame(&mut reader, MAX_REQUEST_BYTES).unwrap()
        else {
            unreachable!("the relay follows its topics")
        };
        let reply = ServerFrame::Reply(Reply {
            id: subscription.id,
            result: Outcome::Ok(serde_json::json!({ "topics": subscription.topics })),
        });
        frame::write_frame(&mut &first, &reply, MAX_RESPONSE_BYTES).unwrap();
        first.shutdown().unwrap();
        drop(reader);
        while let Ok(next) = listener.accept() {
            let mut reader = BufReader::new(&next);
            if let Ok(Some(ClientFrame::Hello(_))) =
                frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
            {
                let refusal = ServerFrame::Error(ControlError::new(code, "Not now"));
                let _ = frame::write_frame(&mut &next, &refusal, MAX_RESPONSE_BYTES);
                if heard.send(()).is_err() {
                    return;
                }
            }
        }
    });
    hellos
}

/// Waits for the fake app's next refused hello.
async fn refused_hello(hellos: mpsc::Receiver<()>) -> mpsc::Receiver<()> {
    tokio::task::spawn_blocking(move || {
        // The app is asked again within the wait.
        hellos.recv_timeout(Duration::from_secs(20)).unwrap();
        hellos
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_refused_reconnect_stops_until_the_next_tool_call() {
    let cases = [
        ("w1", ErrorCode::UserDeclined),
        ("w2", ErrorCode::ScopeDenied),
        ("w3", ErrorCode::ControlDisabled),
    ];
    for (folder, code) in cases {
        let endpoint = Endpoint::for_data_dir(&scratch(folder)).unwrap();
        let hellos = app(&endpoint, code);
        let inbox = Arc::new(Inbox::default());
        let socket = Socket::new(Options::new(Ok(endpoint)), inbox.sink());
        let link = Arc::new(Link::new(Box::new(socket)));
        let watch = Arc::new(Watch::new(Arc::clone(&link), inbox));
        watch
            .follow(&[Topic::Workspace], identity::unnamed())
            .await
            .unwrap();
        // The app hung up; the relay connects again and is refused once.
        let hellos = refused_hello(hellos).await;
        let reconnecting = lock(&watch.state).reconnecting.take().unwrap();
        // A relay that keeps asking the app never finishes here.
        let stopped = tokio::time::timeout(Duration::from_secs(20), reconnecting).await;
        assert!(stopped.is_ok(), "{code:?}: the relay keeps asking the app");
        assert_eq!(
            lock(&watch.state).followed,
            BTreeSet::from([Topic::Workspace]),
            "the topics are kept for the next try"
        );
        // The next tool call asks the app again, and hears the same answer.
        let request = Request {
            command: Command::AppInfo(AppInfoInput {}),
            request_id: None,
        };
        assert_eq!(
            link.call(request, identity::unnamed())
                .await
                .unwrap_err()
                .code,
            code
        );
        refused_hello(hellos).await;
    }
}

//! Wire frames, limits, errors, scopes, policy and paging.
use monocode_control::{
    AccessMode, ClientInfo, Command, ControlError, ErrorCode, Grant, PROTOCOL_VERSION, Policy,
    Scope, SessionCredential, Topic,
    frame::{
        self, Call, ClientFrame, Event, FrameError, Hello, MAX_REQUEST_BYTES, Outcome, Reply,
        ServerFrame, Subscription, Welcome,
    },
    ops::{self, project::ProjectListInput},
    valid_client_name,
};
use serde_json::{Value, json};
use std::io::{BufReader, Cursor};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn round_trip_client(frame: &ClientFrame) {
        let mut wire = Vec::new();
        frame::write_frame(&mut wire, frame, MAX_REQUEST_BYTES).unwrap();
        assert_eq!(wire.last(), Some(&b'\n'));
        assert!(
            !wire[..wire.len() - 1].contains(&b'\n'),
            "one line per frame"
        );
        let back: ClientFrame = frame::read_frame(&mut Cursor::new(wire), MAX_REQUEST_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(&back, frame);
    }

    pub(super) fn round_trip_server(frame: &ServerFrame) {
        let mut wire = Vec::new();
        frame::write_frame(&mut wire, frame, frame::MAX_RESPONSE_BYTES).unwrap();
        let back: ServerFrame =
            frame::read_frame(&mut Cursor::new(wire), frame::MAX_RESPONSE_BYTES)
                .unwrap()
                .unwrap();
        assert_eq!(&back, frame);
    }
}
use support::{round_trip_client, round_trip_server};

#[test]
fn every_frame_round_trips() {
    round_trip_client(&ClientFrame::Hello(Hello {
        version: PROTOCOL_VERSION,
        client: ClientInfo {
            name: "Claude Code".into(),
            version: "2.1".into(),
        },
        session: None,
    }));
    round_trip_client(&ClientFrame::Hello(Hello {
        version: PROTOCOL_VERSION,
        client: ClientInfo {
            name: "session:7".into(),
            version: "1".into(),
        },
        session: Some(SessionCredential {
            id: 7,
            token: "0123abcd".into(),
        }),
    }));
    round_trip_client(&ClientFrame::Call(Call {
        id: 7,
        request_id: Some("retry-1".into()),
        command: Command::ProjectList(ProjectListInput::default()),
    }));
    round_trip_server(&ServerFrame::Welcome(Welcome {
        version: PROTOCOL_VERSION,
        app_version: "0.16.0".into(),
        grant: Grant {
            scopes: Scope::DEFAULT_GRANT.to_vec(),
        },
        push: true,
    }));
    round_trip_server(&ServerFrame::Error(ControlError::control_disabled()));
    round_trip_server(&ServerFrame::Reply(Reply {
        id: 7,
        result: Outcome::Ok(json!({"total": 0})),
    }));
    round_trip_server(&ServerFrame::Reply(Reply {
        id: 8,
        result: Outcome::Error(ControlError::new(ErrorCode::ScopeDenied, "no")),
    }));
    round_trip_server(&ServerFrame::Event(Event {
        topic: Topic::Workspace,
        revision: 3,
    }));
    for topics in [
        vec![Topic::Workspace, Topic::Projects],
        vec![
            Topic::ProjectSessions {
                project: "/work/monocode".into(),
            },
            Topic::Session { id: 7 },
            Topic::SessionTranscript { id: 7 },
        ],
    ] {
        round_trip_client(&ClientFrame::Subscribe(Subscription {
            id: 9,
            topics: topics.clone(),
        }));
        round_trip_client(&ClientFrame::Unsubscribe(Subscription { id: 10, topics }));
    }
}

#[test]
fn subscriptions_and_events_have_a_stable_shape_on_the_wire() {
    let subscribe = serde_json::to_value(ClientFrame::Subscribe(Subscription {
        id: 4,
        topics: vec![
            Topic::Workspace,
            Topic::ProjectSessions {
                project: "/a".into(),
            },
            Topic::SessionTranscript { id: 7 },
        ],
    }))
    .unwrap();
    assert_eq!(
        subscribe,
        json!({"type": "subscribe", "id": 4, "topics": [
            {"kind": "workspace"},
            {"kind": "project_sessions", "project": "/a"},
            {"kind": "session_transcript", "id": 7},
        ]})
    );
    let event = serde_json::to_value(ServerFrame::Event(Event {
        topic: Topic::Session { id: 7 },
        revision: 12,
    }))
    .unwrap();
    assert_eq!(
        event,
        json!({"type": "event", "topic": {"kind": "session", "id": 7}, "revision": 12}),
        "an event says only what changed and when; the client reads it again"
    );
    let old: Welcome = serde_json::from_value(json!({
        "version": 1, "app_version": "0.1",
        "grant": {"scopes": ["read"], "ceiling": "supervised"}
    }))
    .unwrap();
    assert!(!old.push, "an app from before subscriptions does not push");
    assert_eq!(
        old.grant.scopes,
        [Scope::Read],
        "and its ceiling is ignored"
    );
    let hello: Hello = serde_json::from_value(json!({
        "version": 1,
        "client": {"name": "Cursor", "version": "1", "executable": "/opt/monocode-gpui"}
    }))
    .unwrap();
    assert_eq!(
        hello.client,
        ClientInfo {
            name: "Cursor".into(),
            version: "1".into(),
        },
        "fields a client sends that the app does not know are ignored"
    );
    let bad = br#"{"type":"subscribe","id":5,"topics":[{"kind":"nothing"}]}"#;
    let Err((id, error)) = frame::decode_client(bad) else {
        unreachable!("an unknown topic is malformed")
    };
    assert_eq!((id, error.code), (Some(5), ErrorCode::InvalidInput));
}

#[test]
fn frames_have_a_stable_shape_on_the_wire() {
    let call = serde_json::to_value(ClientFrame::Call(Call {
        id: 1,
        request_id: None,
        command: Command::AppInfo(ops::app::AppInfoInput {}),
    }))
    .unwrap();
    assert_eq!(
        call,
        json!({"type": "call", "id": 1, "command": {"operation": "app_info", "input": {}}})
    );
    let error = serde_json::to_value(ServerFrame::Error(ControlError::app_not_running())).unwrap();
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "app_not_running");
    assert_eq!(error["retryable"], true);
    let reply = serde_json::to_value(ServerFrame::Reply(Reply {
        id: 2,
        result: Outcome::Ok(json!(1)),
    }))
    .unwrap();
    assert_eq!(
        reply,
        json!({"type": "reply", "id": 2, "result": {"ok": 1}})
    );
}

#[test]
fn lines_are_capped_and_must_end() {
    let exact = format!("{}\n", "a".repeat(8));
    let line = frame::read_line(&mut Cursor::new(exact), 8)
        .unwrap()
        .unwrap();
    assert_eq!(line.len(), 8);
    let over = format!("{}\n", "a".repeat(9));
    assert!(matches!(
        frame::read_line(&mut Cursor::new(over), 8),
        Err(FrameError::TooLarge(8))
    ));
    let unterminated = "abc";
    assert!(matches!(
        frame::read_line(&mut Cursor::new(unterminated), 8),
        Err(FrameError::Io(_))
    ));
    let crlf = frame::read_line(&mut Cursor::new("ab\r\n"), 8)
        .unwrap()
        .unwrap();
    assert_eq!(crlf, b"ab");
    assert!(frame::read_line(&mut Cursor::new(""), 8).unwrap().is_none());
    let mut two = BufReader::new(Cursor::new("1\n2\n"));
    assert_eq!(frame::read_line(&mut two, 8).unwrap().unwrap(), b"1");
    assert_eq!(frame::read_line(&mut two, 8).unwrap().unwrap(), b"2");
    let mut sink = Vec::new();
    assert!(matches!(
        frame::write_frame(&mut sink, &"x".repeat(32), 16),
        Err(FrameError::TooLarge(16))
    ));
    assert!(sink.is_empty(), "an oversized frame writes nothing");
    let error = ControlError::from(FrameError::TooLarge(16));
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn a_malformed_call_still_yields_its_id() {
    let bad = br#"{"type":"call","id":42,"command":{"operation":"nope","input":{}}}"#;
    let (id, error) = frame::decode_client(bad).unwrap_err();
    assert_eq!(id, Some(42));
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let (id, _) = frame::decode_client(b"not json").unwrap_err();
    assert_eq!(id, None);
    assert!(matches!(
        frame::decode_client(
            br#"{"type":"hello","version":1,"client":{"name":"a","version":"1"}}"#
        ),
        Ok(ClientFrame::Hello(_))
    ));
}

#[test]
fn request_and_client_names_are_validated() {
    assert!(frame::valid_request_id("a"));
    assert!(frame::valid_request_id(&"A-z_9".repeat(25)));
    assert!(!frame::valid_request_id(""));
    assert!(!frame::valid_request_id(&"a".repeat(129)));
    assert!(!frame::valid_request_id("has space"));
    assert!(!frame::valid_request_id("dot.ted"));
    assert!(valid_client_name("Claude Code"));
    assert!(!valid_client_name(""));
    assert!(!valid_client_name("   "));
    assert!(!valid_client_name("line\nbreak"));
    assert!(!valid_client_name(&"x".repeat(129)));
}

#[test]
fn error_codes_are_the_closed_snake_case_set() {
    let ids: Vec<_> = ErrorCode::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(
        ids,
        [
            "app_not_running",
            "control_disabled",
            "scope_denied",
            "user_declined",
            "not_found",
            "project_not_open",
            "session_busy",
            "session_not_idle",
            "provider_unavailable",
            "model_unavailable",
            "remote_unsupported",
            "invalid_input",
            "request_conflict",
            "timeout",
            "internal",
        ]
    );
    for code in ErrorCode::ALL {
        assert_eq!(serde_json::to_value(code).unwrap(), Value::from(code.id()));
    }
    assert!(ControlError::app_not_running().retryable);
    assert!(!ControlError::control_disabled().retryable);
    assert!(!ControlError::invalid_input("x").retryable);
    assert!(ControlError::internal("x").retryable(true).retryable);
    assert_eq!(
        ControlError::new(ErrorCode::NotFound, "gone").to_string(),
        "not_found: gone"
    );
}

#[test]
fn access_modes_order_from_least_to_most_and_use_the_app_ids() {
    let ids: Vec<_> = AccessMode::ALL.iter().map(|m| m.id()).collect();
    assert_eq!(
        ids,
        [
            "read-only",
            "supervised",
            "auto-accept-edits",
            "auto",
            "full-access"
        ]
    );
    for mode in AccessMode::ALL {
        assert_eq!(serde_json::to_value(mode).unwrap(), Value::from(mode.id()));
        assert_eq!(AccessMode::from_id(mode.id()), Some(mode));
    }
    assert_eq!(AccessMode::from_id("root"), None);
    assert!(AccessMode::ALL.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(AccessMode::default(), AccessMode::Supervised);
    let scopes: Vec<_> = Scope::ALL.iter().map(|s| s.id()).collect();
    assert_eq!(
        scopes,
        [
            "read",
            "navigate",
            "dispatch",
            "manage",
            "settings",
            "vcs",
            "destructive"
        ]
    );
    for scope in Scope::ALL {
        assert_eq!(
            serde_json::to_value(scope).unwrap(),
            Value::from(scope.id())
        );
    }
}

#[test]
fn policy_defaults_off_and_approves_with_the_default_grant() {
    let policy = Policy::parse(None);
    assert!(!policy.enabled);
    assert_eq!(Policy::parse(Some("not json")), Policy::default());
    let mut policy = Policy::parse(Some(r#"{"enabled":true}"#));
    assert!(policy.enabled);
    policy.approve("Claude Code", 10);
    let client = policy.approve("Claude Code", 20);
    assert_eq!((client.approved_at, client.last_seen), (10, 20));
    assert_eq!(client.scopes, Scope::DEFAULT_GRANT);
    assert_eq!(policy.clients.len(), 1);
    let grant = policy.grant("Claude Code").unwrap();
    assert!(grant.allows(Scope::Manage) && !grant.allows(Scope::Settings));
    assert!(policy.set_scope("Claude Code", Scope::Destructive, true));
    assert!(policy.set_scope("Claude Code", Scope::Read, false));
    assert_eq!(
        policy.client("Claude Code").unwrap().scopes,
        [
            Scope::Navigate,
            Scope::Dispatch,
            Scope::Manage,
            Scope::Destructive
        ]
    );
    assert!(!policy.set_scope("Codex", Scope::Read, true));
    let stored = serde_json::to_string(&policy).unwrap();
    assert_eq!(Policy::parse(Some(&stored)), policy);
    assert!(stored.contains("\"approved_at\":10"));
    assert!(policy.revoke("Claude Code"));
    assert!(!policy.revoke("Claude Code"));
    assert!(policy.grant("Claude Code").is_none());
}

#[test]
fn paging_and_limits() {
    let items: Vec<u32> = (0..5).collect();
    let (page, next) = ops::page(&items, None, 2).unwrap();
    assert_eq!((page, next.as_deref()), (vec![0, 1], Some("2")));
    let (page, next) = ops::page(&items, Some("4"), 2).unwrap();
    assert_eq!((page, next), (vec![4], None));
    let (page, next) = ops::page(&items, Some("5"), 2).unwrap();
    assert_eq!((page, next), (Vec::new(), None));
    assert!(ops::page(&items, Some("6"), 2).is_err());
    assert!(ops::page(&items, Some("x"), 2).is_err());
    assert_eq!(ops::limit(None, 20, 100).unwrap(), 20);
    assert_eq!(ops::limit(Some(100), 20, 100).unwrap(), 100);
    assert!(ops::limit(Some(0), 20, 100).is_err());
    assert!(ops::limit(Some(101), 20, 100).is_err());
}

#[test]
fn a_hello_without_a_session_is_an_external_one_and_stays_version_one() {
    let old = r#"{"type":"hello","version":1,"client":{"name":"Cursor","version":"1"}}"#;
    let Ok(ClientFrame::Hello(hello)) = frame::decode_client(old.as_bytes()) else {
        unreachable!("a version 1 hello still parses")
    };
    assert_eq!(hello.session, None);
    let external = serde_json::to_value(ClientFrame::Hello(hello)).unwrap();
    assert!(external.get("session").is_none(), "nothing new on the wire");
    let session = r#"{"type":"hello","version":1,"client":{"name":"session:3","version":"1"},"session":{"id":3,"token":"t"}}"#;
    let Ok(ClientFrame::Hello(hello)) = frame::decode_client(session.as_bytes()) else {
        unreachable!("a session hello parses")
    };
    assert_eq!(hello.version, PROTOCOL_VERSION);
    assert_eq!(hello.session.map(|s| s.id), Some(3));
}

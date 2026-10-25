//! End to end: a real `rmcp` client talks to the relay over an in-memory
//! duplex, and the relay talks to a fake app over a real local socket under
//! the checkout's `target/c/` (a named pipe on Windows).
#[cfg(test)]
pub mod common;

use monocode_control::{
    AccessMode, ControlError, Endpoint, ErrorCode, PROTOCOL_VERSION, Scope, catalog,
    frame::{
        self, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame,
    },
    ops::{
        app::{AppInfoOutput, DataMode},
        project::{ProjectListOutput, ProjectLocation, ProjectSummary},
        session::{SessionStatus, SessionStatusOutput},
    },
};
use monocode_mcp::Options;
use rmcp::model::{ProtocolVersion, ReadResourceRequestParams, ResourceContents};
use serde_json::{Value, json};
use std::{
    io::BufReader,
    sync::{Arc, Mutex},
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn app_info() -> AppInfoOutput {
        AppInfoOutput {
            app_version: "0.16.0".into(),
            protocol_version: PROTOCOL_VERSION,
            data_mode: DataMode::Dev,
            platform: "macos".into(),
            scopes: vec![Scope::Read],
            control_enabled: true,
        }
    }

    pub(super) fn projects() -> ProjectListOutput {
        ProjectListOutput {
            projects: vec![ProjectSummary {
                path: "/work/monocode".into(),
                key: "/work/monocode".into(),
                name: "monocode".into(),
                color: "#459cf7".into(),
                current: true,
                archived: false,
                location: ProjectLocation::Local,
            }],
            total: 1,
            next_cursor: None,
        }
    }

    pub(super) fn status() -> SessionStatusOutput {
        SessionStatusOutput {
            session_id: 7,
            project: "/work/monocode".into(),
            status: SessionStatus::Working,
            status_text: "Streaming…".into(),
            busy: true,
            pending_approvals: 0,
            pending_questions: 0,
            queue_length: 1,
            context: None,
            harness: "claude".into(),
            model: Some("sonnet".into()),
            access: AccessMode::Supervised,
        }
    }

    /// Each call as its command and request ID.
    pub(super) type Calls = Arc<Mutex<Vec<(Value, Option<String>)>>>;

    /// Who said hello, and each call.
    #[derive(Clone, Default)]
    pub(super) struct Seen {
        pub(super) hellos: Arc<Mutex<Vec<String>>>,
        pub(super) calls: Calls,
    }

    /// A fake app that does not push: welcomes every client, answers `app_info`,
    /// `project_list` and `session_status` (session 7 only), echoes `session_send`,
    /// refuses `model_list` with `scope_denied`, and records what it saw.
    pub(super) fn fake_app(endpoint: &Endpoint) -> Seen {
        let seen = Seen::default();
        let record = seen.clone();
        common::fake_app(endpoint, move |connection| {
            let mut reader = BufReader::new(connection);
            while let Ok(Some(next)) =
                frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
            {
                let answer = match next {
                    ClientFrame::Subscribe(_) | ClientFrame::Unsubscribe(_) => continue,
                    ClientFrame::Hello(hello) => {
                        record.hellos.lock().unwrap().push(hello.client.name);
                        welcome(vec![Scope::Read])
                    }
                    ClientFrame::Call(call) => {
                        let input = serde_json::to_value(&call.command).unwrap();
                        record
                            .calls
                            .lock()
                            .unwrap()
                            .push((input.clone(), call.request_id.clone()));
                        ServerFrame::Reply(Reply {
                            id: call.id,
                            result: answer(call.command.name(), &input),
                        })
                    }
                };
                if frame::write_frame(&mut &*connection, &answer, MAX_RESPONSE_BYTES).is_err() {
                    break;
                }
            }
        });
        seen
    }

    /// What the fake app answers a call to `name` with `input`.
    fn answer(name: &str, input: &Value) -> Outcome {
        match name {
            "app_info" => Outcome::Ok(serde_json::to_value(app_info()).unwrap()),
            "project_list" => Outcome::Ok(serde_json::to_value(projects()).unwrap()),
            "session_status" if input["input"]["sessionId"] == 7 => {
                Outcome::Ok(serde_json::to_value(status()).unwrap())
            }
            "session_status" => Outcome::Error(ControlError::new(
                ErrorCode::NotFound,
                "There is no session 8",
            )),
            "session_send" => Outcome::Ok(json!({
                "sessionId": 7, "delivery": "queued", "queueLength": 2
            })),
            _ => Outcome::Error(ControlError::new(
                ErrorCode::ScopeDenied,
                "This client may not list models",
            )),
        }
    }
}
use common::{call, scratch, text, welcome};
use support::{app_info, fake_app, projects, status};

async fn connect(options: Options, version: ProtocolVersion) -> common::Session {
    common::connect(options, version, "e2e-client").await
}

#[tokio::test]
async fn tools_list_publishes_the_catalog_with_schemas_and_annotations() {
    let options = Options::new(Endpoint::for_data_dir(&scratch("m1")));
    for version in [ProtocolVersion::LATEST, ProtocolVersion::V_2025_11_25] {
        let session = connect(options.clone(), version.clone()).await;
        let info = session.peer().peer_info().unwrap();
        assert_eq!(info.server_info.as_ref().unwrap().name, "monocode");
        let instructions = info.instructions.clone().unwrap();
        assert!(!instructions.contains("ceiling"), "{instructions}");
        assert!(instructions.contains("session_start") && instructions.contains("session_wait"));
        let tools = session.peer().list_all_tools().await.unwrap();
        let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
        let mut catalog_names: Vec<_> = catalog().iter().map(|d| d.name).collect();
        catalog_names.push("app_launch");
        assert_eq!(names, catalog_names, "{version}");
        assert_eq!(names.len(), 66, "65 catalog tools, then app_launch");
        for (tool, descriptor) in tools.iter().zip(catalog()) {
            assert_eq!(*tool.input_schema, descriptor.input_schema);
            assert_eq!(
                tool.output_schema.as_deref(),
                Some(&descriptor.output_schema)
            );
            assert_eq!(tool.description.as_deref(), Some(descriptor.description));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.read_only_hint, Some(descriptor.hints.read_only));
            assert_eq!(
                annotations.destructive_hint,
                Some(descriptor.hints.destructive)
            );
            assert_eq!(
                annotations.idempotent_hint,
                Some(descriptor.hints.idempotent)
            );
            assert_eq!(annotations.open_world_hint, Some(false));
        }
        let start = tools.iter().find(|t| t.name == "session_start").unwrap();
        let start = start.annotations.as_ref().unwrap();
        assert_eq!(
            (start.read_only_hint, start.idempotent_hint),
            (Some(false), Some(false))
        );
        assert_eq!(tools[2].title.as_deref(), Some("List projects"));
        session.cancel().await.unwrap();
    }
}

#[tokio::test]
async fn a_call_returns_structured_content_and_a_one_line_summary() {
    let endpoint = Endpoint::for_data_dir(&scratch("m2")).unwrap();
    let hellos = fake_app(&endpoint);
    let session = connect(Options::new(Ok(endpoint)), ProtocolVersion::LATEST).await;
    let result = call(&session, "app_info", json!({})).await;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content,
        Some(serde_json::to_value(app_info()).unwrap())
    );
    assert_eq!(
        text(&result),
        "MonoCode 0.16.0 on macos (development data); scopes: read"
    );
    let result = call(&session, "project_list", json!({"limit": 5})).await;
    assert_eq!(
        result.structured_content,
        Some(serde_json::to_value(projects()).unwrap())
    );
    assert_eq!(text(&result), "1 of 1 projects; current: monocode");
    assert_eq!(
        *hellos.hellos.lock().unwrap(),
        ["e2e-client"],
        "one lazy connection, named after the MCP client"
    );
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn control_errors_become_is_error_results_with_code_message_and_retryable() {
    let endpoint = Endpoint::for_data_dir(&scratch("m3")).unwrap();
    fake_app(&endpoint);
    let session = connect(Options::new(Ok(endpoint)), ProtocolVersion::V_2025_11_25).await;
    let denied = call(&session, "model_list", json!({"search": "sonnet"})).await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(
        denied.structured_content,
        Some(json!({
            "code": "scope_denied",
            "message": "This client may not list models",
            "retryable": false
        }))
    );
    assert_eq!(
        text(&denied),
        "scope_denied: This client may not list models"
    );
    let invalid = call(&session, "project_list", json!({"page": 2})).await;
    assert_eq!(invalid.is_error, Some(true));
    assert_eq!(invalid.structured_content.unwrap()["code"], "invalid_input");
    let unknown = call(&session, "project_delete", json!({})).await;
    assert_eq!(unknown.is_error, Some(true));
    assert_eq!(unknown.structured_content.unwrap()["code"], "invalid_input");
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn no_app_is_app_not_running_unless_the_probe_says_control_is_off() {
    let endpoint = Endpoint::for_data_dir(&scratch("m4"));
    let session = connect(Options::new(endpoint.clone()), ProtocolVersion::LATEST).await;
    let result = call(&session, "app_info", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    let error = result.structured_content.unwrap();
    assert_eq!(error["code"], "app_not_running");
    assert_eq!(error["retryable"], true);
    session.cancel().await.unwrap();
    let options = Options::new(endpoint).when_unavailable(Arc::new(ControlError::control_disabled));
    let session = connect(options, ProtocolVersion::LATEST).await;
    let result = call(&session, "project_list", json!({})).await;
    assert_eq!(
        result.structured_content.unwrap()["code"],
        "control_disabled"
    );
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn the_client_option_overrides_the_reported_name() {
    let endpoint = Endpoint::for_data_dir(&scratch("m5")).unwrap();
    let hellos = fake_app(&endpoint);
    let options = Options::new(Ok(endpoint)).client_name(Some("Release script".into()));
    let session = connect(options, ProtocolVersion::LATEST).await;
    assert_eq!(
        call(&session, "app_info", json!({})).await.is_error,
        Some(false)
    );
    assert_eq!(*hellos.hellos.lock().unwrap(), ["Release script"]);
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn an_endpoint_error_is_reported_by_every_call() {
    let options = Options::new(Err(ControlError::invalid_input("socket path too long")));
    let session = connect(options, ProtocolVersion::LATEST).await;
    let result = call(&session, "app_info", json!({})).await;
    assert_eq!(result.structured_content.unwrap()["code"], "invalid_input");
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn a_changing_tools_request_id_travels_as_the_calls_request_id() {
    let endpoint = Endpoint::for_data_dir(&scratch("m6")).unwrap();
    let seen = fake_app(&endpoint);
    let session = connect(Options::new(Ok(endpoint)), ProtocolVersion::LATEST).await;
    let sent = call(
        &session,
        "session_send",
        json!({"sessionId": 7, "prompt": "And the docs", "requestId": "send-1"}),
    )
    .await;
    assert_eq!(sent.is_error, Some(false), "{}", text(&sent));
    assert_eq!(text(&sent), "Message to session 7 queued; 2 queued");
    let again = call(
        &session,
        "session_send",
        json!({"sessionId": 7, "prompt": "Again"}),
    )
    .await;
    assert_eq!(again.is_error, Some(false));
    let bad = call(
        &session,
        "session_send",
        json!({"sessionId": 7, "prompt": "x", "requestId": "has space"}),
    )
    .await;
    assert_eq!(bad.structured_content.unwrap()["code"], "invalid_input");
    let read = call(&session, "app_info", json!({"requestId": "r"})).await;
    assert_eq!(
        read.structured_content.unwrap()["code"],
        "invalid_input",
        "a read takes no requestId"
    );
    let calls = seen.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2, "refused calls never reach the app");
    assert_eq!(
        calls[0],
        (
            json!({"operation": "session_send", "input": {"sessionId": 7, "prompt": "And the docs"}}),
            Some("send-1".into())
        )
    );
    assert_eq!(
        calls[1].1, None,
        "without requestId the call acts every time"
    );
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn resources_and_templates_are_listed_with_json_mime_types() {
    let options = Options::new(Endpoint::for_data_dir(&scratch("m7")));
    let session = connect(options, ProtocolVersion::LATEST).await;
    let info = session.peer().peer_info().unwrap();
    let resources_cap = info.capabilities.resources.clone().unwrap();
    assert_eq!(resources_cap.subscribe, Some(true));
    let resources = session.peer().list_all_resources().await.unwrap();
    let uris: Vec<_> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(
        uris,
        [
            "monocode://app",
            "monocode://workspace",
            "monocode://projects"
        ]
    );
    let templates = session.peer().list_all_resource_templates().await.unwrap();
    let patterns: Vec<_> = templates.iter().map(|t| t.uri_template.as_str()).collect();
    assert_eq!(
        patterns,
        [
            "monocode://projects/{project}/sessions",
            "monocode://sessions/{id}",
            "monocode://sessions/{id}/transcript"
        ]
    );
    for mime in resources
        .iter()
        .map(|r| r.mime_type.as_deref())
        .chain(templates.iter().map(|t| t.mime_type.as_deref()))
    {
        assert_eq!(mime, Some("application/json"));
    }
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn reading_a_resource_calls_its_read_operation() {
    let endpoint = Endpoint::for_data_dir(&scratch("m8")).unwrap();
    let seen = fake_app(&endpoint);
    let session = connect(Options::new(Ok(endpoint)), ProtocolVersion::LATEST).await;
    let peer = session.peer();
    let read = |uri| peer.read_resource(ReadResourceRequestParams::new(uri));
    let status_7 = read("monocode://sessions/7").await.unwrap();
    assert_eq!(status_7.ttl_ms, Some(1_000));
    let [
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            ..
        },
    ] = &status_7.contents[..]
    else {
        unreachable!("one text content: {:?}", status_7.contents)
    };
    assert_eq!(uri, "monocode://sessions/7");
    assert_eq!(mime_type.as_deref(), Some("application/json"));
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body, serde_json::to_value(status()).unwrap());
    let missing = read("monocode://sessions/8").await;
    assert!(missing.is_err(), "a failed read is a protocol error");
    assert!(read("monocode://nothing").await.is_err());
    let sessions = read("monocode://projects/%2Fwork%2Fmonocode/sessions").await;
    assert!(sessions.is_err(), "the fake app refuses session_list");
    let calls = seen.calls.lock().unwrap().clone();
    assert_eq!(
        calls
            .into_iter()
            .map(|(command, _)| command)
            .collect::<Vec<_>>(),
        [
            json!({"operation": "session_status", "input": {"sessionId": 7}}),
            json!({"operation": "session_status", "input": {"sessionId": 8}}),
            json!({"operation": "session_list", "input": {"project": "/work/monocode", "limit": 200}}),
        ],
        "each read maps to its operation; an unknown URI calls nothing"
    );
    session.cancel().await.unwrap();
}

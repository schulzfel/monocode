//! `app_launch` end to end through a real `rmcp` client, with an injected
//! spawner, so no test ever starts the real app.
#[cfg(test)]
pub mod common;

use monocode_control::{Endpoint, transport::Listener};
use monocode_mcp::{Launcher, Options, serve};
use rmcp::{
    ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
    },
    service::{RoleClient, RunningService},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// A fake app start: counts calls and, when `listens`, binds the
    /// endpoint as a started app would.
    #[derive(Clone)]
    pub(super) struct Fake {
        pub(super) starts: Arc<AtomicUsize>,
        app: Arc<Mutex<Option<Listener>>>,
    }

    impl Fake {
        pub(super) fn launcher(
            endpoint: &Endpoint,
            enabled: bool,
            listens: bool,
        ) -> (Self, Launcher) {
            let fake = Self {
                starts: Arc::default(),
                app: Arc::default(),
            };
            let (spawned, endpoint) = (fake.clone(), endpoint.clone());
            let launcher = Launcher::new(
                Arc::new(move || {
                    spawned.starts.fetch_add(1, Ordering::SeqCst);
                    if listens {
                        let listener = Listener::bind(&endpoint).map_err(|e| e.to_string())?;
                        *spawned.app.lock().unwrap() = Some(listener);
                    }
                    Ok(())
                }),
                Arc::new(move || enabled),
            )
            .wait(Duration::from_millis(300));
            (fake, launcher)
        }
    }

    pub(super) async fn launch(options: Options) -> CallToolResult {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move { serve(options, server_io).await });
        let session: RunningService<RoleClient, ClientConfig> = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("launch-client", "1"),
        )
        .serve(client_io)
        .await
        .unwrap();
        let tools = session.peer().list_all_tools().await.unwrap();
        let tool = tools.iter().find(|t| t.name == "app_launch").unwrap();
        assert_eq!(tool.title.as_deref(), Some("Launch MonoCode"));
        let hints = tool.annotations.clone().unwrap();
        assert_eq!(
            (
                hints.read_only_hint,
                hints.destructive_hint,
                hints.open_world_hint
            ),
            (Some(false), Some(false), Some(false)),
            "app_launch starts a process: not read-only, nothing outside MonoCode"
        );
        let output = tool.output_schema.clone().unwrap();
        assert_eq!(
            output["required"],
            json!(["launched", "alreadyRunning", "waitedMs"])
        );
        let result = session
            .peer()
            .call_tool(CallToolRequestParams::new("app_launch"))
            .await
            .unwrap();
        session.cancel().await.unwrap();
        result
    }

    pub(super) fn code(result: &CallToolResult) -> Value {
        assert_eq!(result.is_error, Some(true), "{result:?}");
        result.structured_content.clone().unwrap()["code"].clone()
    }
}
use common::scratch;
use support::{Fake, code, launch};

#[tokio::test]
async fn control_off_is_control_disabled_and_nothing_starts() {
    let endpoint = Endpoint::for_data_dir(&scratch("l1")).unwrap();
    let (fake, launcher) = Fake::launcher(&endpoint, false, true);
    let result = launch(Options::new(Ok(endpoint)).launcher(launcher)).await;
    assert_eq!(code(&result), "control_disabled");
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0, "never launched");
}

#[tokio::test]
async fn a_running_app_is_already_running() {
    let endpoint = Endpoint::for_data_dir(&scratch("l2")).unwrap();
    let _app = Listener::bind(&endpoint).unwrap();
    let (fake, launcher) = Fake::launcher(&endpoint, true, true);
    let result = launch(Options::new(Ok(endpoint)).launcher(launcher)).await;
    assert_eq!(
        result.structured_content,
        Some(json!({"launched": false, "alreadyRunning": true, "waitedMs": 0}))
    );
    assert_eq!(fake.starts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_stopped_app_is_started_and_waited_for() {
    let endpoint = Endpoint::for_data_dir(&scratch("l3")).unwrap();
    let (fake, launcher) = Fake::launcher(&endpoint, true, true);
    let result = launch(Options::new(Ok(endpoint)).launcher(launcher)).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let output = result.structured_content.unwrap();
    assert_eq!(
        (&output["launched"], &output["alreadyRunning"]),
        (&json!(true), &json!(false))
    );
    assert!(output["waitedMs"].as_u64().unwrap() < 300);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_app_that_never_listens_times_out() {
    let endpoint = Endpoint::for_data_dir(&scratch("l4")).unwrap();
    let (fake, launcher) = Fake::launcher(&endpoint, true, false);
    let result = launch(Options::new(Ok(endpoint)).launcher(launcher)).await;
    assert_eq!(code(&result), "timeout");
    assert_eq!(fake.starts.load(Ordering::SeqCst), 1, "started once");
    let unconfigured = Endpoint::for_data_dir(&scratch("l5")).unwrap();
    let result = launch(Options::new(Ok(unconfigured))).await;
    assert_eq!(
        code(&result),
        "app_not_running",
        "a relay without a launcher"
    );
}

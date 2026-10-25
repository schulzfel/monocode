//! A relay that speaks for an agent session the app runs: it says hello
//! with the session's credential as `session:<id>`, and its instructions say
//! it runs inside the app. A real `rmcp` client over an in-memory duplex;
//! a fake app on a real local socket under the checkout's `target/c/`.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, PROTOCOL_VERSION, Scope, SessionCredential,
    frame::{
        self, ClientFrame, Hello, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply,
        ServerFrame,
    },
    ops::app::{AppInfoOutput, DataMode},
};
use monocode_mcp::{Options, serve};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation},
    service::{RoleClient, RunningService},
};
use std::{
    io::BufReader,
    sync::{Arc, Mutex},
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) type Hellos = Arc<Mutex<Vec<Hello>>>;

    /// Welcomes every hello with every scope, records it, and answers
    /// `app_info`.
    pub(super) fn fake_app(endpoint: &Endpoint) -> Hellos {
        let hellos = Hellos::default();
        let record = Arc::clone(&hellos);
        common::fake_app(endpoint, move |connection| {
            let mut reader = BufReader::new(connection);
            while let Ok(Some(next)) =
                frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
            {
                let answer = match next {
                    ClientFrame::Hello(hello) => {
                        record.lock().unwrap().push(hello);
                        welcome(Scope::ALL.to_vec())
                    }
                    ClientFrame::Call(call) => ServerFrame::Reply(Reply {
                        id: call.id,
                        result: Outcome::Ok(serde_json::to_value(app_info()).unwrap()),
                    }),
                    ClientFrame::Subscribe(_) | ClientFrame::Unsubscribe(_) => continue,
                };
                if frame::write_frame(&mut &*connection, &answer, MAX_RESPONSE_BYTES).is_err() {
                    break;
                }
            }
        });
        hellos
    }

    fn app_info() -> AppInfoOutput {
        AppInfoOutput {
            app_version: "0.16.0".into(),
            protocol_version: PROTOCOL_VERSION,
            data_mode: DataMode::Dev,
            platform: "macos".into(),
            scopes: Scope::ALL.to_vec(),
            control_enabled: false,
        }
    }

    /// Connects an MCP client named `Claude Code`, calls `app_info`, and
    /// returns the server's instructions.
    pub(super) async fn instructions_after_app_info(options: Options) -> String {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move { serve(options, server_io).await });
        let session: RunningService<RoleClient, ClientConfig> = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("Claude Code", "2.1"),
        )
        .serve(client_io)
        .await
        .unwrap();
        let instructions = session
            .peer()
            .peer_info()
            .unwrap()
            .instructions
            .clone()
            .unwrap();
        let result = session
            .peer()
            .call_tool(CallToolRequestParams::new("app_info"))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        instructions
    }
}
use common::{scratch, welcome};
use support::{fake_app, instructions_after_app_info};

#[tokio::test]
async fn a_session_relay_says_hello_with_its_credential_and_knows_it_runs_inside_monocode() {
    let endpoint = Endpoint::for_data_dir(&scratch("ms1")).unwrap();
    let hellos = fake_app(&endpoint);
    let credential = SessionCredential {
        id: 7,
        token: "c0ffee".into(),
    };
    let options = Options::new(Ok(endpoint))
        .client_name(Some("Ignored".into()))
        .session(Some(credential.clone()));
    let instructions = instructions_after_app_info(options).await;
    let hello = hellos.lock().unwrap().pop().unwrap();
    assert_eq!(hello.client.name, "session:7", "named after its session");
    assert_eq!(hello.session, Some(credential));
    assert!(
        instructions.starts_with("You are running inside MonoCode"),
        "{instructions}"
    );
    assert!(instructions.contains("project_add"), "{instructions}");
    assert!(
        instructions.contains("never read or edit MonoCode's database"),
        "{instructions}"
    );
    assert!(instructions.contains("your own permission mode"));
}

#[tokio::test]
async fn an_external_relay_says_hello_without_a_credential() {
    let endpoint = Endpoint::for_data_dir(&scratch("ms2")).unwrap();
    let hellos = fake_app(&endpoint);
    let instructions = instructions_after_app_info(Options::new(Ok(endpoint))).await;
    let hello = hellos.lock().unwrap().pop().unwrap();
    assert_eq!(hello.client.name, "Claude Code");
    assert_eq!(hello.session, None);
    assert!(
        instructions.contains("never edit MonoCode's database or files directly"),
        "{instructions}"
    );
    assert!(!instructions.contains("ceiling"), "{instructions}");
}

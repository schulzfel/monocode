//! Phase 1 operations: request IDs on changing tools, wire shapes and
//! summaries.
use monocode_control::{
    AccessMode, REQUEST_ID, catalog,
    ops::{
        dispatch::{
            Delivery, SessionSendOutput, SessionStartOutput, SessionStopOutput, SessionWaitOutput,
        },
        session::{SessionStatus, Text},
        tab::{TabCloseOutput, TabOutput},
        workspace::{LayoutStyle, PaneNode, RightPanel, WorkspaceReadOutput},
    },
    summarize,
};
use serde_json::{Value, json};

fn fields(schema: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut names: Vec<_> = schema["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn changing_tools_accept_a_request_id_and_reads_do_not() {
    for descriptor in catalog() {
        let has = fields(&descriptor.input_schema).contains(&REQUEST_ID.to_owned());
        assert_eq!(has, !descriptor.hints.read_only, "{}", descriptor.name);
    }
    let entries = catalog();
    let start = entries.iter().find(|d| d.name == "session_start").unwrap();
    assert_eq!(
        fields(&start.input_schema),
        [
            "access",
            "besideSessionId",
            "draft",
            "effort",
            "harness",
            "model",
            "modelSettings",
            "notifyOnComplete",
            "placement",
            "project",
            "prompt",
            "requestId",
            "reveal",
            "title",
            "workspaceMode",
            "worktreeBase",
            "worktreeCwd"
        ]
    );
    assert_eq!(start.input_schema["required"], json!(["prompt"]));
    let status = entries.iter().find(|d| d.name == "session_status").unwrap();
    assert_eq!(
        fields(&status.output_schema),
        [
            "access",
            "busy",
            "context",
            "harness",
            "model",
            "pendingApprovals",
            "pendingQuestions",
            "project",
            "queueLength",
            "sessionId",
            "status",
            "statusText"
        ]
    );
}

#[test]
fn a_status_is_a_tagged_state_with_a_message_only_on_error() {
    assert_eq!(
        serde_json::to_value(SessionStatus::AwaitingApproval).unwrap(),
        json!({"state": "awaiting_approval"})
    );
    let failed = SessionStatus::Error {
        message: "Saving failed: disk full".into(),
    };
    assert_eq!(
        serde_json::to_value(&failed).unwrap(),
        json!({"state": "error", "message": "Saving failed: disk full"})
    );
    for (status, settled) in [
        (SessionStatus::Loading, false),
        (SessionStatus::Connecting, false),
        (SessionStatus::Working, false),
        (SessionStatus::Stopping, false),
        (SessionStatus::Idle, true),
        (SessionStatus::AwaitingApproval, true),
        (SessionStatus::AwaitingAnswer, true),
        (failed, true),
    ] {
        assert_eq!(status.settled(), settled, "{}", status.id());
    }
}

#[test]
fn new_summaries_are_one_line() {
    let now = TabOutput {
        tab: Some("session:3".into()),
        pane: Some(3),
        project: "/work/monocode".into(),
    };
    let wait = SessionWaitOutput {
        session_id: 3,
        status: SessionStatus::Idle,
        status_text: "Ready".into(),
        timed_out: false,
        reply: Some(Text {
            text: "Fixed the tests.\nAll green.".into(),
            truncated: false,
        }),
    };
    let summaries = [
        (
            "session_start",
            serde_json::to_value(SessionStartOutput {
                session_id: 3,
                project: "/work/monocode".into(),
                harness: "claude".into(),
                model: Some("sonnet".into()),
                effort: None,
                access: AccessMode::ReadOnly,
                worktree_cwd: None,
                submitted: true,
                draft: false,
            })
            .unwrap(),
            "Started session 3 (claude sonnet, read-only) in /work/monocode",
        ),
        (
            "session_send",
            serde_json::to_value(SessionSendOutput {
                session_id: 3,
                delivery: Delivery::Queued,
                queue_length: 2,
            })
            .unwrap(),
            "Message to session 3 queued; 2 queued",
        ),
        (
            "session_wait",
            serde_json::to_value(&wait).unwrap(),
            "Session 3 is idle: Fixed the tests.…",
        ),
        (
            "session_stop",
            serde_json::to_value(SessionStopOutput {
                session_id: 3,
                was_busy: true,
                status: SessionStatus::Stopping,
            })
            .unwrap(),
            "Session 3 stopped; now stopping",
        ),
        (
            "tab_close",
            serde_json::to_value(TabCloseOutput {
                closed: true,
                now: now.clone(),
            })
            .unwrap(),
            "Closed; Showing session:3 in /work/monocode",
        ),
        (
            "workspace_read",
            serde_json::to_value(WorkspaceReadOutput {
                project: "/work/monocode".into(),
                tabs: Vec::new(),
                panes: Some(PaneNode::Pane {
                    pane: 3,
                    session_id: 3,
                    file: false,
                }),
                focused_pane: Some(3),
                reviews: Vec::new(),
                right_panel: RightPanel {
                    open: false,
                    tab: "files".into(),
                },
                layout: LayoutStyle::Focused,
            })
            .unwrap(),
            "0 tabs in /work/monocode",
        ),
    ];
    for (name, output, text) in summaries {
        let summary = summarize(name, &output).unwrap();
        assert_eq!(summary, text);
        assert!(!summary.contains('\n'), "{name}");
    }
}

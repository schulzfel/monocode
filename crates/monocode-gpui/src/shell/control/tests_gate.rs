//! `control_grant` for agent sessions against the fixture shell: every
//! operation of the catalog gets a decision for external clients and for
//! sessions (remote callers, self-targets, orchestration workers, read-only
//! turns), and refusals are logged without receipts.
use super::gate::READ_ONLY;
use super::tests::{CLIENT, approve, call, sessions, setup};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    Command, Descriptor, SessionCredential, catalog,
    ops::{
        organize::SessionRenameInput,
        session::{SessionInput, SessionReadInput},
    },
    session_caller_name,
};
use serde_json::json;

/// Operator's seven read actions, as operations.
const OPERATOR_READS: [&str; 7] = [
    "model_list",
    "session_list",
    "session_read",
    "folder_list",
    "notes_list",
    "notes_read",
    "worktree_list",
];

/// A call of `descriptor`'s operation with its required fields, naming
/// session `target` wherever it takes one. An operation whose required
/// fields this cannot fill fails here, so it gets a decision before it ships.
pub(super) fn sample(descriptor: &Descriptor, target: i64) -> Command {
    // What required fields cannot spell: a card's fields depend on its
    // `type`, and a habit's schedule is an object.
    let spelled = match descriptor.name {
        "mono_card_post" => Some(json!({"type": "choices", "options": ["x"]})),
        "mono_habit_add" => Some(json!({
            "name": "x", "instructions": "x", "schedule": {"kind": "daily"}
        })),
        _ => None,
    };
    if let Some(input) = spelled {
        let wire = json!({"operation": descriptor.name, "input": input});
        return serde_json::from_value(wire).unwrap();
    }
    let schema = Value::Object(descriptor.input_schema.clone());
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut input = serde_json::Map::new();
    if properties.contains_key("sessionId") {
        input.insert("sessionId".into(), json!(target));
    }
    for name in schema["required"].as_array().into_iter().flatten() {
        let name = name.as_str().unwrap();
        if input.contains_key(name) {
            continue;
        }
        let value = match properties[name]["type"].as_str() {
            Some("string") => json!("x"),
            Some("array") => json!([]),
            Some("boolean") => json!(false),
            Some("integer") => json!(target),
            other => panic!("{}: no sample for {name} ({other:?})", descriptor.name),
        };
        input.insert(name.into(), value);
    }
    let wire = json!({"operation": descriptor.name, "input": input});
    serde_json::from_value(wire).unwrap_or_else(|e| panic!("{}: {e}", descriptor.name))
}

/// A new session of `cwd`, with its chat loaded unless `unloaded`.
pub(super) fn add_session(
    window: WindowHandle<Shell>,
    cwd: &str,
    unloaded: bool,
    cx: &mut TestAppContext,
) -> i64 {
    window
        .update(cx, |shell, _, cx| {
            let summary = locked(&shell.store).unwrap().create("claude", cwd).unwrap();
            let id = summary.id;
            if unloaded {
                shell.session_summaries.insert(id, summary);
                shell.order.push(id);
            } else {
                shell.add(summary, cx);
            }
            id
        })
        .unwrap()
}

/// Makes session `id` one of `lead`'s orchestration workers.
pub(super) fn make_worker(
    window: WindowHandle<Shell>,
    lead: i64,
    id: i64,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |shell, _, _| {
            shell.orchestration_summaries.insert(
                lead,
                monocode_core::orchestration::Summary {
                    tasks: vec![monocode_core::orchestration::SummaryTask {
                        session: Some(id),
                        title: "Worker".into(),
                        ..Default::default()
                    }],
                    live: true,
                },
            );
        })
        .unwrap();
}

/// What the gate says to `caller` about `command`.
pub(super) fn grant(
    window: WindowHandle<Shell>,
    caller: &CallerKind,
    command: &Command,
    cx: &mut TestAppContext,
) -> Result<Grant, ControlError> {
    window
        .update(cx, |shell, _, cx| shell.control_grant(caller, command, cx))
        .unwrap()
}

/// Changes session `id`'s chat as `change` says.
pub(super) fn with_view(
    window: WindowHandle<Shell>,
    id: i64,
    change: impl FnOnce(&mut Conversation),
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |view, _| change(view));
        })
        .unwrap();
}

#[gpui::test]
fn every_operation_has_a_gate_decision_for_clients_and_sessions(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, b) = sessions(window, cx);
    let worker = add_session(window, "/work/monocode", false, cx);
    make_worker(window, a, worker, cx);
    let me = CallerKind::Session(b);
    let client = CallerKind::External(CLIENT.into());
    let mut not_on_self = Vec::new();
    for descriptor in catalog() {
        let name = descriptor.name;
        if descriptor.mono_only {
            // Neither may call what only a Mono may (tests_gate_mono.rs).
            for caller in [&client, &me] {
                let refused = grant(window, caller, &sample(&descriptor, a), cx).unwrap_err();
                assert_eq!(refused.code, ErrorCode::ScopeDenied, "{name}");
            }
            continue;
        }
        let (other, own, protected) = (
            sample(&descriptor, a),
            sample(&descriptor, b),
            sample(&descriptor, worker),
        );
        for command in [&other, &own, &protected] {
            assert!(
                grant(window, &client, command, cx).is_ok(),
                "{name}: client"
            );
        }
        assert!(grant(window, &me, &other, cx).is_ok(), "{name}: other");
        match grant(window, &me, &own, cx) {
            Ok(_) => assert!(!own.not_on_self(), "{name}"),
            Err(error) => {
                assert!(own.not_on_self(), "{name}: {error:?}");
                assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
                assert!(error.message.contains("on itself"), "{}", error.message);
                not_on_self.push(name);
            }
        }
        let guarded = protected.target_session() == Some(worker) && !descriptor.hints.read_only;
        match grant(window, &me, &protected, cx) {
            Ok(_) => assert!(!guarded, "{name}: a worker's lead manages it"),
            Err(error) => {
                assert!(guarded, "{name}: {error:?}");
                assert_eq!(error.code, ErrorCode::InvalidInput);
                assert_eq!(
                    error.message,
                    format!(
                        "Session {worker} belongs to an orchestration; only its lead manages it"
                    )
                );
            }
        }
    }
    assert_eq!(
        not_on_self,
        [
            "session_send",
            "session_wait",
            "session_stop",
            "session_configure",
            "session_draft",
            "session_archive",
            "session_delete"
        ]
    );

    // A read-only turn may make exactly the read-only calls.
    with_view(window, b, |view| view.settings.plan = true, cx);
    let mut allowed = Vec::new();
    for descriptor in catalog().into_iter().filter(|d| !d.mono_only) {
        match grant(window, &me, &sample(&descriptor, a), cx) {
            Ok(_) => allowed.push(descriptor.name),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::ScopeDenied, "{}", descriptor.name);
                assert_eq!(error.message, READ_ONLY);
            }
        }
    }
    let reads: Vec<_> = catalog()
        .into_iter()
        .filter(|d| d.hints.read_only && !d.mono_only)
        .map(|d| d.name)
        .collect();
    assert_eq!(allowed, reads);
    for read in OPERATOR_READS {
        assert!(allowed.contains(&read), "{read} stays allowed");
    }

    // A remote caller is refused whatever it calls.
    with_view(
        window,
        b,
        |view| {
            view.settings.plan = false;
            view.summary.cwd = "remote://box/srv/app".into();
        },
        cx,
    );
    for descriptor in catalog() {
        let refused = grant(window, &me, &sample(&descriptor, a), cx).unwrap_err();
        assert_eq!(
            refused.code,
            ErrorCode::RemoteUnsupported,
            "{}",
            descriptor.name
        );
        assert_eq!(
            refused.message,
            "MonoCode tools are not available on remote projects"
        );
    }
}

fn rename(id: i64, title: &str) -> Command {
    Command::SessionRename(SessionRenameInput {
        session_id: id,
        title: title.into(),
    })
}

fn read(id: i64) -> Command {
    Command::SessionRead(SessionReadInput {
        session_id: id,
        ..SessionReadInput::default()
    })
}

/// What the activity log shows, oldest first.
fn log(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Vec<(String, String)> {
    window
        .update(cx, |shell, _, _| {
            shell
                .control
                .activity
                .entries()
                .map(|e| (e.operation.clone(), e.outcome.clone()))
                .collect()
        })
        .unwrap()
}

/// Whether `caller` kept a receipt for `request`.
fn receipt(
    window: WindowHandle<Shell>,
    caller: &str,
    request: &str,
    cx: &mut TestAppContext,
) -> bool {
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .setting(&receipts::key(caller, request))
                .unwrap()
                .is_some()
        })
        .unwrap()
}

#[gpui::test]
fn a_remote_caller_is_refused_logged_and_keeps_no_receipt(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    with_view(window, b, |v| v.summary.cwd = "remote://box/srv".into(), cx);
    let caller = session_caller_name(b);
    let refused = call(window, &caller, Some("r1"), rename(a, "Remote"), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::RemoteUnsupported);
    assert_eq!(
        log(window, cx),
        [("session_rename".into(), "remote_unsupported".into())]
    );
    assert!(
        !receipt(window, &caller, "r1", cx),
        "a refusal is no result"
    );
    let read = call(window, &caller, None, read(a), cx).unwrap_err();
    assert_eq!(read.code, ErrorCode::RemoteUnsupported, "reads too");
}

#[gpui::test]
fn read_only_turns_only_read_whatever_made_them_read_only(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    let unloaded = add_session(window, "/work/monocode", true, cx);
    type Change = fn(&mut Conversation);
    let turns: [(&str, Change); 4] = [
        // A plan or orchestrator turn: the composer's mode was reset on send,
        // the running turn's policy stays.
        ("plan or orchestrator turn", |v| v.app_read_only = true),
        ("read-only access", |v| {
            v.settings.access = monocode_core::agent::AccessMode::ReadOnly;
        }),
        ("plan toggled mid-turn", |v| v.settings.plan = true),
        ("orchestrator mode alone", |v| {
            v.settings.orchestrator = true
        }),
    ];
    let reset: Change = |v| {
        v.app_read_only = false;
        v.settings.plan = false;
        v.settings.orchestrator = false;
        v.settings.access = monocode_core::agent::AccessMode::Supervised;
    };
    for (ix, (turn, change)) in turns.into_iter().enumerate() {
        with_view(window, b, reset, cx);
        with_view(window, b, change, cx);
        let caller = session_caller_name(b);
        let request = format!("turn-{ix}");
        let renamed = call(window, &caller, Some(&request), rename(a, turn), cx);
        if turn == "orchestrator mode alone" {
            // Only the running turn's policy counts, as for Operator.
            assert!(renamed.is_ok(), "{turn}: {renamed:?}");
            continue;
        }
        let refused = renamed.unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (ErrorCode::ScopeDenied, READ_ONLY),
            "{turn}"
        );
        assert!(!receipt(window, &caller, &request, cx), "{turn}");
        assert!(call(window, &caller, None, read(a), cx).is_ok(), "{turn}");
    }
    // A chat that is not loaded says nothing about its policy: read-only.
    let caller = session_caller_name(unloaded);
    let refused = call(window, &caller, None, rename(a, "Unloaded"), cx).unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (ErrorCode::ScopeDenied, READ_ONLY)
    );
    assert!(call(window, &caller, None, read(a), cx).is_ok());
    let outcomes: Vec<_> = log(window, cx).into_iter().map(|(_, o)| o).collect();
    assert_eq!(
        outcomes,
        [
            "scope_denied",
            "scope_denied",
            "scope_denied",
            "ok",
            "scope_denied"
        ],
        "refusals are logged"
    );
}

#[gpui::test]
fn a_session_cannot_message_draft_into_or_manage_itself(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let caller = session_caller_name(b);
    let commands = [
        json!({"operation": "session_send", "input": {"sessionId": b, "prompt": "Hi"}}),
        json!({"operation": "session_wait", "input": {"sessionId": b, "timeoutMs": 0}}),
        json!({"operation": "session_stop", "input": {"sessionId": b}}),
        json!({"operation": "session_configure", "input": {"sessionId": b, "effort": "high"}}),
        json!({"operation": "session_draft", "input": {"sessionId": b, "text": "Later"}}),
        json!({"operation": "session_archive", "input": {"sessionId": b, "stop": true}}),
        json!({"operation": "session_delete", "input": {"sessionId": b}}),
    ];
    for command in commands {
        let command: Command = serde_json::from_value(command).unwrap();
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
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&b].read(cx);
            assert!(!view.has_saved_draft() && view.input.read(cx).content.is_empty());
            assert!(!view.busy && !view.summary.archived);
            assert!(shell.session_summaries.contains_key(&b));
        })
        .unwrap();
}

#[gpui::test]
fn only_a_workers_lead_changes_it_but_anyone_reads_it(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, b) = sessions(window, cx);
    let worker = add_session(window, "/work/monocode", false, cx);
    make_worker(window, a, worker, cx);
    let caller = session_caller_name(b);
    let refused = call(window, &caller, None, rename(worker, "Mine"), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    assert!(refused.message.contains("only its lead manages it"));
    let moved = json!({"operation": "session_move", "input": {"sessionId": worker, "newFolderName": "Mine"}});
    let moved = call(
        window,
        &caller,
        None,
        serde_json::from_value(moved).unwrap(),
        cx,
    );
    assert_eq!(
        moved.unwrap_err().code,
        ErrorCode::InvalidInput,
        "moving is managing"
    );
    assert!(
        call(window, &caller, None, read(worker), cx).is_ok(),
        "reads are fine"
    );
    // External clients keep today's rules: they may rename it, not delete it.
    assert!(call(window, CLIENT, None, rename(worker, "Theirs"), cx).is_ok());
    let delete = Command::SessionDelete(SessionInput { session_id: worker });
    let refused = call(window, CLIENT, None, delete, cx).unwrap_err();
    assert!(
        refused.message.contains("orchestration worker"),
        "{}",
        refused.message
    );
    // Nor does a worker get MonoCode tools of its own.
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let hello = Request::Hello {
                client: monocode_control::ClientInfo {
                    name: "Claude Code".into(),
                    version: "1".into(),
                },
                session: Some(SessionCredential {
                    id: worker,
                    token: monocode_core::app_mcp::token(worker),
                }),
                reply,
            };
            shell.handle_control(hello, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let refused = answer.try_recv().unwrap().unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied, "{}", refused.message);
    assert!(refused.message.contains("orchestration worker"));
}

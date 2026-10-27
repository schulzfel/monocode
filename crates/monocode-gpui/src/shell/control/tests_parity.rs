//! The old app actions against the operations that replaced them (§12
//! T-X1, T-X2): every old action is an operation, every field it took is
//! one of that operation's or dropped on purpose, and every old limit still
//! holds or is documented wider. The old actions are frozen here, so the
//! guard outlives the old validator.
use super::tests::{call, repository, set_busy};
use super::tests_gate::add_session;
use super::*;
use crate::app_cli::legacy::{self, fields};
use crate::monos::tests::{Setup, setup};
use crate::monos::tests_parity_mono::{Caller, chat, control_command};
use gpui::TestAppContext;
use monocode_control::{Command, catalog, session_caller_name};
use serde_json::{Map, json};
use std::collections::BTreeSet;

/// Every old action and the operation it is (§9.2), in the old help's
/// order: the actions the old validator accepted, whose fields
/// `legacy::fields` keeps.
const LEGACY: &[(&str, &str)] = &[
    ("models.list", "model_list"),
    ("sessions.list", "session_list"),
    ("sessions.read", "session_read"),
    ("sessions.start", "session_start"),
    ("sessions.send", "session_send"),
    ("sessions.draft", "session_draft"),
    ("sessions.stop", "session_stop"),
    ("sessions.archive", "session_archive"),
    ("sessions.delete", "session_delete"),
    ("worktrees.list", "worktree_list"),
    ("worktrees.create", "worktree_create"),
    ("folders.list", "folder_list"),
    ("folders.move", "session_move"),
    ("notes.list", "notes_list"),
    ("notes.read", "notes_read"),
    ("notes.write", "notes_write"),
    ("artifacts.list", "mono_artifact_list"),
    ("artifacts.read", "mono_artifact_read"),
    ("artifacts.write", "mono_artifact_write"),
    ("soul.read", "mono_soul_read"),
    ("soul.update", "mono_soul_update"),
    ("memory.read", "mono_memory_read"),
    ("memory.search", "mono_memory_search"),
    ("memory.add", "mono_memory_add"),
    ("memory.replace", "mono_memory_replace"),
    ("memory.remove", "mono_memory_remove"),
    ("habits.list", "mono_habit_list"),
    ("habits.add", "mono_habit_add"),
    ("habits.update", "mono_habit_update"),
    ("habits.run", "mono_habit_run"),
    ("habits.remove", "mono_habit_remove"),
    ("chat.card", "mono_card_post"),
];

/// The fields old actions took that their operations do not, on purpose,
/// and why.
const DROPS: &[(&str, &str)] = &[
    (
        "project",
        "an operation on one session works in that session's project; the old actions only checked it was text",
    ),
    (
        "folderId",
        "folders.move beside newFolderName: the new folder's name won over a folder ID, as before",
    ),
];

/// `action` with `input` as its operation's input, which must be `op`'s.
fn input_of(action: &str, op: &str, input: &Value) -> Map<String, Value> {
    let wire = legacy::translate(action, input).unwrap_or_else(|e| panic!("{action}: {e}"));
    assert_eq!(wire["operation"], op, "{action}");
    wire["input"].as_object().unwrap().clone()
}

/// Values an old action took for `field`, as far as `legacy` tells them
/// apart.
fn samples(field: &str) -> Vec<Value> {
    match field {
        "sessionId" | "besideSessionId" | "limit" | "offset" | "maxChars" | "number" => {
            vec![json!(1)]
        }
        "before" => vec![json!("turn-1"), json!(2)],
        "id" => vec![json!("n1"), json!(3)],
        _ => vec![json!("x")],
    }
}

/// T-X1: each old action is the operation the table says, that operation
/// is in the catalog, and each field the action took is one of the
/// operation's input properties or a documented drop.
#[test]
fn every_old_action_is_an_operation_that_takes_its_fields() {
    let actions: BTreeSet<&str> = LEGACY.iter().map(|(action, _)| *action).collect();
    assert_eq!(actions.len(), LEGACY.len(), "each action once");
    assert_eq!(
        actions,
        legacy::ACTIONS.iter().copied().collect(),
        "the app command knows exactly these"
    );
    let catalog = catalog();
    let mut dropped = BTreeSet::new();
    for (action, op) in LEGACY {
        let taken = fields(action).unwrap_or_else(|| panic!("{action} was an old action"));
        let descriptor = catalog
            .iter()
            .find(|d| d.name == *op)
            .unwrap_or_else(|| panic!("{action}: {op} is not in the catalog"));
        let properties: BTreeSet<&str> = descriptor.input_schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        // What the translation adds of its own is the operation's too.
        let added = input_of(action, op, &json!({}));
        for key in added.keys() {
            assert!(properties.contains(key.as_str()), "{action} adds {key}");
        }
        for field in taken.split_whitespace() {
            for sample in samples(field) {
                let given = input_of(action, op, &json!({ field: sample }));
                let new: Vec<&String> = given.keys().filter(|k| !added.contains_key(*k)).collect();
                if new.is_empty() {
                    assert!(
                        DROPS.iter().any(|(drop, _)| *drop == field),
                        "{action} drops {field} without a reason"
                    );
                    assert!(!properties.contains(field), "{op} takes {field}");
                    dropped.insert(field);
                }
                for key in new {
                    assert!(
                        properties.contains(key.as_str()),
                        "{action}.{field} becomes {key}, which {op} does not take"
                    );
                }
            }
        }
    }
    // Every documented drop happens.
    let moved = input_of(
        "folders.move",
        "session_move",
        &json!({"sessionId": 1, "folderId": "f1", "newFolderName": "New"}),
    );
    assert!(!moved.contains_key("folderId"));
    dropped.insert("folderId");
    let documented: BTreeSet<&str> = DROPS.iter().map(|(drop, _)| *drop).collect();
    assert_eq!(dropped, documented);
}

/// An old limit (§12 T-X2): operation `op`'s `field` took at most `old`
/// (at least, for a minimum) of what the old check counted, and now takes
/// `now`: the same, or wider where §13 says so.
struct Limit {
    op: &'static str,
    field: &'static str,
    old: usize,
    now: usize,
    /// A lower bound, not an upper one.
    min: bool,
    /// What the refusal names.
    says: &'static str,
    /// What it counts, and why `now` is not `old`.
    note: &'static str,
}

const fn max(op: &'static str, field: &'static str, old: usize, says: &'static str) -> Limit {
    Limit {
        op,
        field,
        old,
        now: old,
        min: false,
        says,
        note: "",
    }
}

const LIMITS: &[Limit] = &[
    max("session_start", "prompt", 240_000, "prompt"),
    max("session_start", "title", 160, "title"),
    Limit {
        now: 240_000,
        note: "C-10: a Mono's send and draft took 65,536 bytes; every caller's take 240,000",
        ..max("session_send", "prompt", 65_536, "prompt")
    },
    max("session_read", "limit", 3, "limit"),
    max("session_read", "maxChars", 6_000, "maxChars"),
    Limit {
        min: true,
        ..max("session_read", "maxChars", 200, "maxChars")
    },
    max("notes_list", "limit", 100, "limit"),
    max("notes_write", "body", 240_000, "body"),
    Limit {
        note: "tags per note",
        ..max("notes_write", "tags", 20, "tags")
    },
    Limit {
        note: "bytes per tag",
        ..max("notes_write", "tags[]", 48, "tags")
    },
    max("session_move", "newFolderName", 100, "newFolderName"),
    Limit {
        note: "C-10 widened a Mono's 240 to Operator's 400",
        ..max("worktree_create", "branch", 400, "branch")
    },
    max("worktree_create", "base", 400, "base"),
    max("mono_artifact_write", "title", 512, "title"),
    max("mono_artifact_write", "body", 512 * 1024, "body"),
    max("mono_soul_update", "text", 128 * 1024, "text"),
    Limit {
        note: "bytes as given; the fact is then redacted and its spaces collapsed",
        ..max("mono_memory_add", "fact", 4_000, "fact")
    },
    Limit {
        note: "characters of the fact as kept (`memory::entry`)",
        ..max("mono_memory_add", "fact (kept)", 1_000, "fact")
    },
    max("mono_memory_search", "query", 4_000, "query"),
    max("mono_habit_add", "name", 120, "name"),
    max("mono_habit_add", "instructions", 32_000, "instructions"),
    Limit {
        note: "habits per Mono",
        ..max("mono_habit_add", "habits", 20, "habits")
    },
    Limit {
        note: "replies per card",
        ..max("mono_card_post", "options", 4, "replies")
    },
    Limit {
        note: "characters per reply",
        ..max("mono_card_post", "options[]", 160, "Replies")
    },
    max("mono_card_post", "note", 280, "note"),
    max("mono_card_post", "name", 80, "name"),
    max("mono_card_post", "instructions", 4_000, "instructions"),
];

/// Where the limits are tried: a Mono and two sessions of its project, the
/// second one busy, so a message to it waits in its queue and no provider
/// starts; and a session of a git repository.
struct Fixture {
    mono: Setup,
    caller: i64,
    target: i64,
    in_repo: i64,
}

fn command(op: &str, input: Value) -> Command {
    serde_json::from_value(json!({"operation": op, "input": input}))
        .unwrap_or_else(|e| panic!("{op}: {e}"))
}

/// A base of `n` characters that git reads as `HEAD`.
fn base(n: usize) -> String {
    let steps = (n - 4) / 2;
    format!(
        "HEAD{}{}",
        "~0".repeat(steps),
        "0".repeat(n - 4 - steps * 2)
    )
}

/// `limit`'s operation, with its field at size `n`.
fn at(
    limit: &Limit,
    n: usize,
    f: &Fixture,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let s = &f.mono;
    let text = |c: &str| c.repeat(n);
    let by = |id: i64, input: Value, cx: &mut TestAppContext| {
        let caller = session_caller_name(id);
        call(s.window, &caller, None, command(limit.op, input), cx)
    };
    let mono = |op: &str, input: Value, cx: &mut TestAppContext| {
        control_command(s, Caller::Chat, command(op, input), None, cx)
    };
    let daily = json!({"kind": "daily"});
    match (limit.op, limit.field) {
        ("session_start", "prompt") => {
            by(f.caller, json!({"prompt": text("p"), "draft": true}), cx)
        }
        ("session_start", "title") => by(
            f.caller,
            json!({"prompt": "P", "draft": true, "title": text("t")}),
            cx,
        ),
        ("session_send", "prompt") => mono(
            limit.op,
            json!({"sessionId": f.target, "prompt": text("p")}),
            cx,
        ),
        ("session_read", field) => by(f.caller, json!({"sessionId": f.target, field: n}), cx),
        ("notes_list", "limit") => by(f.caller, json!({"limit": n}), cx),
        ("notes_write", "body") => by(f.caller, json!({"body": text("b")}), cx),
        ("notes_write", "tags") => {
            let tags: Vec<_> = (0..n).map(|i| format!("tag-{i}")).collect();
            by(f.caller, json!({"body": "B", "tags": tags}), cx)
        }
        ("notes_write", "tags[]") => by(f.caller, json!({"body": "B", "tags": [text("t")]}), cx),
        ("session_move", "newFolderName") => by(
            f.caller,
            json!({"sessionId": f.target, "newFolderName": text("f")}),
            cx,
        ),
        ("worktree_create", "branch") => by(f.in_repo, json!({"branch": text("b")}), cx),
        ("worktree_create", "base") => by(
            f.in_repo,
            json!({"branch": format!("base-{n}"), "base": base(n)}),
            cx,
        ),
        ("mono_artifact_write", "title") => mono(
            limit.op,
            json!({"kind": "markdown", "title": text("t"), "body": "B"}),
            cx,
        ),
        ("mono_artifact_write", "body") => mono(
            limit.op,
            json!({"kind": "markdown", "title": "T", "body": text("b")}),
            cx,
        ),
        ("mono_soul_update", "text") => {
            let soul = mono("mono_soul_read", json!({}), cx)?;
            let input = json!({"text": text("s"), "expectedHash": soul["hash"]});
            mono(limit.op, input, cx)
        }
        ("mono_memory_add", "fact") => {
            // A fact that is short once its spaces collapse.
            let fact = format!("{}{}", "f".repeat(10), " ".repeat(n - 10));
            mono(limit.op, json!({"fact": fact}), cx)
        }
        ("mono_memory_add", "fact (kept)") => mono(limit.op, json!({"fact": text("f")}), cx),
        ("mono_memory_search", "query") => mono(limit.op, json!({"query": text("q")}), cx),
        ("mono_habit_add", "name") => mono(
            limit.op,
            json!({"name": text("n"), "instructions": "Go", "schedule": daily}),
            cx,
        ),
        ("mono_habit_add", "instructions") => mono(
            limit.op,
            json!({"name": "N", "instructions": text("i"), "schedule": daily}),
            cx,
        ),
        ("mono_habit_add", "habits") => {
            // The n-th habit: as many as there are now, up to n - 1, first.
            let count = |cx: &mut TestAppContext| {
                let listed = mono("mono_habit_list", json!({}), cx).unwrap();
                listed["habits"].as_array().unwrap().len()
            };
            while count(cx) < n - 1 {
                let input = json!({"name": "Fill", "instructions": "Go", "schedule": daily});
                mono(limit.op, input, cx).unwrap();
            }
            assert_eq!(count(cx), n - 1);
            let input = json!({"name": "Last", "instructions": "Go", "schedule": daily});
            mono(limit.op, input, cx)
        }
        ("mono_card_post", "options") => {
            let options: Vec<_> = (0..n).map(|i| format!("Reply {i}")).collect();
            mono(limit.op, json!({"type": "choices", "options": options}), cx)
        }
        ("mono_card_post", "options[]") => mono(
            limit.op,
            json!({"type": "choices", "options": [text("r")]}),
            cx,
        ),
        ("mono_card_post", "note") => mono(
            limit.op,
            json!({"type": "session", "sessionId": f.target, "note": text("n")}),
            cx,
        ),
        ("mono_card_post", "name") => mono(
            limit.op,
            json!({"type": "habit", "name": text("n"), "instructions": "Go", "schedule": daily}),
            cx,
        ),
        ("mono_card_post", "instructions") => mono(
            limit.op,
            json!({"type": "habit", "name": "N", "instructions": text("i"), "schedule": daily}),
            cx,
        ),
        (op, field) => panic!("no call for {op}.{field}"),
    }
}

/// T-X2: each old limit takes its largest value (its smallest, for a
/// minimum) and refuses one past it, in the operation's own words; where
/// it is wider now, the table says why. Every call goes in through the
/// real input types and the operation's executor, without a socket.
#[gpui::test]
fn every_old_limit_holds_or_is_documented_wider(cx: &mut TestAppContext) {
    let mono = setup(cx);
    chat(&mono, cx);
    let window = mono.window;
    let caller = add_session(window, &mono.project, false, cx);
    let target = add_session(window, &mono.project, false, cx);
    set_busy(window, target, true, cx);
    let repo = repository("tx2");
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    let in_repo = add_session(window, &repo, false, cx);
    let f = Fixture {
        mono,
        caller,
        target,
        in_repo,
    };
    for limit in LIMITS {
        let name = format!("{}.{}", limit.op, limit.field);
        assert!(limit.now >= limit.old, "{name}: narrower than before");
        assert!(
            limit.now == limit.old || !limit.note.is_empty(),
            "{name}: wider without a reason"
        );
        let (fits, past) = if limit.min {
            (limit.now, limit.now - 1)
        } else {
            (limit.now, limit.now + 1)
        };
        let taken = at(limit, fits, &f, cx);
        let refused = at(limit, past, &f, cx);
        let refused = refused.expect_err(&format!("{name} takes {past}"));
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{name}: {refused:?}");
        assert!(
            refused.message.contains(limit.says),
            "{name} at {past}: {}",
            refused.message
        );
        match taken {
            Ok(_) => {}
            // No file system takes a 400-character folder name, and a
            // worktree's folder is named after its branch: the call gets
            // past the limit, and git refuses it.
            Err(error) if name == "worktree_create.branch" => {
                assert_ne!(error.message, refused.message, "{name} takes {fits}");
            }
            Err(error) => panic!("{name} takes {fits}: {error:?}"),
        }
    }
}

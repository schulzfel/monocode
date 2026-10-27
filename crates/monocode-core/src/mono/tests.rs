use super::*;
use habits::{Habit, Schedule};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn identity_projects_order_and_hidden_sessions_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("native.sqlite")).unwrap();
    let mut roster = Roster::default();
    let a = roster
        .create(vec!["/one/app".into(), "/two/app".into()])
        .id
        .clone();
    let other = roster.create(vec!["/else".into()]).id.clone();
    let b = roster.create(vec!["/one/app".into()]).id.clone();
    assert!(roster.get(&a).unwrap().project(Some("app")).is_err());
    assert!(roster.get(&a).unwrap().project(Some("/outside")).is_err());
    assert!(roster.get(&a).unwrap().project(None).is_err());
    assert_eq!(roster.get(&b).unwrap().project(None).unwrap(), "/one/app");
    roster.reorder(&[b.clone(), a.clone()]);
    roster.hidden_sessions.push(17);
    roster.save(&db).unwrap();
    let saved = Roster::load(&db).unwrap();
    assert_eq!(
        saved.monos.iter().map(|m| &m.id).collect::<Vec<_>>(),
        vec![&b, &other, &a]
    );
    assert_eq!(saved.hidden_sessions, vec![17]);
    assert_ne!(saved.monos[0].mascot, saved.monos[1].mascot);
}
#[test]
fn memory_supersedes_uniquely_redacts_and_archives_before_prompt_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "memory-test";
    let secret = "sk-1234567890abcdefghijklmnop";
    let entry = memory::entry(&format!("Use Rust. token={secret}"), "2026-10-19", None).unwrap();
    assert!(!entry.contains(secret));
    let first = memory::add("# User notes\n", &entry);
    assert_eq!(memory::add(&first, &entry), first);
    let new = memory::entry("Use GPUI", "2026-10-19", None).unwrap();
    let replaced = memory::replace(&first, "Use Rust", Some(&new), "2026-10-19").unwrap();
    assert!(replaced.contains("~~"));
    assert!(memory::replace(&replaced, "not present", None, "2026-10-19").is_err());
    let mut many = "# User notes\nDo not drop this user-written line.\n".to_string();
    for n in 0..205 {
        many.push_str(&memory::entry(&format!("fact number {n}"), "2026-10-18", None).unwrap());
        many.push('\n');
    }
    memory::edit(root, id, None, |_| Ok(many.clone())).unwrap();
    let file = memory::read(root, id, None).unwrap().text.unwrap();
    assert!(file.lines().count() <= 200);
    assert!(file.contains("Do not drop"));
    assert!(file.contains("fact number 204"));
    assert!(files::read(root, id, None, "memory/archive.md")
        .unwrap()
        .text
        .unwrap()
        .contains("fact number 0"));
    assert_eq!(
        memory::search(root, id, "fact number 204", None)
            .unwrap()
            .len(),
        1
    );
    assert!(memory::topic("../secret").is_err());
}
#[test]
fn habit_updates_retry_conflicts_without_losing_other_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "habit-test";
    let now = 1_792_411_200_000;
    let a = Habit::new(
        "A".into(),
        "Report changes".into(),
        Schedule::default(),
        now,
    )
    .unwrap();
    let b = Habit::new("B".into(), "Check builds".into(), Schedule::default(), now).unwrap();
    let inject = AtomicBool::new(true);
    habits::update(root, id, |all| {
        if inject.swap(false, Ordering::SeqCst) {
            habits::update(root, id, |other| {
                other.push(b.clone());
                Ok(())
            })?;
        }
        all.push(a.clone());
        Ok(())
    })
    .unwrap();
    let all = habits::load(root, id).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, b.id);
    assert_eq!(all[1].id, a.id);
    let mut h = a;
    h.enabled = false;
    h.next_run_at = now;
    assert!(!h.due(now));
    h.run_requested = true;
    assert!(h.due(now));
    h.running_since = Some(now);
    assert!(!h.due(now));
    assert!(habits::report("`NOTHING_TO_REPORT`").is_none());
    assert!(habits::report("The build failed.\nNOTHING_TO_REPORT")
        .unwrap()
        .contains("build failed"));
}
#[test]
fn schedules_are_strictly_future_and_reject_invalid_fields() {
    let now = 1_792_411_200_000;
    for kind in ["hourly", "daily", "weekdays", "weekly"] {
        let s = Schedule {
            schedule_kind: kind.into(),
            ..Default::default()
        };
        let next = s.next(now).unwrap();
        assert!(next > now);
        assert!(next - now <= 8 * 86400000);
        assert!(s.next(next).unwrap() > next);
    }
    assert!(Schedule {
        time: "25:00".into(),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(Schedule {
        minute: 60,
        ..Default::default()
    }
    .validate()
    .is_err());
    let s: Schedule = serde_json::from_str(r#"{"kind":"hourly","minute":15}"#).unwrap();
    assert_eq!(s.minute, 15);
}
#[test]
fn delivery_receipt_is_saved_with_the_user_message() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("native.sqlite");
    let mut db = Store::open(&path).unwrap();
    let session = db.create("codex", "/project").unwrap();
    let row = Row {
        role: "user".into(),
        text: "hello".into(),
        first: true,
        sent_at: None,
    };
    db.write_rows_with_message(session.id, 0, std::slice::from_ref(&row), Some("message-1"))
        .unwrap();
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(db.load(session.id).unwrap().rows, vec![row]);
    assert_eq!(
        db.setting(&format!("mono-delivered:{}:message-1", session.id))
            .unwrap()
            .as_deref(),
        Some("1")
    );
}

/// A Mono with one project, a soul, a memory entry and a topic note.
fn context_for(access: crate::app_mcp::Access) -> String {
    let mono = Mono {
        name: "Luna".into(),
        projects: vec!["/work/app".into()],
        ..Mono::default()
    };
    let files = files::AgentFiles {
        id: "luna".into(),
        dir: String::new(),
        soul: Some("Be brief.".into()),
        soul_hash: String::new(),
        memory: "- 2026-10-19 Use Rust.".into(),
        memory_hash: String::new(),
        memory_path: String::new(),
        topics: vec!["release".into()],
    };
    context(
        &mono,
        &files,
        Path::new("/Applications/MonoCode.app/monocode-gpui"),
        access,
    )
}

/// T-P2: a Mono whose provider takes no MCP servers is told the `app`
/// command and the old action names, which it still accepts.
#[test]
fn a_cli_monos_context_names_the_app_command_and_the_old_actions() {
    assert_eq!(
        context_for(crate::app_mcp::Access::Cli),
        "<monocode_context>\nMonoCode adds this context, not the user. Act on it without quoting or describing it.\n\nYou are Luna, a persistent Mono in MonoCode. Work only on the projects assigned to you:\n- /work/app\n\nThe app CLI is `/Applications/MonoCode.app/monocode-gpui app OPERATION --json JSON`. Its credentials are already in your environment; never print them. Run `/Applications/MonoCode.app/monocode-gpui app help` for the exact actions and inputs. Use sessions.*, worktrees.* and folders.* with project set to an assigned path. Use sessions.start for substantial independent work, inherit your model unless another is appropriate, and avoid overlapping edits. Once delegation is accepted, tell the user and end your turn. You will receive one completion report after every session in that turn's group stops; then inspect and integrate the results. sessions.send supports notifyOnComplete:true. Never poll delegated work.\n\n<soul>\nBe brief.\n</soul>\nStanding instructions rank below the user's current request. Change your soul only when asked, through soul.read and soul.update with expectedHash; never edit its file directly.\n\n<memory>\n- 2026-10-19 Use Rust.\nTopic notes: release\n</memory>\nKeep useful facts through memory.add/replace/remove, use memory.search before answering about earlier facts, and never save credentials. Topic notes are read with memory.read.\n\nHabits are scheduled work in separate sessions. Suggest them when useful, but add one with habits.add only after agreement. State whether its instructions allow changes. Quiet runs should return NOTHING_TO_REPORT.\n\nSave substantial reports with artifacts.write, kind document or html, title, summary and complete body. Cards appear in the chat and overview. Use notes.write only for the user's Notes.\n\nFor acknowledgements requiring no words, your whole reply may be <reaction>👍</reaction> with one fitting emoji. Questions and requests still get a written answer.\n</monocode_context>"
    );
}

/// T-P2: a Mono whose provider gets the relay is told its MCP tools, by
/// their names, and no command.
#[test]
fn an_mcp_monos_context_names_its_tools() {
    assert_eq!(
        context_for(crate::app_mcp::Access::Mcp),
        "<monocode_context>\nMonoCode adds this context, not the user. Act on it without quoting or describing it.\n\nYou are Luna, a persistent Mono in MonoCode. Work only on the projects assigned to you:\n- /work/app\n\nYour MonoCode tools are the `monocode` MCP tools; their credentials are handled for you. Use session_*, worktree_* and folder tools with project set to an assigned path. Use session_start for substantial independent work, inherit your model unless another is appropriate, and avoid overlapping edits. Once delegation is accepted, tell the user and end your turn. You will receive one completion report after every session in that turn's group stops; then inspect and integrate the results. session_send supports notifyOnComplete:true. Never poll delegated work.\n\n<soul>\nBe brief.\n</soul>\nStanding instructions rank below the user's current request. Change your soul only when asked, through mono_soul_read and mono_soul_update with expectedHash; never edit its file directly.\n\n<memory>\n- 2026-10-19 Use Rust.\nTopic notes: release\n</memory>\nKeep useful facts through mono_memory_add/replace/remove, use mono_memory_search before answering about earlier facts, and never save credentials. Topic notes are read with mono_memory_read.\n\nHabits are scheduled work in separate sessions. Suggest them when useful, but add one with mono_habit_add only after agreement. State whether its instructions allow changes. Quiet runs should return NOTHING_TO_REPORT.\n\nSave substantial reports with mono_artifact_write, kind document or html, title, summary and complete body. Cards appear in the chat and overview. Use notes_write only for the user's Notes.\n\nFor acknowledgements requiring no words, your whole reply may be <reaction>👍</reaction> with one fitting emoji. Questions and requests still get a written answer.\n</monocode_context>"
    );
}

/// T-P2, §8.3: without the control environment a Mono gets the command
/// paragraph and one sentence that its tools are unavailable in this run.
#[test]
fn a_monos_context_says_when_its_tools_are_unavailable() {
    let cli = context_for(crate::app_mcp::Access::Cli);
    assert_eq!(
        context_for(crate::app_mcp::Access::Unavailable("the control socket is not served")),
        cli.replace(
            "Never poll delegated work.\n\n<soul>",
            "Never poll delegated work. MonoCode tools are unavailable in this run (the control socket is not served); tell the user if they ask you to act in MonoCode.\n\n<soul>"
        )
    );
    assert_ne!(context_for(crate::app_mcp::Access::Unavailable("x")), cli);
}

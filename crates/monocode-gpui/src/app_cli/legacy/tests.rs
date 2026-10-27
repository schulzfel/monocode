use super::*;
use monocode_control::Command;

/// `action` with `input`, as the typed command it becomes.
fn command(action: &str, input: Value) -> Command {
    let wire = translate(action, &input).unwrap_or_else(|e| panic!("{action}: {e}"));
    serde_json::from_value(wire).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn wire(action: &str, input: Value) -> Value {
    translate(action, &input).unwrap_or_else(|e| panic!("{action}: {e}"))
}

#[test]
fn every_shared_action_maps_to_a_valid_command() {
    let rows = [
        ("models.list", json!({}), "model_list"),
        (
            "sessions.list",
            json!({"project":"/work/app"}),
            "session_list",
        ),
        (
            "sessions.read",
            json!({"sessionId":3,"project":"/work/app","before":"t-2","limit":2,"maxChars":500}),
            "session_read",
        ),
        (
            "sessions.start",
            json!({"prompt":"Fix it","project":"/work/app","harness":"codex","model":"gpt","modelSettings":{"effort":"high"},"effort":"high","runtimeMode":"auto","title":"Fix","draft":false,"reveal":true,"workspaceMode":"worktree","worktreeBase":"main","worktreeCwd":"/work/app-wt","placement":"right","besideSessionId":4,"notifyOnComplete":true}),
            "session_start",
        ),
        (
            "sessions.send",
            json!({"sessionId":3,"prompt":"Go on","project":"/work/app","notifyOnComplete":true}),
            "session_send",
        ),
        (
            "sessions.draft",
            json!({"sessionId":3,"prompt":"Later"}),
            "session_draft",
        ),
        ("sessions.stop", json!({"sessionId":3}), "session_stop"),
        (
            "sessions.archive",
            json!({"sessionId":3}),
            "session_archive",
        ),
        ("sessions.delete", json!({"sessionId":3}), "session_delete"),
        (
            "worktrees.list",
            json!({"project":"/work/app"}),
            "worktree_list",
        ),
        (
            "worktrees.create",
            json!({"branch":"fix","base":"main","existing":false,"project":"/work/app"}),
            "worktree_create",
        ),
        (
            "folders.list",
            json!({"project":"/work/app"}),
            "folder_list",
        ),
        (
            "folders.move",
            json!({"sessionId":3,"folderId":"f1"}),
            "session_move",
        ),
        ("notes.list", json!({"limit":10,"offset":20}), "notes_list"),
        ("notes.read", json!({"id":"n1"}), "notes_read"),
        (
            "notes.write",
            json!({"id":"n1","title":"T","body":"B","tags":["a"]}),
            "notes_write",
        ),
    ];
    for (action, input, operation) in rows {
        assert_eq!(command(action, input).name(), operation, "{action}");
    }
}

#[test]
fn inputs_change_as_the_table_says() {
    assert_eq!(
        wire("models.list", json!({}))["input"],
        json!({"limit": MAX_MODELS_PER_PROVIDER})
    );
    assert_eq!(
        wire("sessions.list", json!({"project":"/p"}))["input"],
        json!({"project":"/p","archived":"include","limit":200})
    );
    assert_eq!(
        wire(
            "sessions.read",
            json!({"sessionId":"7","project":"/p","before":3})
        )["input"],
        json!({"sessionId":7,"beforeIndex":3})
    );
    assert_eq!(
        wire("sessions.read", json!({"sessionId":7,"before":"turn-3"}))["input"],
        json!({"sessionId":7,"before":"turn-3"})
    );
    assert_eq!(
        wire(
            "sessions.start",
            json!({"prompt":"P","runtimeMode":"read-only","workspaceMode":"checkout","besideSessionId":"4","placement":"down"})
        )["input"],
        json!({"prompt":"P","access":"read-only","workspaceMode":"current","besideSessionId":4,"placement":"down"})
    );
    assert_eq!(
        wire(
            "sessions.send",
            json!({"sessionId":7,"prompt":"P","project":"/p"})
        )["input"],
        json!({"sessionId":7,"prompt":"P","ifIdle":true})
    );
    assert_eq!(
        wire("sessions.draft", json!({"sessionId":7,"prompt":"P"}))["input"],
        json!({"sessionId":7,"text":"P"})
    );
    assert_eq!(
        wire("sessions.archive", json!({"sessionId":7,"project":"/p"}))["input"],
        json!({"sessionId":7,"stop":true})
    );
    for action in ["sessions.stop", "sessions.delete"] {
        assert_eq!(
            wire(action, json!({"sessionId":"7","project":"/p"}))["input"],
            json!({"sessionId":7}),
            "{action}"
        );
    }
    assert_eq!(
        wire("folders.move", json!({"sessionId":7}))["input"],
        json!({"sessionId":7,"removeFromFolder":true})
    );
    assert_eq!(
        wire(
            "folders.move",
            json!({"sessionId":7,"folderId":"f1","newFolderName":"New","project":"/p"})
        )["input"],
        json!({"sessionId":7,"newFolderName":"New"})
    );
    assert_eq!(
        wire("notes.list", json!({"offset":20,"limit":5}))["input"],
        json!({"cursor":"20","limit":5})
    );
    for action in ["notes.read", "notes.write"] {
        assert_eq!(
            wire(action, json!({"id":12}))["input"],
            json!({"id":"12"}),
            "{action}"
        );
    }
    // Worktrees and folders keep their fields by name.
    assert_eq!(
        wire("worktrees.create", json!({"branch":"b","project":"/p"}))["input"],
        json!({"branch":"b","project":"/p"})
    );
}

#[test]
fn unknown_actions_fields_and_session_ids_are_refused() {
    assert_eq!(translate("sessions.nap", &json!({})), Err(UNKNOWN.into()));
    assert_eq!(
        translate("sessions.delete", &json!({"sessionId":1,"force":true})),
        Err("sessions.delete does not take force".into())
    );
    assert_eq!(
        translate("notes.write", &json!({"title":"T","project":"/p"})),
        Err("notes.write does not take project".into())
    );
    assert_eq!(
        translate(
            "sessions.read",
            &json!({"sessionId":"1f0c3b9e-1111-4c4c-8888-123456789abc"})
        ),
        Err(NUMERIC_SESSION.into())
    );
    assert_eq!(
        translate(
            "sessions.send",
            &json!({"sessionId":1,"prompt":"P","project":3})
        ),
        Err("project must be text".into())
    );
    assert_eq!(
        translate("models.list", &json!("all")),
        Err("Input must be an object".into())
    );
}

#[test]
fn every_mono_action_maps_to_its_own_operation() {
    let daily = json!({"kind":"daily","time":"09:00"});
    let rows = [
        (
            "artifacts.list",
            json!({"kind":"html","limit":5,"offset":10}),
            "mono_artifact_list",
        ),
        ("artifacts.read", json!({"id":"a1"}), "mono_artifact_read"),
        (
            "artifacts.write",
            json!({"id":"a1","kind":"markdown","title":"T","body":"B","summary":"S"}),
            "mono_artifact_write",
        ),
        ("soul.read", json!({}), "mono_soul_read"),
        (
            "soul.update",
            json!({"text":"Be brief.","expectedHash":"e3b0"}),
            "mono_soul_update",
        ),
        (
            "memory.read",
            json!({"topic":"Tooling"}),
            "mono_memory_read",
        ),
        (
            "memory.search",
            json!({"query":"release day","since":"7d"}),
            "mono_memory_search",
        ),
        (
            "memory.add",
            json!({"fact":"Ships Fridays","topic":"Release","until":"2026-12-31"}),
            "mono_memory_add",
        ),
        (
            "memory.replace",
            json!({"find":"Fridays","fact":"Ships Thursdays","topic":"Release","until":"2026-12-31"}),
            "mono_memory_replace",
        ),
        (
            "memory.remove",
            json!({"find":"Thursdays","topic":"Release"}),
            "mono_memory_remove",
        ),
        ("habits.list", json!({}), "mono_habit_list"),
        (
            "habits.add",
            json!({"name":"Digest","instructions":"Summarize","schedule":daily}),
            "mono_habit_add",
        ),
        (
            "habits.update",
            json!({"id":"h1","name":"Digest","instructions":"Go","schedule":daily,"enabled":false}),
            "mono_habit_update",
        ),
        ("habits.run", json!({"id":"h1"}), "mono_habit_run"),
        ("habits.remove", json!({"id":"h1"}), "mono_habit_remove"),
        (
            "chat.card",
            json!({"type":"pr","repo":"owner/app","number":7,"note":"Ready"}),
            "mono_card_post",
        ),
        (
            "chat.card",
            json!({"type":"session","sessionId":"12","note":"Look"}),
            "mono_card_post",
        ),
        (
            "chat.card",
            json!({"type":"choices","options":["Ship it","Wait"]}),
            "mono_card_post",
        ),
        (
            "chat.card",
            json!({"type":"habit","name":"Digest","instructions":"Go","schedule":daily}),
            "mono_card_post",
        ),
    ];
    for (action, input, operation) in rows {
        let command = command(action, input);
        assert_eq!(command.name(), operation, "{action}");
        assert!(command.mono_only(), "{action}");
    }
    // The cursor is the offset, as for notes; the rest keep their names.
    assert_eq!(
        wire("artifacts.list", json!({"offset":10,"limit":5}))["input"],
        json!({"cursor":"10","limit":5})
    );
    assert_eq!(
        wire("chat.card", json!({"type":"session","sessionId":"12"}))["input"],
        json!({"type":"session","sessionId":12})
    );
    assert_eq!(
        wire("soul.update", json!({"text":"","expectedHash":"e3b0"}))["input"],
        json!({"text":"","expectedHash":"e3b0"})
    );
}

#[test]
fn a_monos_actions_refuse_the_fields_they_never_took() {
    for (action, input, field) in [
        ("artifacts.list", json!({"cursor":"1"}), "cursor"),
        (
            "artifacts.write",
            json!({"title":"T","body":"B","tags":[]}),
            "tags",
        ),
        ("soul.read", json!({"text":"x"}), "text"),
        ("soul.update", json!({"text":"x","hash":"h"}), "hash"),
        ("memory.search", json!({"query":"q","topic":"t"}), "topic"),
        ("memory.remove", json!({"find":"x","fact":"y"}), "fact"),
        ("habits.run", json!({"id":"h1","enabled":true}), "enabled"),
        (
            "chat.card",
            json!({"type":"pr","number":1,"url":"u"}),
            "url",
        ),
    ] {
        assert_eq!(
            translate(action, &input),
            Err(format!("{action} does not take {field}")),
            "{action}"
        );
    }
    // A card of one type with another's field passes the old field list
    // and is refused by the card's typed input (C-1).
    let wire = translate(
        "chat.card",
        &json!({"type":"choices","options":["a"],"note":"n"}),
    );
    assert!(serde_json::from_value::<Command>(wire.unwrap()).is_err());
    assert_eq!(
        translate("chat.card", &json!({"type":"session","sessionId":"abc"})),
        Err(NUMERIC_SESSION.into())
    );
}

/// The list of old actions is the old validator's: each is one it knew,
/// and each is an operation's alias.
#[test]
fn every_old_action_is_listed_once_and_is_an_alias() {
    assert_eq!(ACTIONS.len(), 32);
    for action in ACTIONS {
        assert!(fields(action).is_some(), "{action}");
        assert_eq!(ACTIONS.iter().filter(|a| *a == action).count(), 1);
        let operation = wire(action, json!({}))["operation"].clone();
        let operation = operation.as_str().unwrap();
        assert!(aliases(operation).contains(action), "{action}");
    }
    assert_eq!(aliases("session_list"), ["sessions.list"]);
    assert_eq!(aliases("notes_write"), ["notes.write"]);
    assert!(aliases("session_wait").is_empty());
}

/// The old validator's type checks, now the typed inputs' (C-1 wording):
/// a flag that is text, an access or placement there is none of.
#[test]
fn wrong_types_are_refused_by_the_typed_inputs() {
    for input in [
        json!({"prompt":"x","draft":"false"}),
        json!({"prompt":"x","reveal":"false"}),
        json!({"prompt":"x","runtimeMode":"unsafe"}),
        json!({"prompt":"x","placement":"left"}),
    ] {
        let wire = translate("sessions.start", &input).unwrap();
        assert!(serde_json::from_value::<Command>(wire).is_err(), "{input}");
    }
    assert_eq!(
        translate(
            "sessions.start",
            &json!({"prompt":"x","sidebarHidden":true})
        ),
        Err("sessions.start does not take sidebarHidden".into())
    );
}

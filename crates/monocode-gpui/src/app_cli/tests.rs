//! The `app` command without an app: its arguments, help, input, envelope
//! and exit codes (T-L1), request IDs (T-L2), the old actions through it
//! (T-L3) and help generated from the catalog (T-L4). Calls that reach a
//! window are in `shell::control::tests_cli` (T-L5).
use super::*;
use monocode_control::catalog;
use monocode_core::app_mcp::{ENV_ENDPOINT, ENV_SESSION, ENV_TOKEN};
use std::{collections::HashMap, path::PathBuf};

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// A folder of this test under the checkout's `target/c`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/c")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A session's environment whose socket nobody serves.
fn unserved(name: &str) -> HashMap<&'static str, String> {
    let socket = scratch(name).join("monocode-control.sock");
    HashMap::from([
        (ENV_ENDPOINT, socket.display().to_string()),
        (ENV_SESSION, "7".to_owned()),
        (ENV_TOKEN, "token".to_owned()),
    ])
}

/// `app WORDS…` with environment `env` and standard input `stdin`.
fn run_with(words: &[&str], env: &HashMap<&'static str, String>, stdin: &[u8]) -> Outcome {
    execute(
        &args(words),
        |key| env.get(key).cloned(),
        &mut std::io::Cursor::new(stdin.to_vec()),
    )
}

fn run(words: &[&str], env: &HashMap<&'static str, String>) -> Outcome {
    run_with(words, env, b"")
}

/// A failure before anything was sent: the old CLI's shape, exit code 1.
fn client_error(outcome: &Outcome) -> &str {
    assert_eq!(outcome.code, 1, "{}", outcome.output);
    assert_eq!(outcome.output["ok"], false);
    assert!(
        outcome.output.get("requestId").is_none(),
        "nothing was sent"
    );
    outcome.output["error"].as_str().unwrap()
}

#[test]
fn help_is_printed_for_no_arguments_help_and_its_flags() {
    let none = HashMap::new();
    for words in [&[][..], &["help"], &["--help"], &["-h"]] {
        let outcome = run(words, &none);
        assert_eq!(outcome.code, 0, "{words:?}");
        assert_eq!(
            outcome.output,
            json!({"help": help::text(false)}),
            "{words:?}"
        );
    }
    let mono = HashMap::from([(ENV_ROLE, "mono".to_owned())]);
    assert_eq!(run(&["help"], &mono).output["help"], help::text(true));
}

#[test]
fn argument_errors_keep_the_old_words() {
    let env = unserved("cli1");
    for (words, error) in [
        (&["session_list", "--json"][..], "--json needs JSON"),
        (&["session_list", "--input"], "--input needs a path"),
        (
            &["session_list", "--request-id"],
            "--request-id needs an ID",
        ),
        (&["session_list", "--nope"], "Unknown app argument: --nope"),
        (&["session_list", "list"], "Unknown app argument: list"),
    ] {
        assert_eq!(client_error(&run(words, &env)), error, "{words:?}");
    }
    let broken = run(&["session_list", "{nope"], &env);
    assert!(client_error(&broken).contains("key must be a string"));
    assert_eq!(
        client_error(&run(&["sessions.nap"], &env)),
        legacy::UNKNOWN,
        "neither an operation nor an old action"
    );
    assert_eq!(
        client_error(&run(&["session_list", "--json", "[]"], &env)),
        "Input must be an object"
    );
    let wrong = run(&["session_read", "--json", r#"{"sessionId":"x"}"#], &env);
    assert!(client_error(&wrong).starts_with("Invalid arguments for session_read:"));
}

#[test]
fn input_comes_from_json_a_file_standard_input_or_a_bare_object() {
    let given = |words: &[&str], stdin: &[u8]| {
        parse(&args(words), &mut std::io::Cursor::new(stdin.to_vec())).map(|p| p.input)
    };
    let object = json!({"sessionId": 3});
    assert_eq!(
        given(&["a", "--json", r#"{"sessionId":3}"#], b""),
        Ok(object.clone())
    );
    assert_eq!(given(&["a", r#"{"sessionId":3}"#], b""), Ok(object.clone()));
    assert_eq!(
        given(&["a", "--input", "-"], br#"{"sessionId":3}"#),
        Ok(object.clone())
    );
    let file = scratch("cli2").join("input.json");
    std::fs::write(&file, r#"{"sessionId":3}"#).unwrap();
    let path = file.display().to_string();
    assert_eq!(given(&["a", "--input", &path], b""), Ok(object));
    assert_eq!(given(&["a"], b""), Ok(json!({})), "no input is {{}}");
    // At most 1 MiB, from a file or standard input.
    let mut large = vec![b' '; MAX_INPUT - 2];
    large.splice(0..0, *b"{}");
    assert_eq!(given(&["a", "--input", "-"], &large), Ok(json!({})));
    large.push(b' ');
    assert_eq!(
        given(&["a", "--input", "-"], &large),
        Err("Input too large".into())
    );
    std::fs::write(&file, &large).unwrap();
    assert_eq!(
        given(&["a", "--input", &path], b""),
        Err("Input too large".into())
    );
}

#[test]
fn outside_a_session_or_with_a_partial_environment_nothing_is_sent() {
    let outside = run(&["session_list"], &HashMap::new());
    assert_eq!(client_error(&outside), OUTSIDE);
    let mut partial = unserved("cli3");
    partial.remove(ENV_TOKEN);
    let refused = run(&["session_list"], &partial);
    assert_eq!(refused.code, 1);
    assert_eq!(refused.output["code"], "invalid_input");
    assert_eq!(refused.output["retryable"], false);
    assert!(refused.output.get("requestId").is_none());
    assert!(
        refused.output["error"]
            .as_str()
            .unwrap()
            .contains("MONOCODE_CONTROL_TOKEN")
    );
}

/// The app's failure as the envelope shows it: here nobody serves the
/// socket, so the call is not answered.
#[test]
fn a_failed_call_prints_its_code_retry_advice_and_request_id() {
    let env = unserved("cli4");
    let outcome = run(&["session_list"], &env);
    assert_eq!(outcome.code, 1);
    let output = &outcome.output;
    assert_eq!(output["ok"], false);
    assert_eq!(output["code"], "app_not_running");
    assert_eq!(output["retryable"], true);
    assert!(output["error"].as_str().unwrap().contains("not running"));
    assert!(frame::valid_request_id(
        output["requestId"].as_str().unwrap()
    ));
}

/// T-L2: each run has its own valid request ID unless `--request-id` or
/// an operation's `requestId` names one. Any ID is taken, as the old CLI
/// took any (R13): the output names it as given.
#[test]
fn request_ids_are_generated_per_run_or_given_and_always_valid() {
    let env = unserved("cli5");
    let id = |words: &[&str]| run(words, &env).output["requestId"].clone();
    let first = id(&["notes_write", "--json", r#"{"body":"B"}"#]);
    let second = id(&["notes_write", "--json", r#"{"body":"B"}"#]);
    assert_ne!(first, second, "a new ID per run");
    for generated in [&first, &second] {
        let generated = generated.as_str().unwrap();
        assert_eq!(generated.len(), 32);
        assert!(frame::valid_request_id(generated));
    }
    assert_eq!(
        id(&[
            "notes.write",
            "--json",
            r#"{"body":"B"}"#,
            "--request-id",
            "note-1"
        ]),
        "note-1"
    );
    assert_eq!(
        id(&[
            "notes_write",
            "--json",
            r#"{"body":"B","requestId":"note-2"}"#
        ]),
        "note-2",
        "an operation's own requestId"
    );
    assert_eq!(
        id(&[
            "notes_write",
            "--json",
            r#"{"body":"B","requestId":"note-2"}"#,
            "--request-id",
            "note-3"
        ]),
        "note-3",
        "the flag wins"
    );
    let long = "x".repeat(129);
    for (words, given) in [
        (
            &["notes_write", "--request-id", "no spaces"][..],
            "no spaces",
        ),
        (&["notes_write", "--request-id", ""][..], ""),
        (&["notes_write", "--request-id", &long][..], long.as_str()),
        (
            &[
                "notes_write",
                "--json",
                r#"{"body":"B","requestId":"nöt ascii"}"#,
            ][..],
            "nöt ascii",
        ),
    ] {
        let outcome = run(words, &env);
        assert_eq!(outcome.output["code"], "app_not_running", "sent: {words:?}");
        assert_eq!(outcome.output["requestId"], given, "{words:?}");
    }
    assert_eq!(
        client_error(&run(&["notes_write", "--json", r#"{"requestId":7}"#], &env)),
        "requestId must be text"
    );
}

/// R13: an ID outside the control pattern goes out as a valid one made from
/// it, the same each time, so its retry is the same request; a valid ID
/// goes as it is.
#[test]
fn a_request_id_outside_the_pattern_maps_to_the_same_valid_one() {
    for given in ["no spaces", "", "nöt ascii", &"x".repeat(129)] {
        let sent = sent_request_id(given);
        assert!(frame::valid_request_id(&sent), "{sent}");
        assert!(sent.starts_with("cli-") && sent.len() == 36, "{sent}");
        assert_eq!(sent, sent_request_id(given), "the same for a retry");
    }
    assert_ne!(sent_request_id("a b"), sent_request_id("a c"));
    assert_eq!(sent_request_id("note-1"), "note-1");
}

/// R11: outside a Mono's chat, the old `folders.move` takes exactly one of
/// `folderId` and `newFolderName`, as Operator's did (OP-48); a Mono's
/// keeps its own grammar, where neither takes the session out of its folder.
#[test]
fn the_old_folder_move_keeps_operators_grammar_outside_a_mono() {
    let env = unserved("cli6");
    let mut mono = env.clone();
    mono.insert(ENV_ROLE, "mono".to_owned());
    for input in [
        r#"{"sessionId":3}"#,
        r#"{"sessionId":3,"folderId":"f1","newFolderName":"New"}"#,
    ] {
        let words = ["folders.move", "--json", input];
        assert_eq!(
            client_error(&run(&words, &env)),
            "Supply exactly one of folderId or newFolderName",
            "{input}"
        );
        let sent = run(&words, &mono);
        assert_eq!(sent.output["code"], "app_not_running", "{input}");
    }
    for input in [
        r#"{"sessionId":3,"folderId":"f1"}"#,
        r#"{"sessionId":3,"newFolderName":"New"}"#,
    ] {
        let sent = run(&["folders.move", "--json", input], &env);
        assert_eq!(sent.output["code"], "app_not_running", "{input}");
    }
}

/// An operation's input reaches it as its tool's arguments, less the
/// `requestId` of one that changes something; an old action is translated.
#[test]
fn operations_take_their_input_and_old_actions_are_translated() {
    let (write, id) = command(
        "notes_write",
        json!({"title":"T","body":"B","requestId":"n1"}),
    )
    .unwrap();
    assert_eq!(write.name(), "notes_write");
    assert_eq!(id.as_deref(), Some("n1"));
    assert_eq!(
        serde_json::to_value(&write).unwrap()["input"],
        json!({"title":"T","body":"B"})
    );
    let (list, id) = command("session_list", json!({"limit": 3})).unwrap();
    assert_eq!((list.name(), id), ("session_list", None));
    // T-L3: every old action becomes the command of its operation.
    for action in legacy::ACTIONS {
        let input = match *action {
            "sessions.read" | "sessions.stop" | "sessions.archive" | "sessions.delete" => {
                json!({"sessionId": "3"})
            }
            "sessions.send" | "sessions.draft" => json!({"sessionId": 3, "prompt": "P"}),
            "sessions.start" => json!({"prompt": "P"}),
            "worktrees.create" => json!({"branch": "b"}),
            "folders.move" => json!({"sessionId": 3}),
            "notes.read" | "artifacts.read" | "habits.run" | "habits.remove" => {
                json!({"id": "x"})
            }
            "artifacts.write" => json!({"kind":"markdown","title":"T","body":"B"}),
            "soul.update" => json!({"text":"","expectedHash":"h"}),
            "memory.search" => json!({"query":"q"}),
            "memory.add" => json!({"fact":"f"}),
            "memory.replace" => json!({"find":"a","fact":"b"}),
            "memory.remove" => json!({"find":"a"}),
            "habits.add" => {
                json!({"name":"n","instructions":"i","schedule":{"kind":"daily","time":"09:00"}})
            }
            "habits.update" => json!({"id":"h"}),
            "chat.card" => json!({"type":"choices","options":["a"]}),
            _ => json!({}),
        };
        let (translated, id) = command(action, input).unwrap_or_else(|e| panic!("{action}: {e}"));
        assert_eq!(legacy::aliases(translated.name()), [*action].as_slice());
        assert_eq!(id, None, "{action}");
    }
}

/// T-L4: help lists every operation of the catalog with its old names and
/// fields, the envelope and the retry rules; a Mono's own operations only
/// for a Mono.
#[test]
fn help_lists_every_operation_and_old_name_from_the_catalog() {
    let session = help::text(false);
    let mono = help::text(true);
    for descriptor in catalog() {
        let line = |text: &str| {
            text.lines()
                .find(|l| l.split_whitespace().next() == Some(descriptor.name))
                .map(str::to_owned)
        };
        let listed = line(&session)
            .or_else(|| line(&mono))
            .unwrap_or_else(|| panic!("{}", descriptor.name));
        assert!(listed.contains(descriptor.title), "{listed}");
        for alias in legacy::aliases(descriptor.name) {
            assert!(listed.contains(alias), "{listed}");
        }
        assert_eq!(
            line(&session).is_some(),
            !descriptor.mono_only,
            "{}",
            descriptor.name
        );
    }
    // R12: a Mono's help lists only what a Mono may call.
    let mut listed: Vec<&str> = mono
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| catalog().iter().any(|d| d.name == *name))
        .collect();
    let mut allowed = crate::shell::MONO_OPS.to_vec();
    listed.sort_unstable();
    allowed.sort_unstable();
    assert_eq!(listed, allowed);
    for name in ["settings_update", "project_delete", "tab_open", "mono_send"] {
        assert!(
            !mono.lines().any(|l| l.starts_with(&format!("{name} "))),
            "{name}"
        );
    }
    for name in ["session_start", "mono_memory_add", "queue_list"] {
        assert!(
            mono.lines().any(|l| l.starts_with(&format!("{name} "))),
            "{name}"
        );
    }
    for action in legacy::ACTIONS {
        assert!(mono.contains(action), "{action}");
    }
    assert!(!session.contains("artifacts.list"));
    let start = session
        .lines()
        .find(|l| l.starts_with("session_start "))
        .unwrap();
    assert!(
        start.starts_with("session_start [sessions.start] {prompt,"),
        "{start}"
    );
    assert!(start.contains(",project?,"), "{start}");
    assert!(
        !start.contains("requestId"),
        "left to the retry rules: {start}"
    );
    // R12: an old name whose fields are not the operation's shows its own.
    let after = |text: &str, name: &str| {
        let mut lines = text.lines();
        lines.find(|l| l.starts_with(&format!("{name} ")));
        lines.next().unwrap_or_default().to_owned()
    };
    assert_eq!(
        after(&session, "session_draft"),
        "  sessions.draft {sessionId,prompt,project}"
    );
    assert_eq!(
        after(&session, "session_start"),
        "  sessions.start {prompt,project,harness,model,modelSettings,effort,runtimeMode,title,draft,reveal,workspaceMode,worktreeBase,worktreeCwd,placement,besideSessionId,notifyOnComplete}"
    );
    // A nested input shows its fields.
    let line = |text: &str, name: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .unwrap()
            .to_owned()
    };
    let schedule = "schedule{scheduleKind,minute?,time?,dayOfWeek?}";
    assert!(line(&mono, "mono_habit_add").contains(schedule));
    let update = line(&mono, "mono_habit_update");
    assert!(
        update.contains(&format!(",{schedule}").replacen('{', "?{", 1)),
        "{update}"
    );
    for text in [&session, &mono] {
        assert!(
            text.contains("requestId in the input of an operation that changes something"),
            "{text}"
        );
        assert!(text.contains(r#"{"ok":true,"result":...,"requestId":ID}"#));
        assert!(text.contains(r#""code":CODE,"retryable":BOOL"#));
        assert!(text.contains("--request-id"));
        assert!(text.contains("Credentials are in the environment; never print them."));
    }
}

//! A Mono's own actions, on its memory, soul, habits, artifacts and cards,
//! on the control pipeline: each old action is translated as the `app`
//! command translates it and called as the Mono's session. The assertions
//! are the facts the old bridge's tests pinned; where the answer differs
//! from the bridge's, a comment names the accepted change or fix of the
//! spec (§13, §14). Then what the control pipeline adds: its results fit
//! the published outputs, the panels read the files again after a write
//! (T-M5), and a Mono's turns are told they are a Mono's (T-M17).
use super::tests::setup;
use super::tests_parity_mono::{
    Caller, act, chat, control_call, control_command, habit_run, held_cards, ids, plain_session,
    start, view,
};
use super::*;
use gpui::TestAppContext;
use monocode_control::{Command, ErrorCode, summarize};
use monocode_core::{
    app_mcp,
    mono::{files, memory},
};
use serde_json::{Value, json};

/// A refusal: the bridge's words, or the typed input's (C-1), which name
/// `typed`, when the old check was the bridge's own validator.
fn refused(result: &Result<Value, String>, bridge: &str, typed: Option<&str>) {
    match typed {
        Some(typed) => {
            assert!(
                result.as_ref().is_err_and(|e| e.contains(typed)),
                "C-1: the typed input names {typed}: {result:?}"
            );
        }
        None => assert_eq!(result, &Err(bridge.into())),
    }
}

/// The items of a list result. shape: C-8 (and C-7): the bridge returned
/// the array, an operation returns it under `field`.
fn items(listed: &Value, field: &str) -> Value {
    listed[field].clone()
}

#[gpui::test]
fn a_habit_run_cannot_change_habits_but_can_list_them(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let habit = act(
        &s,
        Caller::Chat,
        "habits.add",
        json!({"name":"Digest","instructions":"Summarize changes","schedule":{"kind":"daily","time":"09:00"}}),
        "add",
        cx,
    )
    .unwrap();
    let run = habit_run(&s, cx);
    let id = habit["id"].clone();
    for (n, (action, input)) in [
        (
            "habits.add",
            json!({"name":"Nested","instructions":"Run again","schedule":{"kind":"hourly"}}),
        ),
        ("habits.update", json!({"id":id,"enabled":false})),
        ("habits.run", json!({"id":id})),
        ("habits.remove", json!({"id":id})),
    ]
    .into_iter()
    .enumerate()
    {
        let result = act(&s, Caller::Run(run), action, input, &format!("run-{n}"), cx);
        assert_eq!(
            result,
            Err("A habit run cannot schedule more runs".into()),
            "{action}"
        );
    }
    // The bridge refused before validation, so a misspelled field got "A
    // habit run cannot schedule more runs"; C-1: the control pipeline reads
    // the input first.
    let typo = act(
        &s,
        Caller::Run(run),
        "habits.add",
        json!({"nmae":"Typo"}),
        "run-typo",
        cx,
    );
    assert_eq!(typo, Err("habits.add does not take nmae".into()));
    let listed = act(&s, Caller::Run(run), "habits.list", json!({}), "list", cx).unwrap();
    assert_eq!(ids(&items(&listed, "habits")), vec![id]);
    let saved = habits::load(&s.root, &s.id).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].enabled && !saved[0].run_requested);
}

#[gpui::test]
fn mono_artifacts_keep_kinds_caps_and_creation_time_and_post_a_card(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("artifact-{n}");
        act(&s, Caller::Chat, action, input, &request, cx)
    };
    let plan = call(
        "artifacts.write",
        json!({"title":"Plan","body":"# Plan\nShip it."}),
        cx,
    )
    .unwrap();
    assert_eq!(plan["kind"], "document");
    assert_eq!(plan["createdAt"], plan["updatedAt"]);
    let id = plan["id"].as_str().unwrap().to_string();
    let card = held_cards(&s, cx)
        .into_iter()
        .find(|c| c["documentId"] == id)
        .expect("writing an artifact posts its card");
    assert_eq!(card["type"], "document");
    assert_eq!(card["title"], "Plan");

    let updated = call(
        "artifacts.write",
        json!({"id":id,"kind":"markdown","title":"Plan v2","body":"# Plan\nShip it twice.","summary":"é".repeat(4001)}),
        cx,
    )
    .unwrap();
    assert_eq!(updated["createdAt"], plan["createdAt"]);
    assert!(updated["updatedAt"].as_u64() >= plan["updatedAt"].as_u64());
    assert_eq!(updated["kind"], "markdown");
    assert_eq!(updated["summary"].as_str().unwrap().chars().count(), 4000);
    let page = call(
        "artifacts.write",
        json!({"kind":"html","title":"Page","body":"<p>Hi</p>"}),
        cx,
    )
    .unwrap();

    let pdf = call(
        "artifacts.write",
        json!({"kind":"pdf","title":"Plan","body":"x"}),
        cx,
    );
    refused(&pdf, "Use document or html", Some("`pdf`"));
    // Titles count bytes: 256 two-byte characters fit, 257 do not.
    assert!(
        call(
            "artifacts.write",
            json!({"title":"é".repeat(256),"body":"x"}),
            cx
        )
        .is_ok()
    );
    for title in ["é".repeat(257), " ".into()] {
        assert_eq!(
            call("artifacts.write", json!({"title":title,"body":"x"}), cx),
            Err("Provide title (up to 512 bytes)".into())
        );
    }
    assert!(
        call(
            "artifacts.write",
            json!({"title":"Largest","body":"b".repeat(512 * 1024)}),
            cx
        )
        .is_ok()
    );
    assert_eq!(
        call(
            "artifacts.write",
            json!({"title":"Too large","body":"b".repeat(512 * 1024 + 1)}),
            cx
        ),
        Err("Provide body (up to 524288 bytes)".into())
    );

    let read = call("artifacts.read", json!({"id":id}), cx).unwrap();
    assert_eq!(read["title"], "Plan v2");
    assert_eq!(read["body"], "# Plan\nShip it twice.");
    assert_eq!(
        call("artifacts.read", json!({"id":"missing"}), cx),
        Err("Document not found".into())
    );
    let documents = s
        .manager
        .read_with(cx, |m, _| m.roster.get(&s.id).unwrap().documents.len());
    assert_eq!(documents, 4);
    let all = call("artifacts.list", json!({}), cx).unwrap();
    let listed = items(&all, "artifacts");
    assert_eq!(listed.as_array().unwrap().len(), 4);
    assert_eq!(ids(&listed)[..2], [json!(id), page["id"].clone()]);
    // C-7: the list drops bodies and counts what there is.
    assert_eq!(all["total"], 4);
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a.get("body").is_none())
    );
    let html = call("artifacts.list", json!({"kind":"html"}), cx).unwrap();
    assert_eq!(ids(&items(&html, "artifacts")), vec![page["id"].clone()]);
    let second = call("artifacts.list", json!({"offset":1,"limit":1}), cx).unwrap();
    assert_eq!(ids(&items(&second, "artifacts")), vec![page["id"].clone()]);
}

#[gpui::test]
fn mono_cards_are_parsed_held_for_the_chat_and_refuse_bad_shapes(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let launch = start(
        &s,
        Caller::Chat,
        "card-child",
        json!({"title":"Card child"}),
        cx,
    );
    let mut n = 0;
    let mut call = |input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("card-{n}");
        act(&s, Caller::Chat, "chat.card", input, &request, cx)
    };

    let choices = call(json!({"type":"choices","options":["Ship it","Wait"]}), cx).unwrap();
    assert_eq!(choices["options"], json!(["Ship it", "Wait"]));
    let session = call(
        json!({"type":"session","sessionId":launch.to_string(),"note":"  Look   here "}),
        cx,
    )
    .unwrap();
    assert_eq!(session["sessionId"], launch);
    assert_eq!(session["title"], "Card child");
    assert_eq!(session["note"], "Look here");
    // F-7: not the bridge's "Saved" but the state the chat's card refresh
    // shows for the session, in the renderer's lowercase words.
    let state = view(&s, launch, cx).read_with(cx, cards::launched_state);
    assert_eq!(session["state"], state);
    assert!(
        ["working", "input", "draft", "failed", "stopped", "done"].contains(&state),
        "{state}"
    );
    let busy_view = view(&s, launch, cx);
    busy_view.update(cx, |v, _| v.busy = true);
    let busy = call(json!({"type":"session","sessionId":launch}), cx).unwrap();
    // F-7: a busy session is "working", as the renderer spells it, not the
    // bridge's "Working".
    assert_eq!(busy["state"], "working");
    busy_view.update(cx, |v, _| v.busy = false);
    let habit = call(
        json!({"type":"habit","name":"Daily digest","instructions":"Summarize","schedule":{"kind":"daily","time":"08:30"}}),
        cx,
    )
    .unwrap();
    assert_eq!(habit["name"], "Daily digest");
    assert_eq!(habit["schedule"]["time"], "08:30");
    let held: Vec<_> = held_cards(&s, cx).iter().map(|c| c["id"].clone()).collect();
    for card in [&choices, &session, &habit] {
        assert!(card["id"].is_string());
        assert!(held.contains(&card["id"]), "{card}");
    }

    for (input, error, typed) in [
        (
            json!({"type":"poll"}),
            "Card type must be pr, session, choices or habit",
            Some("`poll`"),
        ),
        (
            json!({"type":"choices","options":[]}),
            "Choose 1–4 replies",
            None,
        ),
        (
            json!({"type":"choices","options":["a","b","c","d","e"]}),
            "Choose 1–4 replies",
            None,
        ),
        (
            json!({"type":"choices","options":["x".repeat(161)]}),
            "Replies must contain 1–160 characters",
            None,
        ),
        (
            json!({"type":"choices","options":["a"],"note":"Why"}),
            "Unexpected choices card field",
            Some("`note`"),
        ),
        (
            json!({"type":"session"}),
            "Provide sessionId",
            Some("`sessionId`"),
        ),
        (
            json!({"type":"session","sessionId":launch,"note":"n".repeat(281)}),
            "Provide note under 280 characters",
            None,
        ),
        (
            json!({"type":"session","sessionId":s.session}),
            "Session is outside assigned projects",
            None,
        ),
        (
            json!({"type":"habit","name":"n".repeat(81),"instructions":"Go","schedule":{"kind":"daily"}}),
            "Provide name under 80 characters",
            None,
        ),
        (
            json!({"type":"habit","name":"Digest","instructions":"Go","schedule":{"kind":"monthly"}}),
            "Choose hourly, daily, weekdays or weekly, a valid 24-hour time, minute 0–59 and weekday 0–6",
            None,
        ),
        (
            json!({"type":"habit","name":"Digest","instructions":"Go"}),
            "Provide schedule",
            Some("`schedule`"),
        ),
    ] {
        refused(&call(input, cx), error, typed);
    }
}

#[gpui::test]
fn mono_soul_updates_need_the_current_hash(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("soul-{n}");
        act(&s, Caller::Chat, action, input, &request, cx)
    };
    let soul = "# Soul\nBe brief.\n";
    files::save(&s.root, &s.id, None, "SOUL.md", soul, None).unwrap();
    let read = call("soul.read", json!({}), cx).unwrap();
    assert_eq!(read["text"], soul);
    assert_eq!(read["hash"], files::content_hash(soul));

    let next = "# Soul\nBe thorough.\n";
    let updated = call(
        "soul.update",
        json!({"text":next,"expectedHash":read["hash"]}),
        cx,
    )
    .unwrap();
    assert_eq!(updated["hash"], files::content_hash(next));
    let stale = json!({"text":"Lost","expectedHash":read["hash"]});
    assert_eq!(
        call("soul.update", stale.clone(), cx),
        Err("conflict".into())
    );
    // C-1: "conflict" is a request_conflict.
    let conflict = control_call(&s, Caller::Chat, "soul.update", stale, "soul-stale", cx);
    assert_eq!(conflict.unwrap_err().code, ErrorCode::RequestConflict);
    let file = || files::read(&s.root, &s.id, None, "SOUL.md").unwrap().text;
    assert_eq!(file().as_deref(), Some(next));

    assert_eq!(
        call(
            "soul.update",
            json!({"text":"x".repeat(128 * 1024 + 1),"expectedHash":updated["hash"]}),
            cx
        ),
        Err("Provide text".into())
    );
    refused(
        &call("soul.update", json!({"text":"x"}), cx),
        "Provide expectedHash (up to 128 bytes)",
        Some("`expectedHash`"),
    );
    let emptied = call(
        "soul.update",
        json!({"text":"","expectedHash":updated["hash"]}),
        cx,
    )
    .unwrap();
    assert_eq!(emptied["hash"], files::content_hash(""));
    assert_eq!(file().as_deref(), Some(""));
}

#[gpui::test]
fn mono_memory_edits_are_dated_redacted_and_capped(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let today = memory::date();
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("memory-{n}");
        act(&s, Caller::Chat, action, input, &request, cx)
    };
    let saved = || {
        files::read(&s.root, &s.id, None, "MEMORY.md")
            .unwrap()
            .text
            .unwrap_or_default()
    };

    let added = call("memory.add", json!({"fact":"Prefers   short replies"}), cx).unwrap();
    assert_eq!(added["hash"], files::content_hash(&saved()));
    let again = call("memory.add", json!({"fact":"Prefers short replies"}), cx).unwrap();
    assert_eq!(again["hash"], added["hash"]);
    let line = format!("- {today} · Prefers short replies");
    assert_eq!(saved().matches(&line).count(), 1);
    call(
        "memory.add",
        json!({"fact":"Freeze merges","until":"2026-12-31"}),
        cx,
    )
    .unwrap();
    call(
        "memory.add",
        json!({"fact":format!("Deploy with ghp_{}", "a".repeat(24))}),
        cx,
    )
    .unwrap();
    let read = call("memory.read", json!({}), cx).unwrap();
    let text = read["text"].as_str().unwrap();
    assert!(text.contains(&format!("- {today} · Freeze merges · until 2026-12-31")));
    assert!(text.contains(&format!("- {today} · Deploy with «redacted 28 chars»")));
    assert!(!text.contains("ghp_"));
    assert_eq!(read["hash"], files::content_hash(text));

    call(
        "memory.replace",
        json!({"find":"short replies","fact":"Prefers detailed replies"}),
        cx,
    )
    .unwrap();
    let text = saved();
    assert!(text.contains(&format!(
        "- ~~{today} · Prefers short replies~~ · superseded {today}"
    )));
    assert!(text.contains(&format!("- {today} · Prefers detailed replies")));
    call("memory.remove", json!({"find":"Freeze merges"}), cx).unwrap();
    assert!(!saved().contains("Freeze merges"));

    call(
        "memory.add",
        json!({"fact":"Uses pnpm","topic":"Tooling"}),
        cx,
    )
    .unwrap();
    let topic = call("memory.read", json!({"topic":"Tooling"}), cx).unwrap();
    assert!(
        topic["text"]
            .as_str()
            .unwrap()
            .contains(&format!("- {today} · Uses pnpm"))
    );
    assert!(!saved().contains("Uses pnpm"));

    let hits = call("memory.search", json!({"query":"detailed replies"}), cx).unwrap();
    let hits = items(&hits, "hits");
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["file"], "MEMORY.md");
    assert_eq!(hits[0]["date"], today);
    assert!(
        hits[0]["line"]
            .as_str()
            .unwrap()
            .contains("Prefers detailed replies")
    );

    for (action, input, error) in [
        (
            "memory.add",
            json!({"fact":"x".repeat(4001)}),
            "Provide fact (up to 4000 bytes)",
        ),
        (
            "memory.add",
            json!({"fact":"x".repeat(1001)}),
            "A memory fact must contain 1–1000 characters",
        ),
        (
            "memory.add",
            json!({"fact":"Ship","until":"next week"}),
            "until must be a date like 2026-10-31",
        ),
        (
            "memory.add",
            json!({"fact":"Ship","topic":"../escape"}),
            "Use a topic name containing letters, numbers, spaces, dots or dashes",
        ),
        (
            "memory.replace",
            json!({"find":"","fact":"Ship"}),
            "find must not be empty",
        ),
        (
            "memory.remove",
            json!({"find":"·"}),
            "Read memory and quote text matching exactly one live entry",
        ),
        (
            "memory.search",
            json!({"query":"the and"}),
            "Use a few specific words to search memory",
        ),
        (
            "memory.search",
            json!({"query":"x".repeat(4001)}),
            "Provide query (up to 4000 bytes)",
        ),
    ] {
        assert_eq!(
            call(action, input.clone(), cx),
            Err(error.into()),
            "{input}"
        );
    }
}

#[gpui::test]
fn mono_habits_are_added_updated_run_and_removed_within_their_caps(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("habit-{n}");
        act(&s, Caller::Chat, action, input, &request, cx)
    };
    let daily = json!({"kind":"daily","time":"09:00"});
    let habit = call(
        "habits.add",
        json!({"name":"  Morning digest ","instructions":"Summarize changes","schedule":daily}),
        cx,
    )
    .unwrap();
    assert_eq!(habit["name"], "Morning digest");
    assert_eq!(habit["enabled"], true);
    let id = habit["id"].clone();
    let saved = || habits::load(&s.root, &s.id).unwrap();

    let renamed = call(
        "habits.update",
        json!({"id":id,"name":"  Evening digest  ","instructions":"Summarize the day"}),
        cx,
    )
    .unwrap();
    assert_eq!(renamed["name"], "Evening digest");
    assert_eq!(renamed["instructions"], "Summarize the day");
    let weekly = call(
        "habits.update",
        json!({"id":id,"schedule":{"kind":"weekly","time":"10:00","dayOfWeek":3}}),
        cx,
    )
    .unwrap();
    assert_eq!(weekly["schedule"]["scheduleKind"], "weekly");
    assert_eq!(saved()[0].schedule.day_of_week, 3);
    let paused = call("habits.update", json!({"id":id,"enabled":false}), cx).unwrap();
    assert_eq!(paused["enabled"], false);
    assert!(!saved()[0].enabled);
    let requested = call("habits.run", json!({"id":id}), cx).unwrap();
    assert_eq!(requested["runRequested"], true);
    assert!(saved()[0].run_requested);

    for (action, input, error, typed) in [
        (
            "habits.add",
            json!({"name":"Digest","instructions":"Go"}),
            "Provide schedule",
            Some("`schedule`"),
        ),
        (
            "habits.add",
            json!({"name":"Digest","instructions":"Go","schedule":{"kind":"monthly"}}),
            "Choose hourly, daily, weekdays or weekly, a valid 24-hour time, minute 0–59 and weekday 0–6",
            None,
        ),
        (
            "habits.add",
            json!({"name":"n".repeat(481),"instructions":"Go","schedule":daily}),
            "Provide name (up to 480 bytes)",
            None,
        ),
        (
            "habits.add",
            json!({"name":"n".repeat(121),"instructions":"Go","schedule":daily}),
            "A habit needs a name and instructions",
            None,
        ),
        (
            "habits.add",
            json!({"name":"Digest","instructions":"i".repeat(32001),"schedule":daily}),
            "Provide instructions (up to 32000 bytes)",
            None,
        ),
        (
            "habits.update",
            json!({"id":"missing","enabled":true}),
            "Habit not found",
            None,
        ),
        (
            "habits.update",
            json!({"id":id,"name":"   "}),
            "Invalid habit name",
            None,
        ),
        (
            "habits.update",
            json!({"id":id,"name":"n".repeat(121)}),
            "Invalid habit name",
            None,
        ),
        (
            "habits.update",
            json!({"id":id,"instructions":" "}),
            "Invalid habit instructions",
            None,
        ),
        (
            "habits.update",
            json!({"id":id,"schedule":{"kind":"daily","time":"25:00"}}),
            "Choose hourly, daily, weekdays or weekly, a valid 24-hour time, minute 0–59 and weekday 0–6",
            None,
        ),
        (
            "habits.run",
            json!({"id":"missing"}),
            "Habit not found",
            None,
        ),
        (
            "habits.remove",
            json!({"id":"missing"}),
            "Habit not found",
            None,
        ),
    ] {
        refused(&call(action, input, cx), error, typed);
    }
    assert_eq!(saved()[0].name, "Evening digest");

    for i in 1..20 {
        call(
            "habits.add",
            json!({"name":format!("Habit {i}"),"instructions":"Check","schedule":daily}),
            cx,
        )
        .unwrap();
    }
    assert_eq!(
        call(
            "habits.add",
            json!({"name":"One more","instructions":"Check","schedule":daily}),
            cx
        ),
        Err("At most 20 habits per Mono".into())
    );
    assert_eq!(saved().len(), 20);
    assert_eq!(
        call("habits.remove", json!({"id":id}), cx).unwrap()["removed"],
        true
    );
    assert!(saved().iter().all(|h| json!(h.id) != id));
    assert_eq!(saved().len(), 19);
}

#[gpui::test]
fn mono_retries_replay_the_persisted_receipt_after_a_restart(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let input = json!({"title":"Receipt","body":"Written once"});
    let write = |input: Value, cx: &mut TestAppContext| {
        act(&s, Caller::Chat, "artifacts.write", input, "write", cx)
    };
    let first = write(input.clone(), cx).unwrap();
    // The control pipeline keeps its receipts in the store alone, so every
    // retry finds what is left after a restart: the stored receipt.
    let retry = write(input, cx).unwrap();
    assert_eq!(retry, first);
    let documents = s
        .manager
        .read_with(cx, |m, _| m.roster.get(&s.id).unwrap().documents.len());
    assert_eq!(documents, 1);
    let other = json!({"title":"Receipt","body":"Different"});
    // C-5: the control pipeline's receipts, and its code for a conflict, not
    // the bridge's "Request ID was already used with different input".
    let refused = control_call(&s, Caller::Chat, "artifacts.write", other, "write", cx);
    assert_eq!(refused.unwrap_err().code, ErrorCode::RequestConflict);
}

/// A habit run's cards, its artifacts' among them, wait for its report
/// (MO-59): they are held with the run, not posted in the Mono's chat.
#[gpui::test]
fn a_habit_runs_cards_wait_for_its_report(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let run = habit_run(&s, cx);
    let card = json!({"type":"choices","options":["Ship it"]});
    let card = act(&s, Caller::Run(run), "chat.card", card, "run-card", cx).unwrap();
    let report = json!({"title":"Report","body":"All green"});
    let written = act(
        &s,
        Caller::Run(run),
        "artifacts.write",
        report,
        "run-artifact",
        cx,
    )
    .unwrap();
    let held = s
        .manager
        .read_with(cx, |m, _| m.running[&run].cards.clone());
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(held[0]["id"], card["id"]);
    assert_eq!(held[1]["documentId"], written["id"]);
    assert!(
        held_cards(&s, cx).is_empty(),
        "nothing reaches the chat yet"
    );
}

/// Every Mono-only operation's result fits the output it publishes, so a
/// relay's summary and structured content describe it.
#[gpui::test]
fn a_monos_own_results_fit_their_published_outputs(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let launch = start(&s, Caller::Chat, "fit-child", json!({}), cx);
    let soul = files::read(&s.root, &s.id, None, "SOUL.md").unwrap().hash;
    let daily = json!({"scheduleKind":"daily","time":"09:00"});
    let mut habit = String::new();
    let mut artifact = String::new();
    for (operation, input) in [
        ("mono_memory_add", json!({"fact":"Ships on Fridays"})),
        (
            "mono_memory_replace",
            json!({"find":"Fridays","fact":"Ships on Thursdays"}),
        ),
        ("mono_memory_read", json!({})),
        ("mono_memory_search", json!({"query":"ships thursdays"})),
        ("mono_memory_remove", json!({"find":"Thursdays"})),
        ("mono_soul_read", json!({})),
        (
            "mono_soul_update",
            json!({"text":"Be brief.","expectedHash":soul}),
        ),
        (
            "mono_habit_add",
            json!({"name":"Digest","instructions":"Summarize","schedule":daily}),
        ),
        ("mono_habit_list", json!({})),
        ("mono_habit_update", json!({"id":"<habit>","enabled":false})),
        ("mono_habit_run", json!({"id":"<habit>"})),
        ("mono_habit_remove", json!({"id":"<habit>"})),
        ("mono_artifact_write", json!({"title":"Plan","body":"Ship"})),
        ("mono_artifact_list", json!({})),
        ("mono_artifact_read", json!({"id":"<artifact>"})),
        (
            "mono_card_post",
            json!({"type":"choices","options":["Yes"]}),
        ),
        (
            "mono_card_post",
            json!({"type":"session","sessionId":launch}),
        ),
        (
            "mono_card_post",
            json!({"type":"habit","name":"Digest","instructions":"Go","schedule":daily}),
        ),
    ] {
        let input = input
            .to_string()
            .replace("<habit>", &habit)
            .replace("<artifact>", &artifact);
        let wire = json!({"operation": operation, "input": serde_json::from_str::<Value>(&input).unwrap()});
        let command: Command = serde_json::from_value(wire).unwrap();
        let result = control_command(&s, Caller::Chat, command, None, cx)
            .unwrap_or_else(|e| panic!("{operation}: {e:?}"));
        assert!(
            summarize(operation, &result).is_some(),
            "{operation}: {result}"
        );
        match operation {
            "mono_habit_add" => habit = result["id"].as_str().unwrap().to_owned(),
            "mono_artifact_write" => artifact = result["id"].as_str().unwrap().to_owned(),
            _ => {}
        }
    }
}

/// T-M5: a write through a Mono's own operations reads its files again, so
/// its panels show the new memory, soul and habits (MO-18).
#[gpui::test]
fn the_panels_read_a_monos_files_again_after_its_writes(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let shown = |cx: &mut TestAppContext| {
        s.manager.read_with(cx, |m, _| {
            let files = m.files.as_ref().expect("the open Mono's files are loaded");
            (
                files.memory.clone(),
                files.soul.clone(),
                m.habits.iter().map(|h| h.name.clone()).collect::<Vec<_>>(),
            )
        })
    };
    let (memory, _, habits) = shown(cx);
    assert!(!memory.contains("Prefers short replies") && habits.is_empty());
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("reload-{n}");
        act(&s, Caller::Chat, action, input, &request, cx).unwrap()
    };
    call("memory.add", json!({"fact":"Prefers short replies"}), cx);
    assert!(shown(cx).0.contains("Prefers short replies"), "memory");
    let hash = call("soul.read", json!({}), cx)["hash"].clone();
    call(
        "soul.update",
        json!({"text":"# Soul\nBe kind.\n","expectedHash":hash}),
        cx,
    );
    assert_eq!(shown(cx).1.as_deref(), Some("# Soul\nBe kind.\n"), "soul");
    call(
        "habits.add",
        json!({"name":"Digest","instructions":"Summarize","schedule":{"kind":"daily"}}),
        cx,
    );
    assert_eq!(shown(cx).2, ["Digest"], "habits");
}

/// T-M17: a Mono's main chat and its habit runs get the relay as a Mono's
/// (`MONOCODE_CONTROL_ROLE=mono`, from the Mono their settings carry); a
/// project session gets it as a session's.
#[gpui::test]
fn a_monos_chat_and_habit_runs_get_the_relay_as_a_monos(cx: &mut TestAppContext) {
    let s = setup(cx);
    let main = chat(&s, cx);
    let run = habit_run(&s, cx);
    // As `start_habit` configures a run: the Mono's context on its settings.
    s.manager.update(cx, |m, cx| {
        let mono = m.roster.get(&s.id).unwrap().clone();
        let root = m.root.clone();
        m.running[&run].view.update(cx, |v, cx| {
            v.configure_mono(mono, root, cx);
        });
    });
    let run = s.manager.read_with(cx, |m, _| m.running[&run].view.clone());
    let plain = view(&s, plain_session(&s, cx), cx);
    let store = s.store.clone();
    let relay = |chat: &Entity<Conversation>, cx: &mut TestAppContext| {
        chat.read_with(cx, |v, _| {
            let db = locked(&store).unwrap();
            crate::runtime::app_control::for_turn(&db, v.summary.id, &v.summary.cwd, &v.settings)
        })
    };
    for (chat, role) in [(&main, "mono"), (&run, "mono"), (&plain, "session")] {
        let environment = relay(chat, cx);
        let id = chat.read_with(cx, |v, _| v.summary.id);
        assert_eq!(
            environment.get(app_mcp::ENV_SESSION),
            Some(&id.to_string()),
            "{role}: the relay speaks for its own session"
        );
        assert_eq!(environment[app_mcp::ENV_ROLE], role);
    }
}

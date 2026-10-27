//! The operations only a Mono calls, on its own memory, soul, habits,
//! artifacts and chat: which they are, what they need, and the card input,
//! a tagged enum published as one object.
use monocode_control::{
    Command, Scope, catalog,
    ops::{
        mono_card::MonoCardPostInput,
        mono_habit::{HabitSchedule, MonoHabitAddInput, MonoHabitUpdateInput},
    },
    summarize,
};
use serde_json::{Value, json};

/// Every Mono-only operation, in registry order.
const MONO_ONLY: [&str; 16] = [
    "mono_memory_read",
    "mono_memory_search",
    "mono_memory_add",
    "mono_memory_replace",
    "mono_memory_remove",
    "mono_soul_read",
    "mono_soul_update",
    "mono_habit_list",
    "mono_habit_add",
    "mono_habit_update",
    "mono_habit_run",
    "mono_habit_remove",
    "mono_artifact_list",
    "mono_artifact_read",
    "mono_artifact_write",
    "mono_card_post",
];

fn card(input: Value) -> Result<MonoCardPostInput, serde_json::Error> {
    serde_json::from_value(input)
}

#[test]
fn only_a_monos_own_operations_are_mono_only() {
    let mono_only: Vec<_> = catalog()
        .into_iter()
        .filter(|d| d.mono_only)
        .map(|d| d.name)
        .collect();
    assert_eq!(mono_only, MONO_ONLY);
    let command = |operation: &str, input: Value| -> Command {
        serde_json::from_value(json!({"operation": operation, "input": input})).unwrap()
    };
    assert!(command("mono_soul_read", json!({})).mono_only());
    for (operation, input) in [
        ("mono_list", json!({})),
        ("mono_send", json!({"monoId": "m1", "text": "Hi"})),
        ("notes_write", json!({"title": "T"})),
    ] {
        assert!(!command(operation, input).mono_only(), "{operation}");
    }
}

#[test]
fn adding_rescheduling_or_turning_a_habit_on_also_needs_dispatch() {
    let daily = HabitSchedule {
        schedule_kind: "daily".into(),
        ..HabitSchedule::default()
    };
    let add = Command::MonoHabitAdd(MonoHabitAddInput {
        name: "Digest".into(),
        instructions: "Summarize".into(),
        schedule: daily.clone(),
    });
    assert_eq!(add.scopes(), [Scope::Manage, Scope::Dispatch]);
    let update = |input| Command::MonoHabitUpdate(input).scopes();
    let rename = MonoHabitUpdateInput {
        id: "h1".into(),
        name: Some("Evening digest".into()),
        ..MonoHabitUpdateInput::default()
    };
    assert_eq!(update(rename.clone()), [Scope::Manage]);
    let pause = MonoHabitUpdateInput {
        enabled: Some(false),
        ..rename.clone()
    };
    assert_eq!(update(pause), [Scope::Manage], "pausing schedules nothing");
    for input in [
        MonoHabitUpdateInput {
            enabled: Some(true),
            ..rename.clone()
        },
        MonoHabitUpdateInput {
            schedule: Some(daily),
            ..rename
        },
    ] {
        assert_eq!(update(input), [Scope::Manage, Scope::Dispatch]);
    }
}

#[test]
fn a_card_is_one_object_tagged_by_type_and_each_type_takes_only_its_fields() {
    let entries = catalog();
    let post = entries.iter().find(|d| d.name == "mono_card_post").unwrap();
    let schema = Value::Object(post.input_schema.clone());
    assert_eq!(schema["type"], "object");
    assert!(schema.get("oneOf").is_none(), "no oneOf at the root");
    assert_eq!(schema["required"], json!(["type"]));
    assert_eq!(
        schema["properties"]["type"]["enum"],
        json!(["pr", "session", "choices", "habit"])
    );
    let mut fields: Vec<_> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "instructions",
            "name",
            "note",
            "number",
            "options",
            "repo",
            "requestId",
            "schedule",
            "sessionId",
            "type"
        ]
    );

    assert_eq!(
        card(json!({"type":"session","sessionId":12,"note":"Look"})).unwrap(),
        MonoCardPostInput::Session {
            session_id: 12,
            note: Some("Look".into())
        }
    );
    let habit = card(json!({"type":"habit","name":"Digest","instructions":"Go","schedule":{"kind":"weekly","dayOfWeek":3}})).unwrap();
    let MonoCardPostInput::Habit { schedule, .. } = &habit else {
        unreachable!("a habit card")
    };
    assert_eq!(
        (schedule.schedule_kind.as_str(), schedule.day_of_week),
        ("weekly", Some(3)),
        "a schedule's kind is also accepted as kind"
    );
    let wire = serde_json::to_value(&habit).unwrap();
    assert_eq!(wire["type"], "habit");
    assert_eq!(wire["schedule"]["scheduleKind"], "weekly");
    for refused in [
        json!({"type":"choices","options":["a"],"note":"Why"}),
        json!({"type":"session"}),
        json!({"type":"pr","number":1,"sessionId":2}),
        json!({"type":"poll"}),
        json!({"options":["a"]}),
    ] {
        assert!(card(refused.clone()).is_err(), "{refused}");
    }
}

#[test]
fn posted_cards_and_habits_read_as_their_outputs() {
    let session = json!({"id":"c1","type":"session","sessionId":4,"title":"Fix","state":"working"});
    assert_eq!(
        summarize("mono_card_post", &session).as_deref(),
        Some("Posted a session card")
    );
    let pull = json!({"id":"c2","type":"pr","number":7,"repo":null,"title":"Fix","state":"OPEN","url":"https://github.com/o/r/pull/7","isDraft":false,"additions":3,"deletions":1});
    assert_eq!(
        summarize("mono_card_post", &pull).as_deref(),
        Some("Posted a pull request card")
    );
    let habit = json!({"id":"h1","name":"Digest","instructions":"Summarize","schedule":{"scheduleKind":"daily","minute":0,"time":"09:00","dayOfWeek":1},"enabled":true,"createdAt":1,"nextRunAt":2,"lastRunAt":null,"lastOutcome":null,"lastError":null,"runs":[],"runRequested":false,"runningSince":null});
    assert_eq!(
        summarize("mono_habit_add", &habit).as_deref(),
        Some("Added habit “Digest”")
    );
    let soul = json!({"text":null,"hash":"e3b0"});
    assert_eq!(
        summarize("mono_soul_read", &soul).as_deref(),
        Some("No soul yet")
    );
}

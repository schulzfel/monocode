//! Titles and captions: every operation has a human title, its subject
//! fields exist, and captions stay one short line.
use monocode_control::{
    APP_LAUNCH, APP_LAUNCH_TITLE, Caption, SET_FIELDS, caption, caption::humanize, catalog,
};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Transcript actions that render as file links or search pills; a tool
/// title must not be mistaken for one.
const BUILT_IN_ACTIONS: &[&str] = &[
    "Read", "Write", "Edit", "Find", "List", "Search", "Run", "Agent", "Task", "Tasks",
];

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn shown(tool: &str, input: &Value) -> (&'static str, Option<String>) {
        let Caption { title, subject } = caption(tool, input).unwrap();
        (title, subject)
    }
}
use support::shown;

#[test]
fn every_operation_has_a_unique_sentence_case_title() {
    let mut titles = HashSet::new();
    for descriptor in catalog() {
        let title = descriptor.title;
        assert!(!title.trim().is_empty(), "{} has no title", descriptor.name);
        assert!(
            title.starts_with(|c: char| c.is_uppercase()) && !title.ends_with('.'),
            "{title:?} is not sentence case"
        );
        assert!(!title.contains('_'), "{title:?} reads like a tool name");
        assert!(
            !BUILT_IN_ACTIONS.contains(&title),
            "{title:?} is a built-in"
        );
        assert!(titles.insert(title), "{title:?} is used twice");
    }
    assert!(!titles.contains(APP_LAUNCH_TITLE));
}

#[test]
fn subject_fields_are_input_fields() {
    for descriptor in catalog() {
        if descriptor.subject == [SET_FIELDS] {
            continue;
        }
        let properties = descriptor.input_schema["properties"].as_object();
        for field in descriptor.subject {
            assert!(
                properties.is_some_and(|p| p.contains_key(*field)),
                "{}: {field} is not an input field",
                descriptor.name
            );
        }
        let unique: HashSet<_> = descriptor.subject.iter().collect();
        assert_eq!(
            unique.len(),
            descriptor.subject.len(),
            "{}",
            descriptor.name
        );
        assert!(
            !descriptor.subject.contains(&SET_FIELDS),
            "{}",
            descriptor.name
        );
    }
}

#[test]
fn captions_name_the_title_and_a_short_subject() {
    let project = json!({"project": "/Users/x/github.com/repost-sh/monorepo"});
    assert_eq!(
        shown("project_delete", &project),
        ("Remove project", Some("repost-sh/monorepo".into()))
    );
    assert_eq!(
        shown("project_add", &json!({"path": "/work/acme/app/"})),
        ("Add project", Some("acme/app".into()))
    );
    let prompt = format!("\n  Fix {}\nand the docs", "the flaky test ".repeat(8));
    let (title, subject) = shown("session_start", &json!({"prompt": prompt}));
    assert_eq!(title, "Start session");
    let subject = subject.unwrap();
    assert!(subject.starts_with("Fix the flaky test") && subject.ends_with('…'));
    assert!((50..=60).contains(&subject.chars().count()), "{subject}");
    assert_eq!(
        shown("session_wait", &json!({"sessionId": 7, "timeoutMs": 1000})),
        ("Wait for session", Some("#7".into()))
    );
    assert_eq!(
        shown(
            "session_rename",
            &json!({"sessionId": 7, "title": "Docs pass"})
        ),
        ("Rename session", Some("Docs pass".into()))
    );
    assert_eq!(
        shown("tab_open", &json!({"view": "changes"})),
        ("Open tab", Some("changes".into()))
    );
    assert_eq!(shown("model_list", &json!({})), ("List models", None));
    assert_eq!(shown("git_push", &json!({})), ("Push", None));
}

#[test]
fn lists_flags_and_changed_settings_read_as_words() {
    let paths = json!({"paths": ["src/a.rs", "src/b.rs", "docs/c.md", "d.md", "e.md"]});
    assert_eq!(
        shown("git_stage", &paths),
        ("Stage", Some("src/a.rs, src/b.rs, docs/c.md +2".into()))
    );
    assert_eq!(
        shown("git_discard", &json!({"all": true})),
        ("Discard changes", Some("all".into()))
    );
    let settings =
        json!({"theme": "dark", "uiScale": 110, "showRemainingUsage": null, "requestId": "r1"});
    assert_eq!(
        shown("settings_update", &settings),
        ("Update settings", Some("theme, ui scale".into()))
    );
    assert_eq!(
        shown("settings_update", &json!({})),
        ("Update settings", None)
    );
}

#[test]
fn the_relay_tool_has_a_title_and_unknown_tools_have_none() {
    assert_eq!(shown(APP_LAUNCH, &json!({})), ("Launch MonoCode", None));
    assert_eq!(caption("create_issue", &json!({})), None);
    assert_eq!(humanize("create_issue"), "Create issue");
    assert_eq!(humanize("createIssue"), "Create issue");
    assert_eq!(humanize("fetch_URL"), "Fetch URL");
}

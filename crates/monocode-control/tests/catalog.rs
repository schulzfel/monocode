//! The registry, catalog and schemas: every operation's scope, annotations,
//! whether the person confirms it, the session it targets and the rules
//! that come with that, checked against one table and one sample call each.
#[cfg(test)]
pub mod common;

use common::samples::commands;
use monocode_control::{
    CONFIRM_TIMEOUT, Command, Hints, Operation, Scope, catalog, hints_of,
    ops::{
        app::{AppInfo, AppInfoOutput, DataMode},
        automation::AutomationUpsertInput,
        model::{ModelList, ModelListOutput, ModelSummary, ProviderModels},
        notes::NotesWriteInput,
        project::{ProjectList, ProjectListOutput},
        queue::QueueEditInput,
    },
    summarize, valid_tool_name,
};
use serde_json::{Value, json};
use std::time::Duration;

/// Every operation in registry order: its scope, annotations and whether
/// the person confirms each call.
const OPERATIONS: [(&str, Scope, Hints, bool); 81] = [
    ("app_info", Scope::Read, Hints::READ, false),
    ("model_list", Scope::Read, Hints::READ, false),
    ("project_list", Scope::Read, Hints::READ, false),
    ("project_select", Scope::Navigate, Hints::IDEMPOTENT, false),
    ("project_add", Scope::Manage, Hints::IDEMPOTENT, false),
    ("workspace_read", Scope::Read, Hints::READ, false),
    ("session_list", Scope::Read, Hints::READ, false),
    ("session_read", Scope::Read, Hints::READ, false),
    ("session_status", Scope::Read, Hints::READ, false),
    ("session_start", Scope::Dispatch, Hints::CHANGE, false),
    ("session_send", Scope::Dispatch, Hints::CHANGE, false),
    ("session_wait", Scope::Dispatch, Hints::READ, false),
    ("session_stop", Scope::Dispatch, Hints::IDEMPOTENT, false),
    ("tab_open", Scope::Navigate, Hints::CHANGE, false),
    ("tab_focus", Scope::Navigate, Hints::IDEMPOTENT, false),
    ("tab_close", Scope::Navigate, Hints::IDEMPOTENT, false),
    (
        "session_configure",
        Scope::Dispatch,
        Hints::IDEMPOTENT,
        false,
    ),
    ("session_draft", Scope::Dispatch, Hints::IDEMPOTENT, false),
    ("session_rename", Scope::Manage, Hints::IDEMPOTENT, false),
    ("session_archive", Scope::Manage, Hints::DISCARD, false),
    ("session_restore", Scope::Manage, Hints::IDEMPOTENT, false),
    ("folder_list", Scope::Manage, Hints::READ, false),
    ("session_move", Scope::Manage, Hints::CHANGE, false),
    ("question_list", Scope::Read, Hints::READ, false),
    ("question_answer", Scope::Dispatch, Hints::CHANGE, false),
    ("queue_list", Scope::Read, Hints::READ, false),
    ("queue_remove", Scope::Destructive, Hints::DISCARD, true),
    ("queue_steer", Scope::Dispatch, Hints::CHANGE, false),
    ("queue_resume", Scope::Dispatch, Hints::IDEMPOTENT, false),
    ("queue_edit", Scope::Dispatch, Hints::DISCARD, false),
    ("worktree_list", Scope::Manage, Hints::READ, false),
    ("worktree_create", Scope::Manage, Hints::CHANGE, false),
    ("project_update", Scope::Manage, Hints::IDEMPOTENT, false),
    ("project_archive", Scope::Manage, Hints::IDEMPOTENT, false),
    ("project_unarchive", Scope::Manage, Hints::IDEMPOTENT, false),
    ("project_reorder", Scope::Manage, Hints::IDEMPOTENT, false),
    ("tab_move", Scope::Navigate, Hints::CHANGE, false),
    ("pane_split", Scope::Navigate, Hints::CHANGE, false),
    ("layout_set", Scope::Navigate, Hints::IDEMPOTENT, false),
    ("settings_read", Scope::Settings, Hints::READ, false),
    ("settings_update", Scope::Settings, Hints::IDEMPOTENT, false),
    ("search_sessions", Scope::Read, Hints::READ, false),
    ("search_project", Scope::Read, Hints::READ, false),
    ("notes_list", Scope::Read, Hints::READ, false),
    ("notes_read", Scope::Read, Hints::READ, false),
    ("notes_write", Scope::Manage, Hints::CHANGE, false),
    ("changes_read", Scope::Read, Hints::READ, false),
    ("session_compact", Scope::Dispatch, Hints::CHANGE, false),
    ("session_handoff", Scope::Dispatch, Hints::CHANGE, false),
    ("session_delete", Scope::Destructive, Hints::DISCARD, true),
    ("project_delete", Scope::Destructive, Hints::DISCARD, true),
    ("git_stage", Scope::Vcs, Hints::IDEMPOTENT, false),
    ("git_unstage", Scope::Vcs, Hints::IDEMPOTENT, false),
    ("git_commit", Scope::Vcs, Hints::CHANGE, false),
    ("git_discard", Scope::Vcs, Hints::DISCARD, true),
    ("git_push", Scope::Vcs, Hints::DISCARD, true),
    ("automation_list", Scope::Read, Hints::READ, false),
    ("automation_read", Scope::Read, Hints::READ, false),
    ("automation_run", Scope::Dispatch, Hints::CHANGE, false),
    (
        "automation_set_enabled",
        Scope::Manage,
        Hints::IDEMPOTENT,
        false,
    ),
    ("automation_upsert", Scope::Manage, Hints::CHANGE, false),
    (
        "automation_delete",
        Scope::Destructive,
        Hints::DISCARD,
        true,
    ),
    ("mono_list", Scope::Read, Hints::READ, false),
    ("mono_open", Scope::Navigate, Hints::IDEMPOTENT, false),
    ("mono_send", Scope::Dispatch, Hints::CHANGE, false),
    ("mono_memory_read", Scope::Read, Hints::READ, false),
    ("mono_memory_search", Scope::Read, Hints::READ, false),
    ("mono_memory_add", Scope::Manage, Hints::CHANGE, false),
    ("mono_memory_replace", Scope::Manage, Hints::CHANGE, false),
    ("mono_memory_remove", Scope::Manage, Hints::DISCARD, false),
    ("mono_soul_read", Scope::Read, Hints::READ, false),
    ("mono_soul_update", Scope::Manage, Hints::IDEMPOTENT, false),
    ("mono_habit_list", Scope::Read, Hints::READ, false),
    ("mono_habit_add", Scope::Manage, Hints::CHANGE, false),
    ("mono_habit_update", Scope::Manage, Hints::IDEMPOTENT, false),
    ("mono_habit_run", Scope::Dispatch, Hints::CHANGE, false),
    ("mono_habit_remove", Scope::Manage, Hints::DISCARD, false),
    ("mono_artifact_list", Scope::Read, Hints::READ, false),
    ("mono_artifact_read", Scope::Read, Hints::READ, false),
    ("mono_artifact_write", Scope::Manage, Hints::CHANGE, false),
    ("mono_card_post", Scope::Dispatch, Hints::CHANGE, false),
];

#[test]
fn the_catalog_lists_every_operation_once_with_its_scope_annotations_and_confirmation() {
    let entries = catalog();
    let names: Vec<_> = entries.iter().map(|d| d.name).collect();
    let expected: Vec<_> = OPERATIONS.iter().map(|(name, ..)| *name).collect();
    assert_eq!(names, expected);
    assert_eq!(catalog(), catalog(), "the catalog is deterministic");
    assert_eq!(AppInfo::NAME, "app_info");
    assert_eq!(ModelList::NAME, "model_list");
    assert_eq!(ProjectList::NAME, "project_list");
    for (descriptor, (name, scope, hints, asks)) in entries.iter().zip(OPERATIONS) {
        assert!(valid_tool_name(descriptor.name), "{name}");
        assert_ne!(descriptor.description, "", "{name}");
        assert_eq!(descriptor.scope, scope, "{name}");
        assert_eq!(descriptor.hints, hints, "{name}");
        assert_eq!(hints_of(name), Some(hints), "{name}");
        assert_eq!(descriptor.confirmed, asks, "{name}");
    }
    assert_eq!(hints_of("nope"), None);
    let destructive: Vec<_> = entries
        .iter()
        .filter(|d| d.hints.destructive)
        .map(|d| (d.name, d.scope))
        .collect();
    assert_eq!(
        destructive,
        [
            ("session_archive", Scope::Manage),
            ("queue_remove", Scope::Destructive),
            ("queue_edit", Scope::Dispatch),
            ("session_delete", Scope::Destructive),
            ("project_delete", Scope::Destructive),
            ("git_discard", Scope::Vcs),
            ("git_push", Scope::Vcs),
            ("automation_delete", Scope::Destructive),
            ("mono_memory_remove", Scope::Manage),
            ("mono_habit_remove", Scope::Manage),
        ],
        "archiving closes a pane, removing or editing a queued message discards the person's text, and a Mono may forget its own entries and habits"
    );
    assert!(
        entries.iter().all(|d| !d.name.contains("approv")),
        "approvals have no tool, by design"
    );
    // Left out on purpose: orchestration start, app screenshots and
    // commit-message generation.
    for name in ["orchestration", "screenshot", "generate", "message_suggest"] {
        assert!(entries.iter().all(|d| !d.name.contains(name)), "{name}");
    }
}

/// Exactly the `destructive` tools and the `vcs` tools that discard or
/// publish are confirmed, and their descriptions say so.
#[test]
fn deleting_discarding_and_publishing_are_confirmed_in_the_app() {
    for descriptor in catalog() {
        let expected = descriptor.scope == Scope::Destructive
            || (descriptor.scope == Scope::Vcs && descriptor.hints.destructive);
        assert_eq!(descriptor.confirmed, expected, "{}", descriptor.name);
        if descriptor.confirmed {
            assert!(
                descriptor.description.contains("confirms in MonoCode"),
                "{} says it is confirmed",
                descriptor.name
            );
        }
    }
    let edit = Command::QueueEdit(QueueEditInput {
        session_id: 3,
        message_id: "m1".into(),
        text: "Shorter".into(),
    });
    assert!(!edit.confirmed(), "queue_edit is not asked");
    assert_eq!(CONFIRM_TIMEOUT, Duration::from_secs(120));
    let entries = catalog();
    let project_delete = entries.iter().find(|d| d.name == "project_delete").unwrap();
    assert!(
        project_delete
            .description
            .contains("Files on disk are never deleted")
    );
}

#[test]
fn every_command_round_trips_under_its_operation_name() {
    let entries = catalog();
    let commands = commands();
    assert_eq!(
        commands.len(),
        OPERATIONS.len(),
        "every operation is exercised"
    );
    for (command, (name, scope, hints, asks)) in commands.into_iter().zip(OPERATIONS) {
        assert_eq!(command.name(), name);
        assert_eq!(command.scope(), scope, "{name}");
        assert_eq!(command.hints(), hints, "{name}");
        assert_eq!(command.confirmed(), asks, "{name}");
        let wire = serde_json::to_value(&command).unwrap();
        assert_eq!(wire["operation"], name);
        assert!(wire["input"].is_object(), "{name}");
        let back: Command = serde_json::from_value(wire).unwrap();
        assert_eq!(back, command);
        let entry = entries.iter().find(|d| d.name == name).unwrap();
        let input = Value::Object(entry.input_schema.clone());
        assert_eq!(input["additionalProperties"], false, "{name}");
        assert_eq!(
            input["properties"].get("requestId").is_some(),
            !entry.hints.read_only,
            "{name}"
        );
    }
}

/// A call that names a session in `sessionId` targets it, and only the
/// declared operations refuse a session acting on itself or need the
/// session to be local.
#[test]
fn session_targets_and_their_rules_are_declared_with_each_operation() {
    let entries = catalog();
    let mut not_on_self = Vec::new();
    let mut local_only = Vec::new();
    for command in commands() {
        let name = command.name();
        let wire = serde_json::to_value(&command).unwrap();
        let named = wire["input"]["sessionId"].as_i64();
        assert_eq!(command.target_session(), named, "{name}");
        let entry = entries.iter().find(|d| d.name == name).unwrap();
        let takes_session = entry.input_schema["properties"].get("sessionId").is_some();
        if command.not_on_self() {
            assert!(takes_session, "{name} refuses itself without a target");
            not_on_self.push(name);
        }
        if command.local_only() {
            assert!(takes_session, "{name} needs a target to be local");
            local_only.push(name);
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
        ],
        "a session may not message, stop, wait on, reconfigure, draft into, archive or delete itself"
    );
    assert_eq!(
        local_only,
        [
            "session_send",
            "session_stop",
            "tab_open",
            "session_configure",
            "session_draft",
            "session_rename",
            "session_archive",
            "session_restore",
            "session_move",
            "question_answer",
            "queue_remove",
            "queue_steer",
            "queue_resume",
            "queue_edit",
            "session_compact",
            "session_handoff",
            "session_delete"
        ],
        "remote sessions are read-only to clients"
    );
}

/// The operations whose absent `project` means the current project, which
/// the app fills in with an agent session's own project; an update by id
/// keeps the project it has.
#[test]
fn project_defaults_are_declared_with_each_operation() {
    let mut defaulted = Vec::new();
    for mut command in commands() {
        let name = command.name();
        if let Some(slot) = command.project_slot() {
            *slot = Some("/work/monocode".into());
            defaulted.push(name);
            let wire = serde_json::to_value(&command).unwrap();
            assert_eq!(wire["input"]["project"], "/work/monocode", "{name}");
        }
    }
    assert_eq!(
        defaulted,
        [
            "session_list",
            "session_start",
            "folder_list",
            "worktree_list",
            "worktree_create",
            "search_project",
            "notes_write",
            "git_stage",
            "git_unstage",
            "git_commit",
            "git_discard",
            "git_push",
            "automation_upsert"
        ]
    );
    let mut update = Command::NotesWrite(NotesWriteInput {
        id: Some("n1".into()),
        ..NotesWriteInput::default()
    });
    assert!(update.project_slot().is_none(), "a note keeps its project");
    let mut change = Command::AutomationUpsert(AutomationUpsertInput {
        automation_id: Some("a1".into()),
        ..AutomationUpsertInput::default()
    });
    assert!(change.project_slot().is_none(), "so does an automation");
}

#[test]
fn input_schemas_are_closed_objects_without_a_wrapper_title() {
    for descriptor in catalog() {
        let input = Value::Object(descriptor.input_schema.clone());
        assert_eq!(input["type"], "object", "{}", descriptor.name);
        assert_eq!(input["additionalProperties"], false, "{}", descriptor.name);
        assert!(input.get("title").is_none() && input.get("description").is_none());
        assert_eq!(
            input["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        let output = Value::Object(descriptor.output_schema.clone());
        assert_eq!(output["type"], "object", "{}", descriptor.name);
        assert!(output.get("title").is_none());
    }
}

#[test]
fn schema_snapshots_name_the_published_fields() {
    let field_names = |schema: &serde_json::Map<String, Value>| {
        let mut names: Vec<_> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    };
    let entries = catalog();
    let entry = |name| entries.iter().find(|d| d.name == name).unwrap();
    assert_eq!(
        field_names(&entry("app_info").input_schema),
        Vec::<String>::new()
    );
    assert_eq!(
        field_names(&entry("app_info").output_schema),
        [
            "appVersion",
            "controlEnabled",
            "dataMode",
            "platform",
            "protocolVersion",
            "scopes"
        ]
    );
    assert_eq!(
        field_names(&entry("model_list").input_schema),
        ["limit", "provider", "search"]
    );
    assert_eq!(
        field_names(&entry("model_list").output_schema),
        ["accessModes", "providers"]
    );
    let models = serde_json::to_string(&entry("model_list").output_schema).unwrap();
    for field in ["settings", "defaultValue", "options", "value", "label"] {
        assert!(models.contains(&format!("\"{field}\"")), "{field}");
    }
    assert_eq!(
        field_names(&entry("project_list").input_schema),
        ["cursor", "limit"]
    );
    assert_eq!(
        field_names(&entry("project_list").output_schema),
        ["nextCursor", "projects", "total"]
    );
    let projects = serde_json::to_string(&entry("project_list").output_schema).unwrap();
    for field in [
        "path", "key", "name", "color", "current", "archived", "location",
    ] {
        assert!(projects.contains(&format!("\"{field}\"")), "{field}");
    }
}

#[test]
fn unknown_arguments_and_operations_are_rejected() {
    let unknown_field = json!({"operation": "project_list", "input": {"page": 2}});
    assert!(serde_json::from_value::<Command>(unknown_field).is_err());
    let unknown_operation = json!({"operation": "project_delete", "input": {}});
    assert!(serde_json::from_value::<Command>(unknown_operation).is_err());
}

#[test]
fn summaries_are_one_line_and_check_the_output_type() {
    let app = AppInfoOutput {
        app_version: "0.16.0".into(),
        protocol_version: 1,
        data_mode: DataMode::Dev,
        platform: "macos".into(),
        scopes: vec![Scope::Read, Scope::Navigate],
        control_enabled: true,
    };
    let text = summarize("app_info", &serde_json::to_value(&app).unwrap()).unwrap();
    assert_eq!(
        text,
        "MonoCode 0.16.0 on macos (development data); scopes: read, navigate"
    );
    let models = ModelListOutput {
        providers: vec![ProviderModels {
            id: "claude".into(),
            label: "Claude Code".into(),
            available: true,
            models: vec![ModelSummary {
                id: "sonnet".into(),
                name: "Sonnet".into(),
                settings: Vec::new(),
            }],
            omitted: 3,
            error: None,
        }],
        access_modes: Vec::new(),
    };
    let text = summarize("model_list", &serde_json::to_value(&models).unwrap()).unwrap();
    assert_eq!(
        text,
        "1 providers (1 available), 1 models listed, 3 omitted"
    );
    let projects = ProjectListOutput {
        projects: Vec::new(),
        total: 0,
        next_cursor: None,
    };
    let text = summarize("project_list", &serde_json::to_value(&projects).unwrap()).unwrap();
    assert_eq!(text, "0 of 0 projects");
    assert!(!text.contains('\n'));
    assert_eq!(summarize("project_list", &json!({"bogus": true})), None);
    assert_eq!(summarize("nope", &json!({})), None);
}

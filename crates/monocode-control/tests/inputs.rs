//! What the catalog's inputs accept and what policies grant: the closed
//! settings allowlist, closed objects, the scopes scheduling a prompt needs,
//! and the `vcs` scope's place in old and new policies.
use monocode_control::{
    Command, Policy, Scope, catalog,
    ops::{
        automation::{AutomationSetEnabledInput, AutomationUpsertInput, Schedule},
        session::SessionInput,
    },
};
use serde_json::{Value, json};

/// Every property name anywhere in `schema`, including nested definitions.
fn property_names(schema: &Value, out: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                out.extend(properties.keys().cloned());
            }
            for value in map.values() {
                property_names(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                property_names(item, out);
            }
        }
        _ => {}
    }
}

/// Proposal 7.4: never readable or writable. Checked as substrings of every
/// property name, in any case.
const NEVER: [&str; 20] = [
    "binary",
    "path",
    "hook",
    "access",
    "skill",
    "keybinding",
    "action",
    "mask",
    "email",
    "account",
    "mcp",
    "mobile",
    "remote",
    "router",
    "update",
    "inbox",
    "automation",
    "terminal",
    "ceiling",
    "control",
];

#[test]
fn the_settings_allowlist_never_names_a_forbidden_key() {
    let entries = catalog();
    for name in ["settings_read", "settings_update"] {
        let entry = entries.iter().find(|d| d.name == name).unwrap();
        let mut names = Vec::new();
        property_names(&Value::Object(entry.input_schema.clone()), &mut names);
        property_names(&Value::Object(entry.output_schema.clone()), &mut names);
        for property in &names {
            let lower = property.to_lowercase();
            for word in NEVER {
                assert!(
                    !lower.contains(word),
                    "{name} exposes {property}, which contains {word}"
                );
            }
        }
        names.retain(|n| n != "requestId");
        names.sort();
        names.dedup();
        assert_eq!(
            names,
            [
                "accentColor",
                "alerts",
                "autosave",
                "changed",
                "defaultModels",
                "defaultProvider",
                "diffView",
                "favoriteModels",
                "files",
                "followUp",
                "formatOnSave",
                "model",
                "notifications",
                "provider",
                "settings",
                "showRemainingUsage",
                "sounds",
                "tabAnimations",
                "theme",
                "transcriptLayout",
                "uiScale",
                "wordWrap",
                "workspaceTheme"
            ]
            .into_iter()
            .filter(|n| name == "settings_update" || !matches!(*n, "changed" | "settings"))
            .collect::<Vec<_>>(),
            "the allowlist changes only on purpose: {name}"
        );
    }
    for forbidden in [
        json!({"binaryPaths": {"claude": "/tmp/evil"}}),
        json!({"claudeHooks": false}),
        json!({"defaultAccess": "full-access"}),
        json!({"keybindings": {}}),
        json!({"maskEmails": false}),
        json!({"skillSourcePaths": ["/tmp"]}),
    ] {
        let call = json!({"operation": "settings_update", "input": forbidden});
        assert!(
            serde_json::from_value::<Command>(call.clone()).is_err(),
            "refused: {call}"
        );
    }
}

#[test]
fn the_vcs_scope_is_off_by_default_and_old_policies_stay_valid() {
    assert!(!Scope::DEFAULT_GRANT.contains(&Scope::Vcs));
    assert_eq!(Scope::Vcs.id(), "vcs");
    assert_eq!(serde_json::to_value(Scope::Vcs).unwrap(), "vcs");
    // Policies kept before the permission ceiling went away still carry it.
    let old = r#"{"enabled":true,"ceiling":"supervised","clients":[{"name":"Cursor","scopes":["read","navigate"],"approved_at":5,"last_seen":6}]}"#;
    let mut policy = Policy::parse(Some(old));
    assert!(policy.enabled, "an old policy still parses");
    assert!(
        !serde_json::to_string(&policy).unwrap().contains("ceiling"),
        "and its ceiling is dropped"
    );
    let grant = policy.grant("Cursor").unwrap();
    assert_eq!(grant.scopes, [Scope::Read, Scope::Navigate]);
    assert!(!grant.allows(Scope::Vcs));
    let approved = policy.approve("Codex", 7);
    assert!(
        !approved.scopes.contains(&Scope::Vcs),
        "new clients start without it"
    );
    assert!(policy.set_scope("Cursor", Scope::Vcs, true));
    assert!(policy.grant("Cursor").unwrap().allows(Scope::Vcs));
}

#[test]
fn git_paths_and_schedules_are_closed_objects() {
    let unknown = serde_json::json!({
        "operation": "git_stage",
        "input": { "paths": ["a"], "force": true }
    });
    assert!(serde_json::from_value::<Command>(unknown).is_err());
    let schedule = serde_json::json!({ "kind": "hourly", "minute": 5, "every": 2 });
    assert!(serde_json::from_value::<Schedule>(schedule).is_err());
    let commit = serde_json::json!({ "operation": "git_commit", "input": {} });
    assert!(
        serde_json::from_value::<Command>(commit).is_err(),
        "a commit message is required"
    );
}

/// Turning an automation on, or saving one with a prompt, schedules that
/// prompt to run: it needs `dispatch` as well as `manage`.
#[test]
fn scheduling_a_prompt_also_needs_dispatch() {
    let entries = catalog();
    for name in ["automation_set_enabled", "automation_upsert"] {
        let descriptor = entries.iter().find(|d| d.name == name).unwrap();
        assert_eq!(descriptor.scope, Scope::Manage, "{name}");
        assert_eq!(descriptor.also, [Scope::Dispatch], "{name}");
        assert!(descriptor.description.contains("dispatch"), "{name}");
    }
    let others: Vec<_> = entries
        .iter()
        .filter(|d| !d.also.is_empty())
        .map(|d| d.name)
        .collect();
    assert_eq!(
        others,
        [
            "automation_set_enabled",
            "automation_upsert",
            "mono_habit_add",
            "mono_habit_update"
        ]
    );
    let enable = |enabled| {
        Command::AutomationSetEnabled(AutomationSetEnabledInput {
            automation_id: "a1".into(),
            enabled,
        })
        .scopes()
    };
    assert_eq!(enable(true), [Scope::Manage, Scope::Dispatch]);
    assert_eq!(
        enable(false),
        [Scope::Manage],
        "turning off schedules nothing"
    );
    let upsert = |input| Command::AutomationUpsert(input).scopes();
    let rename = AutomationUpsertInput {
        automation_id: Some("a1".into()),
        name: Some("Nightly".into()),
        ..AutomationUpsertInput::default()
    };
    assert_eq!(upsert(rename.clone()), [Scope::Manage]);
    for input in [
        AutomationUpsertInput {
            prompt: Some("Run the tests".into()),
            ..rename.clone()
        },
        AutomationUpsertInput {
            enabled: Some(true),
            ..rename.clone()
        },
        AutomationUpsertInput {
            automation_id: None,
            ..rename
        },
    ] {
        assert_eq!(upsert(input), [Scope::Manage, Scope::Dispatch]);
    }
    assert_eq!(
        Command::SessionStop(SessionInput { session_id: 1 }).scopes(),
        [Scope::Dispatch]
    );
}

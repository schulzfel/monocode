//! The old app actions (`sessions.start`, `folders.move`, …) as control
//! operations: each dotted name maps to one operation, and its input to that
//! operation's input, so every old call still works. The fields each action
//! took are the old validator's, frozen in [`fields`], so a misspelled field
//! is still refused; the operations' typed inputs check the rest. The
//! actions only a Mono had (artifacts, soul, memory, habits and cards) are
//! the operations only a Mono may call.
use monocode_control::ops::{model::MAX_MODELS_PER_PROVIDER, session::MAX_SESSIONS_PER_PAGE};
use serde_json::{Map, Value, json};

mod fields;
#[cfg(test)]
mod tests;

pub(crate) use fields::fields;

/// Every old action, in the old help's order.
pub(crate) const ACTIONS: &[&str] = &[
    "models.list",
    "sessions.list",
    "sessions.read",
    "sessions.start",
    "sessions.send",
    "sessions.draft",
    "sessions.stop",
    "sessions.archive",
    "sessions.delete",
    "worktrees.list",
    "worktrees.create",
    "folders.list",
    "folders.move",
    "notes.list",
    "notes.read",
    "notes.write",
    "artifacts.list",
    "artifacts.read",
    "artifacts.write",
    "soul.read",
    "soul.update",
    "memory.read",
    "memory.search",
    "memory.add",
    "memory.replace",
    "memory.remove",
    "habits.list",
    "habits.add",
    "habits.update",
    "habits.run",
    "habits.remove",
    "chat.card",
];

/// What an unknown action is told, in the old CLI's words.
pub(crate) const UNKNOWN: &str = "Unknown app action. Run app help";

/// What a session ID that is not a number is told: the Tauri app's UUIDs are
/// gone (C-4).
pub(crate) const NUMERIC_SESSION: &str = "Use the numeric session ID from sessions.list";

/// The operation old action `action` is, and its `input` as that operation's
/// input: the wire form of a `monocode_control::Command`,
/// `{"operation", "input"}`.
pub(crate) fn translate(action: &str, input: &Value) -> Result<Value, String> {
    let fields = fields(action).ok_or(UNKNOWN)?;
    let given = input.as_object().ok_or("Input must be an object")?;
    if let Some(key) = given
        .keys()
        .find(|key| !fields.split_whitespace().any(|field| field == key.as_str()))
    {
        return Err(format!("{action} does not take {key}"));
    }
    let mut input = given.clone();
    session_ids(&mut input)?;
    let operation = match action {
        "models.list" => {
            // Operator listed every model.
            input.insert("limit".into(), json!(MAX_MODELS_PER_PROVIDER));
            "model_list"
        }
        "sessions.list" => {
            // The old list held archived sessions too, in one page.
            input.insert("archived".into(), json!("include"));
            input.insert("limit".into(), json!(MAX_SESSIONS_PER_PAGE));
            "session_list"
        }
        "sessions.read" => {
            drop_project(&mut input)?;
            // A number was a turn index, clamped to the turns there are.
            if let Some(before) = input.remove("before") {
                let key = if before.is_u64() {
                    "beforeIndex"
                } else {
                    "before"
                };
                input.insert(key.into(), before);
            }
            "session_read"
        }
        "sessions.start" => {
            rename(&mut input, "runtimeMode", "access");
            if input.get("workspaceMode").and_then(Value::as_str) == Some("checkout") {
                input.insert("workspaceMode".into(), json!("current"));
            }
            "session_start"
        }
        "sessions.send" => {
            drop_project(&mut input)?;
            // The old send only went into an idle session without a draft.
            input.insert("ifIdle".into(), json!(true));
            "session_send"
        }
        "sessions.draft" => {
            drop_project(&mut input)?;
            rename(&mut input, "prompt", "text");
            "session_draft"
        }
        "sessions.stop" => {
            drop_project(&mut input)?;
            "session_stop"
        }
        "sessions.archive" => {
            drop_project(&mut input)?;
            // The old archive stopped a working session first.
            input.insert("stop".into(), json!(true));
            "session_archive"
        }
        "sessions.delete" => {
            drop_project(&mut input)?;
            "session_delete"
        }
        "worktrees.list" => "worktree_list",
        "worktrees.create" => "worktree_create",
        "folders.list" => "folder_list",
        "folders.move" => {
            drop_project(&mut input)?;
            // A new folder's name won over a folder ID; with neither, the
            // session left its folder.
            if input.contains_key("newFolderName") {
                input.remove("folderId");
            } else if !input.contains_key("folderId") {
                input.insert("removeFromFolder".into(), json!(true));
            }
            "session_move"
        }
        "notes.list" => {
            offset_to_cursor(&mut input);
            "notes_list"
        }
        "notes.read" | "notes.write" => {
            if let Some(id) = input.get("id").and_then(Value::as_u64) {
                input.insert("id".into(), json!(id.to_string()));
            }
            if action == "notes.read" {
                "notes_read"
            } else {
                "notes_write"
            }
        }
        "artifacts.list" => {
            offset_to_cursor(&mut input);
            "mono_artifact_list"
        }
        "artifacts.read" => "mono_artifact_read",
        "artifacts.write" => "mono_artifact_write",
        "soul.read" => "mono_soul_read",
        "soul.update" => "mono_soul_update",
        "memory.read" => "mono_memory_read",
        "memory.search" => "mono_memory_search",
        "memory.add" => "mono_memory_add",
        "memory.replace" => "mono_memory_replace",
        "memory.remove" => "mono_memory_remove",
        "habits.list" => "mono_habit_list",
        "habits.add" => "mono_habit_add",
        "habits.update" => "mono_habit_update",
        "habits.run" => "mono_habit_run",
        "habits.remove" => "mono_habit_remove",
        "chat.card" => "mono_card_post",
        _ => return Err(UNKNOWN.into()),
    };
    Ok(json!({ "operation": operation, "input": input }))
}

/// Operator's rule for old action `action` where its grammar was not the
/// Mono's, for a call outside a Mono's chat: `folders.move` takes exactly
/// one of `folderId` and `newFolderName` (OP-48). The role that picks it
/// is a hint (`MONOCODE_CONTROL_ROLE`); it authorizes nothing.
pub(crate) fn operator_grammar(action: &str, input: &Value) -> Result<(), String> {
    let given = |key: &str| input.get(key).is_some();
    if action == "folders.move" && given("folderId") == given("newFolderName") {
        return Err(FOLDER_CHOICE.into());
    }
    Ok(())
}

/// What Operator's `folders.move` with neither or both folders was told.
pub(crate) const FOLDER_CHOICE: &str = "Supply exactly one of folderId or newFolderName";

/// The old actions that are operation `operation`, for its help line.
pub(crate) fn aliases(operation: &str) -> Vec<&'static str> {
    ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            translate(action, &json!({}))
                .is_ok_and(|wire| wire["operation"].as_str() == Some(operation))
        })
        .collect()
}

/// Session IDs as the integers the operations take: a numeric string
/// becomes its number; any other string is refused (C-4).
fn session_ids(input: &mut Map<String, Value>) -> Result<(), String> {
    for key in ["sessionId", "besideSessionId"] {
        let Some(text) = input.get(key).and_then(Value::as_str) else {
            continue;
        };
        let id: i64 = text.parse().map_err(|_| NUMERIC_SESSION.to_owned())?;
        input.insert(key.into(), json!(id));
    }
    Ok(())
}

/// A list's `offset` as its `cursor`: the cursor is the offset, as text.
fn offset_to_cursor(input: &mut Map<String, Value>) {
    if let Some(offset) = input.remove("offset") {
        let cursor = offset.as_u64().map_or(offset, |n| json!(n.to_string()));
        input.insert("cursor".into(), cursor);
    }
}

/// Drops `project` from the input of an operation that takes none: the
/// session it acts on decides the project. It still had to be text.
fn drop_project(input: &mut Map<String, Value>) -> Result<(), String> {
    match input.remove("project") {
        Some(project) if !project.is_string() => Err("project must be text".into()),
        _ => Ok(()),
    }
}

/// Renames field `from` to `to`, keeping its value.
fn rename(input: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = input.remove(from) {
        input.insert(to.into(), value);
    }
}

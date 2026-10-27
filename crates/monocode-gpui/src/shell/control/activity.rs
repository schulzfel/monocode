//! What external clients and MonoCode's own sessions changed (a session
//! named by its title): a capped ring buffer, newest last, kept in
//! the store (`external-control-activity.v1`) for Settings to show. Every
//! call that may change something is logged, refused or not, and every
//! approval and confirmation decision; reads are not, so a client polling
//! `session_status` neither floods the log nor rewrites it. Changes gather
//! for [`SAVE_DELAY`] before the log is written, and a pending write is
//! made before the app quits.
use super::*;
use monocode_control::{Command, Scope};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The most entries kept.
pub(super) const CAPACITY: usize = 200;
/// How long changes to the log gather before it is written.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// One call or approval decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Entry {
    /// When, in Unix milliseconds.
    pub(super) at: i64,
    pub(super) client: String,
    /// The operation, `approve` / `decline` for a hello decision, or
    /// `confirm` / `decline` for a confirmation prompt.
    pub(super) operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) scope: Option<Scope>,
    /// What the call named: a session, message, question, project or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target: Option<String>,
    /// `ok` or the error code.
    pub(super) outcome: String,
}

#[derive(Default)]
pub(super) struct Activity {
    entries: VecDeque<Entry>,
    /// Whether it changed since it was last written; a write is scheduled.
    unsaved: bool,
}

impl Activity {
    pub(super) fn parse(raw: Option<&str>) -> Self {
        let mut entries: VecDeque<Entry> = raw
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        while entries.len() > CAPACITY {
            entries.pop_front();
        }
        Self {
            entries,
            unsaved: false,
        }
    }
    pub(super) fn push(&mut self, entry: Entry) {
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }
    /// Oldest first.
    pub(super) fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter()
    }
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    fn encode(&self) -> String {
        serde_json::to_string(&self.entries).unwrap_or_else(|_| "[]".into())
    }
}

/// A call's command as the wire carries it, for the log and the call's
/// receipt: made once per call, and only when one of them needs it.
pub(super) fn wire(command: &Command, request_id: Option<&str>) -> Option<Value> {
    (!command.hints().read_only || request_id.is_some())
        .then(|| serde_json::to_value(command).unwrap_or_default())
}

/// What the command `wire` acts on, for the log: its session, then the
/// thing within it, or its project or path.
pub(super) fn target(wire: &Value) -> Option<String> {
    let input = wire.get("input")?;
    let text = |key: &str| match input.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    let session = text("sessionId").map(|id| format!("session {id}"));
    let inner = text("questionId")
        .or_else(|| text("messageId"))
        .or_else(|| text("besideSessionId").map(|id| format!("beside {id}")));
    match (session, inner) {
        (Some(session), Some(inner)) => Some(format!("{session}, {inner}")),
        (Some(session), None) => Some(session),
        (None, _) => text("project")
            .or_else(|| text("automationId").map(|id| format!("automation {id}")))
            .or_else(|| text("monoId").map(|id| format!("Mono {id}")))
            .or_else(|| text("path"))
            .or_else(|| text("id"))
            .or_else(|| text("tab"))
            .map(|t| t.chars().take(120).collect()),
    }
}

impl Shell {
    /// Records the refusal of `call` and answers with it. A refusal is not a
    /// result: no receipt, so a retry after the person grants the scope runs
    /// for real.
    pub(super) fn refuse_control(
        &mut self,
        call: Pending,
        error: ControlError,
        cx: &mut Context<Self>,
    ) {
        let result = Err(error);
        let logged = wire(&call.command, call.request_id.as_deref());
        self.record_control(&call.caller, &call.command, logged.as_ref(), &result, cx);
        let _ = call.reply.send(result);
    }

    /// Logs a finished call, unless it only reads; `wire` is its command
    /// as [`wire`] made it.
    pub(super) fn record_control(
        &mut self,
        caller: &CallerKind,
        command: &Command,
        wire: Option<&Value>,
        result: &Result<Value, ControlError>,
        cx: &mut Context<Self>,
    ) {
        if command.hints().read_only {
            return;
        }
        let client = self.caller_label(caller, cx);
        self.push_activity(
            Entry {
                at: now_ms(),
                client,
                operation: command.name().to_owned(),
                scope: Some(command.scope()),
                target: wire.and_then(target),
                outcome: result
                    .as_ref()
                    .map_or_else(|e| e.code.id().to_owned(), |_| "ok".into()),
            },
            cx,
        );
    }

    /// Logs the person's answer to an approval prompt.
    pub(super) fn record_decision(&mut self, client: &str, allowed: bool, cx: &mut Context<Self>) {
        self.push_activity(
            Entry {
                at: now_ms(),
                client: client.to_owned(),
                operation: if allowed { "approve" } else { "decline" }.into(),
                scope: None,
                target: None,
                outcome: if allowed { "ok" } else { "user_declined" }.into(),
            },
            cx,
        );
    }

    /// Logs the person's answer to a confirmation prompt about `command`.
    pub(super) fn record_confirmation(
        &mut self,
        caller: &CallerKind,
        command: &Command,
        target: &str,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) {
        let client = self.caller_label(caller, cx);
        self.push_activity(
            Entry {
                at: now_ms(),
                client,
                operation: if confirmed { "confirm" } else { "decline" }.into(),
                scope: Some(command.scope()),
                target: Some(
                    format!("{}: {target}", command.name())
                        .chars()
                        .take(160)
                        .collect(),
                ),
                outcome: if confirmed { "ok" } else { "user_declined" }.into(),
            },
            cx,
        );
    }

    fn push_activity(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.control.activity.push(entry);
        self.save_activity(cx);
        self.sync_settings(cx);
    }

    /// Forgets every logged call, as Settings' Clear does, at once.
    pub(super) fn clear_activity(&mut self, cx: &mut Context<Self>) {
        self.control.activity.clear();
        self.control.activity.unsaved = true;
        self.flush_activity(cx).detach();
    }

    /// Writes the log once its changes have gathered for [`SAVE_DELAY`];
    /// changes made meanwhile go with the same write.
    fn save_activity(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.control.activity.unsaved, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, cx| this.flush_activity(cx).detach());
        })
        .detach();
    }

    /// Writes the log now if it has unsaved changes; the task ends once it
    /// is stored.
    pub(super) fn flush_activity(&mut self, cx: &mut Context<Self>) -> gpui::Task<()> {
        if !std::mem::take(&mut self.control.activity.unsaved) {
            return gpui::Task::ready(());
        }
        let store = self.store.clone();
        let encoded = self.control.activity.encode();
        cx.background_executor().spawn(async move {
            if let Err(error) = locked(&store).and_then(|s| s.save_setting(ACTIVITY_KEY, &encoded))
            {
                eprintln!("Cannot save external control activity: {error}");
            }
        })
    }

    /// The log as Settings lists it, newest first.
    pub(super) fn activity_rows(&self) -> Vec<crate::settings::ControlActivity> {
        self.control
            .activity
            .entries()
            .rev()
            .map(|e| crate::settings::ControlActivity {
                at: e.at,
                client: e.client.clone(),
                operation: e.operation.clone(),
                target: e.target.clone(),
                outcome: e.outcome.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_activity_log_is_a_capped_ring() {
        let mut log = Activity::parse(Some("not json"));
        for at in 0..(CAPACITY as i64 + 5) {
            log.push(Entry {
                at,
                client: "c".into(),
                operation: "app_info".into(),
                scope: Some(Scope::Read),
                target: None,
                outcome: "ok".into(),
            });
        }
        let kept: Vec<_> = log.entries().map(|e| e.at).collect();
        assert_eq!(kept.len(), CAPACITY);
        assert_eq!(kept[0], 5, "the oldest entries go first");
    }

    #[test]
    fn each_entry_names_what_the_call_acted_on() {
        use monocode_control::ops::{
            project::ProjectListInput, queue::QueueItemInput, rail::ProjectTarget,
            session::SessionInput,
        };
        let target = |command: Command| target(&serde_json::to_value(command).unwrap());
        assert_eq!(
            target(Command::SessionStop(SessionInput { session_id: 7 })).as_deref(),
            Some("session 7")
        );
        assert_eq!(
            target(Command::QueueRemove(QueueItemInput {
                session_id: 7,
                message_id: "m1".into()
            }))
            .as_deref(),
            Some("session 7, m1")
        );
        assert_eq!(
            target(Command::ProjectArchive(ProjectTarget {
                project: "/work/old".into()
            }))
            .as_deref(),
            Some("/work/old")
        );
        assert_eq!(
            target(Command::ProjectList(ProjectListInput::default())),
            None
        );
    }
}

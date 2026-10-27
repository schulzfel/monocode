//! Automations: saved prompts that run on a schedule or on demand. A client
//! never runs, enables or saves one whose run mode is full access.
use crate::{AccessMode, Hints, Operation, Scope, ops::session::Text};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Automations listed per page unless `limit` says otherwise.
pub const DEFAULT_AUTOMATIONS_PER_PAGE: u32 = 50;
/// The largest `limit` accepted.
pub const MAX_AUTOMATIONS_PER_PAGE: u32 = 200;
/// Runs `automation_read` lists unless `runs` says otherwise.
pub const DEFAULT_RUNS: u32 = 10;
/// The largest `runs` accepted.
pub const MAX_RUNS: u32 = 50;
/// The most characters of a prompt `automation_read` returns.
pub const MAX_PROMPT_CHARS: usize = 4_000;
/// The longest name accepted, in characters.
pub const MAX_NAME_CHARS: usize = 200;
/// The longest prompt accepted, in bytes, as for `session_start`.
pub const MAX_PROMPT_BYTES: usize = 240_000;
/// The most schedules one automation has.
pub const MAX_SCHEDULES: usize = 20;

/// How often a schedule fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// Every hour at `minute`.
    Hourly,
    /// Every day at `time`.
    Daily,
    /// Monday to Friday at `time`.
    Weekdays,
    /// Once a week on `dayOfWeek` at `time`.
    Weekly,
}

/// A time schedule, in the computer's time zone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    /// How often it fires.
    pub kind: ScheduleKind,
    /// For `hourly`: the minute past the hour, 0 to 59 (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<i64>,
    /// For the others: the time of day as `HH:MM` (default `09:00`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// For `weekly`: 0 (Sunday) to 6 (Saturday) (default 1, Monday).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_week: Option<i64>,
}

/// Where an automation's session works.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWorkspace {
    /// The project's folder.
    Current,
    /// A new worktree for each run.
    Worktree,
    /// An existing worktree (set in the app).
    Existing,
}

/// An automation as lists show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSummary {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its project's path.
    pub project: String,
    /// The provider it runs.
    pub harness: String,
    /// The model it runs.
    pub model: String,
    /// The access mode its sessions get.
    pub runtime_mode: AccessMode,
    /// Whether its schedules fire.
    pub enabled: bool,
    /// Its triggers, as the app labels them.
    pub trigger: String,
    /// The next scheduled run, in Unix milliseconds, while enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<i64>,
    /// The last run's start, in Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<i64>,
    /// The last run's status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<String>,
}

/// `automation_list`: the automations, optionally of one project.
#[derive(Debug)]
pub struct AutomationList;

/// Which automations, and one page of them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationListInput {
    /// Only this project's (path or key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Automations per page, 1 to 200 (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `nextCursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// One page of automations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListOutput {
    /// The automations, by name.
    pub automations: Vec<AutomationSummary>,
    /// Pass as `cursor` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Operation for AutomationList {
    const NAME: &'static str = "automation_list";
    const TITLE: &'static str = "List automations";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List automations (saved prompts that run on a schedule or on demand): name, project, harness, model, run mode, triggers, next and last run. Paged.";
    const HINTS: Hints = Hints::READ;
    type Input = AutomationListInput;
    type Output = AutomationListOutput;

    fn summary(output: &AutomationListOutput) -> String {
        format!(
            "{} automations{}",
            output.automations.len(),
            if output.next_cursor.is_some() {
                ", more on the next page"
            } else {
                ""
            }
        )
    }
}

/// One automation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationInput {
    /// The automation's id, from `automation_list`.
    pub automation_id: String,
}

/// `automation_read`: an automation's definition and recent runs.
#[derive(Debug)]
pub struct AutomationRead;

/// The automation and how many runs to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationReadInput {
    /// The automation's id.
    pub automation_id: String,
    /// Recent runs listed, 1 to 50 (default 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
}

/// One run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunInfo {
    /// The run's id.
    pub id: String,
    /// What started it: `schedule`, `manual` or `event`.
    pub trigger: String,
    /// `pending`, `running`, `succeeded`, `failed` or `skipped`.
    pub status: String,
    /// When it was due, in Unix milliseconds.
    pub scheduled_for: i64,
    /// When it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    /// When it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    /// Why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The definition and runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationReadOutput {
    /// The automation.
    pub automation: AutomationSummary,
    /// Its prompt, cut to 4,000 characters.
    pub prompt: Text,
    /// Where its sessions work.
    pub workspace_mode: AutomationWorkspace,
    /// Its time schedules.
    pub schedules: Vec<Schedule>,
    /// Its event triggers (set in the app), as labelled there.
    pub events: Vec<String>,
    /// Its latest runs, newest first.
    pub runs: Vec<AutomationRunInfo>,
    /// Older runs left out.
    pub runs_omitted: u32,
}

impl Operation for AutomationRead {
    const NAME: &'static str = "automation_read";
    const TITLE: &'static str = "Read automation";
    const SUBJECT: &'static [&'static str] = &["automationId"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read one automation: its definition (prompt, project, harness, model, run mode, workspace, schedules and event triggers) and its latest runs.";
    const HINTS: Hints = Hints::READ;
    type Input = AutomationReadInput;
    type Output = AutomationReadOutput;

    fn summary(output: &AutomationReadOutput) -> String {
        format!(
            "Automation “{}” ({}), {} runs listed",
            output.automation.name,
            output.automation.trigger,
            output.runs.len()
        )
    }
}

/// `automation_run`: run an automation now.
#[derive(Debug)]
pub struct AutomationRun;

/// The started run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunOutput {
    /// The automation.
    pub automation_id: String,
    /// The new run's id; `automation_read` follows it.
    pub run_id: String,
}

impl Operation for AutomationRun {
    const NAME: &'static str = "automation_run";
    const TITLE: &'static str = "Run automation";
    const SUBJECT: &'static [&'static str] = &["automationId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Run an automation now, as its Run now button does: a session starts with its prompt. Refused with invalid_input when its run mode is full access.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = AutomationInput;
    type Output = AutomationRunOutput;

    fn summary(output: &AutomationRunOutput) -> String {
        format!(
            "Started run {} of automation {}",
            output.run_id, output.automation_id
        )
    }
}

/// `automation_set_enabled`: turn an automation's schedules on or off.
#[derive(Debug)]
pub struct AutomationSetEnabled;

/// The automation and its state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationSetEnabledInput {
    /// The automation's id.
    pub automation_id: String,
    /// Whether its schedules fire.
    pub enabled: bool,
}

impl Operation for AutomationSetEnabled {
    const NAME: &'static str = "automation_set_enabled";
    const TITLE: &'static str = "Turn automation on or off";
    const SUBJECT: &'static [&'static str] = &["automationId"];
    const SCOPE: Scope = Scope::Manage;
    const ALSO: &'static [Scope] = &[Scope::Dispatch];
    const DESCRIPTION: &'static str = "Turn an automation's schedules on or off. Turning one on schedules its prompt to run, so it needs the dispatch scope as well as manage, and is refused with invalid_input when its run mode is full access; turning off needs only manage and always works.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = AutomationSetEnabledInput;
    type Output = AutomationSummary;

    fn needs_also(input: &AutomationSetEnabledInput) -> bool {
        input.enabled
    }

    fn summary(output: &AutomationSummary) -> String {
        format!(
            "Automation “{}” is {}",
            output.name,
            if output.enabled { "on" } else { "off" }
        )
    }
}

/// `automation_upsert`: create or change an automation.
#[derive(Debug)]
pub struct AutomationUpsert;

/// The automation's fields. Creating needs `name`, `prompt`, `harness` and
/// `model`; changing keeps every field left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationUpsertInput {
    /// The automation to change; a new one is created when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    /// Its name (1 to 200 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its prompt (at most 240,000 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// A local project's path or key; when creating without one, the
    /// calling agent session's project, or for other clients the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The provider, for example `claude` or `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// A model id from `model_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The access mode its sessions get: `supervised` (default when
    /// creating), `auto-accept-edits` or `auto`. Full access is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<AccessMode>,
    /// `current` or `worktree` (default when creating).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_mode: Option<AutomationWorkspace>,
    /// Its time schedules, replacing the old ones (at most 20); none means
    /// it only runs on demand. Event triggers set in the app are kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules: Option<Vec<Schedule>>,
    /// Whether its schedules fire (default on when creating).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// The saved automation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpsertOutput {
    /// The automation as saved.
    pub automation: AutomationSummary,
    /// Whether it was created rather than changed.
    pub created: bool,
}

impl Operation for AutomationUpsert {
    const NAME: &'static str = "automation_upsert";
    const TITLE: &'static str = "Save automation";
    const SUBJECT: &'static [&'static str] = &["name", "automationId"];
    const SCOPE: Scope = Scope::Manage;
    const ALSO: &'static [Scope] = &[Scope::Dispatch];
    const DESCRIPTION: &'static str = "Create an automation, or change one by id: name, prompt, project, harness, model, run mode, workspace, time schedules and whether it is on. Creating one, setting its prompt or turning it on schedules a prompt to run, so those need the dispatch scope as well as manage. The run mode is never full access (invalid_input). The app validates schedules as its editor does.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = AutomationUpsertInput;
    type Output = AutomationUpsertOutput;

    fn project_slot(input: &mut AutomationUpsertInput) -> Option<&mut Option<String>> {
        // A new automation runs in the default project; a change keeps its own.
        input.automation_id.is_none().then_some(&mut input.project)
    }

    /// Creating one, setting its prompt or turning it on schedules a prompt.
    fn needs_also(input: &AutomationUpsertInput) -> bool {
        input.automation_id.is_none() || input.prompt.is_some() || input.enabled == Some(true)
    }

    fn summary(output: &AutomationUpsertOutput) -> String {
        format!(
            "{} automation “{}” ({})",
            if output.created { "Created" } else { "Saved" },
            output.automation.name,
            output.automation.trigger
        )
    }
}

/// `automation_delete`: delete an automation and its run history.
#[derive(Debug)]
pub struct AutomationDelete;

/// The deleted automation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDeleteOutput {
    /// Its id.
    pub automation_id: String,
    /// Its name.
    pub name: String,
}

impl Operation for AutomationDelete {
    const NAME: &'static str = "automation_delete";
    const TITLE: &'static str = "Delete automation";
    const SUBJECT: &'static [&'static str] = &["automationId"];
    const SCOPE: Scope = Scope::Destructive;
    const DESCRIPTION: &'static str = "Delete an automation and its run history, after the person confirms in MonoCode. Sessions it started stay. Declining, or no answer within two minutes, fails with user_declined and changes nothing.";
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    type Input = AutomationInput;
    type Output = AutomationDeleteOutput;

    fn summary(output: &AutomationDeleteOutput) -> String {
        format!("Deleted automation “{}”", output.name)
    }
}

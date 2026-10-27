//! A Mono's habits: scheduled work that runs in sessions of its own and
//! reports back in its chat. Only the Mono's chat and habit runs call these,
//! on the calling Mono's habits; a habit run may list them but not change or
//! run them.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most habits one Mono has.
pub const MAX_HABITS: usize = 20;
/// The longest name accepted, in bytes; it must also fit 120 characters.
pub const MAX_HABIT_NAME_BYTES: usize = 480;
/// The longest name accepted, in characters.
pub const MAX_HABIT_NAME_CHARS: usize = 120;
/// The longest instructions accepted, in bytes.
pub const MAX_HABIT_INSTRUCTIONS_BYTES: usize = 32_000;
/// The longest habit id accepted, in bytes.
pub const MAX_HABIT_ID_BYTES: usize = 128;

/// When a habit runs, in the computer's time zone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HabitSchedule {
    /// `hourly`, `daily`, `weekdays` or `weekly` (also accepted as `kind`).
    #[serde(alias = "kind")]
    pub schedule_kind: String,
    /// For `hourly`: the minute past the hour, 0 to 59 (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// For the others: the time of day as `HH:MM` (default `09:00`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// For `weekly`: 0 (Sunday) to 6 (Saturday) (default 1, Monday).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_week: Option<u32>,
}

/// One past run of a habit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HabitRun {
    /// When it started, in Unix milliseconds.
    pub at: u64,
    /// How long it took.
    #[serde(default)]
    pub duration_ms: u64,
    /// `posted`, `quiet`, `failed` or `interrupted`.
    pub outcome: String,
    /// Its report, when it posted one.
    #[serde(default)]
    pub report: Option<String>,
    /// Why it failed.
    #[serde(default)]
    pub error: Option<String>,
}

/// A habit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What each run does.
    pub instructions: String,
    /// When it runs.
    pub schedule: HabitSchedule,
    /// Whether its schedule runs it.
    pub enabled: bool,
    /// When it was added, in Unix milliseconds.
    #[serde(default)]
    pub created_at: u64,
    /// Its next scheduled run, in Unix milliseconds.
    #[serde(default)]
    pub next_run_at: u64,
    /// Its last run's start.
    #[serde(default)]
    pub last_run_at: Option<u64>,
    /// Its last run's outcome.
    #[serde(default)]
    pub last_outcome: Option<String>,
    /// Why its last run failed.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Its newest runs, newest first (at most 20).
    #[serde(default)]
    pub runs: Vec<HabitRun>,
    /// Whether it runs at the next scheduler tick, whatever its schedule.
    #[serde(default)]
    pub run_requested: bool,
    /// When its current run started, while one runs.
    #[serde(default)]
    pub running_since: Option<u64>,
    /// The suggestion card it was added from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_card: Option<String>,
}

/// `mono_habit_list`: the Mono's habits.
#[derive(Debug)]
pub struct MonoHabitList;

/// No arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoHabitListInput {}

/// The habits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoHabitListOutput {
    /// In the order they were added.
    pub habits: Vec<Habit>,
}

impl Operation for MonoHabitList {
    const NAME: &'static str = "mono_habit_list";
    const TITLE: &'static str = "List habits";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List your habits: each one's name, instructions, schedule, whether it is on, its next run and its recent runs.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = MonoHabitListInput;
    type Output = MonoHabitListOutput;

    fn summary(output: &MonoHabitListOutput) -> String {
        format!("{} habits", output.habits.len())
    }
}

/// `mono_habit_add`: a new habit, on.
#[derive(Debug)]
pub struct MonoHabitAdd;

/// The habit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoHabitAddInput {
    /// Its name (at most 120 characters).
    pub name: String,
    /// What each run does (at most 32,000 bytes); say whether it may change
    /// anything.
    pub instructions: String,
    /// When it runs.
    pub schedule: HabitSchedule,
}

impl Operation for MonoHabitAdd {
    const NAME: &'static str = "mono_habit_add";
    const TITLE: &'static str = "Add habit";
    const SUBJECT: &'static [&'static str] = &["name"];
    const SCOPE: Scope = Scope::Manage;
    const ALSO: &'static [Scope] = &[Scope::Dispatch];
    const DESCRIPTION: &'static str = "Add a habit, only after the person agreed: scheduled work that runs in a session of its own and reports back here; a quiet run replies NOTHING_TO_REPORT. It schedules runs, so it needs the dispatch scope as well as manage. At most 20 habits.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = MonoHabitAddInput;
    type Output = Habit;

    /// A new habit is on: its schedule runs it.
    fn needs_also(_input: &MonoHabitAddInput) -> bool {
        true
    }

    fn summary(output: &Habit) -> String {
        format!("Added habit “{}”", output.name)
    }
}

/// `mono_habit_update`: a habit's fields changed.
#[derive(Debug)]
pub struct MonoHabitUpdate;

/// The habit and what changes; every field left out is kept.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoHabitUpdateInput {
    /// The habit's id, from `mono_habit_list`.
    pub id: String,
    /// Its new name (at most 120 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its new instructions (at most 32,000 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Its new schedule; its next run is worked out again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<HabitSchedule>,
    /// Whether its schedule runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl Operation for MonoHabitUpdate {
    const NAME: &'static str = "mono_habit_update";
    const TITLE: &'static str = "Update habit";
    const SUBJECT: &'static [&'static str] = &["name", "id"];
    const SCOPE: Scope = Scope::Manage;
    const ALSO: &'static [Scope] = &[Scope::Dispatch];
    const DESCRIPTION: &'static str = "Change a habit's name, instructions, schedule or whether it is on. Turning it on or giving it a schedule schedules runs, so those need the dispatch scope as well as manage.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const MONO_ONLY: bool = true;
    type Input = MonoHabitUpdateInput;
    type Output = Habit;

    /// Turning it on or rescheduling it schedules runs.
    fn needs_also(input: &MonoHabitUpdateInput) -> bool {
        input.enabled == Some(true) || input.schedule.is_some()
    }

    fn summary(output: &Habit) -> String {
        format!(
            "Habit “{}” is {}",
            output.name,
            if output.enabled { "on" } else { "off" }
        )
    }
}

/// One habit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HabitInput {
    /// The habit's id, from `mono_habit_list`.
    pub id: String,
}

/// `mono_habit_run`: a habit run at the next scheduler tick.
#[derive(Debug)]
pub struct MonoHabitRun;

impl Operation for MonoHabitRun {
    const NAME: &'static str = "mono_habit_run";
    const TITLE: &'static str = "Run habit";
    const SUBJECT: &'static [&'static str] = &["id"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Run a habit now, whatever its schedule and even while it is off: its run starts at the next scheduler tick, within about ten seconds.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = HabitInput;
    type Output = Habit;

    fn summary(output: &Habit) -> String {
        format!("Habit “{}” runs next", output.name)
    }
}

/// `mono_habit_remove`: a habit deleted.
#[derive(Debug)]
pub struct MonoHabitRemove;

/// The removed habit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoHabitRemoveOutput {
    /// Always true.
    pub removed: bool,
}

impl Operation for MonoHabitRemove {
    const NAME: &'static str = "mono_habit_remove";
    const TITLE: &'static str = "Remove habit";
    const SUBJECT: &'static [&'static str] = &["id"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str =
        "Delete a habit and its run history. Sessions its runs started stay.";
    const HINTS: Hints = Hints::DISCARD;
    const MONO_ONLY: bool = true;
    type Input = HabitInput;
    type Output = MonoHabitRemoveOutput;

    fn summary(_output: &MonoHabitRemoveOutput) -> String {
        "Removed the habit".into()
    }
}

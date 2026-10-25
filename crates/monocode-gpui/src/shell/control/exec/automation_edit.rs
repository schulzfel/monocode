//! `automation_upsert`: the Automations editor's save, from typed fields.
//! A change starts from the saved automation, so fields left out keep their
//! values; the service validates the result as the editor's save does. The
//! run mode is never full access.
use super::automation::{allowed, find, mode, summary};
use super::*;
use monocode_control::{
    AccessMode,
    ops::automation::{
        AutomationUpsertInput, AutomationUpsertOutput, AutomationWorkspace, MAX_NAME_CHARS,
        MAX_PROMPT_BYTES, MAX_SCHEDULES, Schedule, ScheduleKind,
    },
};
use monocode_core::automations::{AutomationTrigger, AutomationUpsert};

fn required(field: Option<String>, name: &str) -> Result<String, ControlError> {
    field
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ControlError::invalid_input(format!("{name} is required to create an automation."))
        })
}

fn trigger(schedule: &Schedule) -> AutomationTrigger {
    let kind = match schedule.kind {
        ScheduleKind::Hourly => "hourly",
        ScheduleKind::Daily => "daily",
        ScheduleKind::Weekdays => "weekdays",
        ScheduleKind::Weekly => "weekly",
    };
    let mut trigger = AutomationTrigger::new("time", kind);
    if let Some(minute) = schedule.minute {
        trigger.minute = minute;
    }
    if let Some(time) = &schedule.time {
        trigger.time.clone_from(time);
    }
    if let Some(day) = schedule.day_of_week {
        trigger.day_of_week = day;
    }
    trigger
}

/// Applies `input` to `draft`, checking what the service does not.
fn apply(draft: &mut AutomationUpsert, input: AutomationUpsertInput) -> Result<(), ControlError> {
    if let Some(name) = input.name {
        if name.trim().is_empty() || name.trim().chars().count() > MAX_NAME_CHARS {
            return Err(ControlError::invalid_input(format!(
                "name must be 1 to {MAX_NAME_CHARS} characters"
            )));
        }
        draft.name = name;
    }
    if let Some(prompt) = input.prompt {
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(ControlError::invalid_input(format!(
                "The prompt is {} bytes; at most {MAX_PROMPT_BYTES} are accepted.",
                prompt.len()
            )));
        }
        draft.prompt = prompt;
    }
    if let Some(harness) = input.harness {
        if Kind::parse(&harness).is_none() {
            return Err(ControlError::invalid_input(format!(
                "{harness} is not a harness MonoCode runs; see model_list."
            )));
        }
        draft.harness = harness;
    }
    if let Some(model) = input.model {
        draft.model = model;
    }
    if let Some(runtime) = input.runtime_mode {
        draft.runtime_mode = runtime.id().to_owned();
    }
    match input.workspace_mode {
        Some(AutomationWorkspace::Current) => draft.workspace_mode = "current".into(),
        Some(AutomationWorkspace::Worktree) => draft.workspace_mode = "worktree".into(),
        Some(AutomationWorkspace::Existing) if draft.workspace_mode != "existing" => {
            return Err(ControlError::invalid_input(
                "An existing worktree is chosen in MonoCode's automation editor.",
            ));
        }
        Some(AutomationWorkspace::Existing) | None => {}
    }
    if let Some(schedules) = input.schedules {
        if schedules.len() > MAX_SCHEDULES {
            return Err(ControlError::invalid_input(format!(
                "At most {MAX_SCHEDULES} schedules are accepted."
            )));
        }
        let triggers = draft.triggers.get_or_insert_with(Vec::new);
        triggers.retain(|t| t.kind != "time");
        triggers.extend(schedules.iter().map(trigger));
    }
    if let Some(enabled) = input.enabled {
        draft.enabled = enabled;
    }
    Ok(())
}

impl Shell {
    pub(super) fn control_automation_upsert(
        &mut self,
        mut input: AutomationUpsertInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let project = match &input.project {
            Some(project) => self.control_local_project(Some(project)).map(Some),
            None if input.automation_id.is_none() => self.control_local_project(None).map(Some),
            None => Ok(None),
        };
        let project = project?;
        let service = self.automation_service(cx)?;
        let work = cx.background_executor().spawn(async move {
            let (mut draft, created) = match input.automation_id.take() {
                Some(id) => (
                    AutomationUpsert::from_automation(&find(&service, &id)?),
                    false,
                ),
                None => {
                    let harness = required(input.harness.take(), "harness")?;
                    let model = required(input.model.take(), "model")?;
                    let mut draft =
                        AutomationUpsert::new(project.clone().unwrap_or_default(), harness, model);
                    draft.name = required(input.name.clone(), "name")?;
                    required(input.prompt.clone(), "prompt")?;
                    draft.runtime_mode = AccessMode::Supervised.id().to_owned();
                    (draft, true)
                }
            };
            if let Some(project) = project {
                draft.cwd = project;
            }
            apply(&mut draft, input)?;
            allowed(mode(&draft.runtime_mode))?;
            draft.sync_triggers(now_ms());
            let saved = service
                .lock()
                .map_err(|_| ControlError::internal("Automations are unavailable"))?
                .upsert(draft)
                .map_err(ControlError::invalid_input)?;
            output(&AutomationUpsertOutput {
                automation: summary(&saved),
                created,
            })
        });
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.refresh_automations(cx));
            result
        })))
    }
}

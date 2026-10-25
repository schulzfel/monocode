//! `automation_list`, `automation_read`, `automation_run`,
//! `automation_set_enabled` and `automation_delete`, through the
//! automations service the Automations view uses, on a worker. Running or
//! turning on never takes full access (7.4). `automation_upsert` lives in
//! `automation_edit`.
use super::*;
use crate::shell::automation_runs::ServiceHandle;
use monocode_control::{
    AccessMode,
    ops::{
        self,
        automation::{
            AutomationDeleteOutput, AutomationListInput, AutomationListOutput, AutomationReadInput,
            AutomationReadOutput, AutomationRunInfo, AutomationRunOutput,
            AutomationSetEnabledInput, AutomationSummary, AutomationWorkspace,
            DEFAULT_AUTOMATIONS_PER_PAGE, DEFAULT_RUNS, MAX_AUTOMATIONS_PER_PAGE, MAX_PROMPT_CHARS,
            MAX_RUNS, Schedule, ScheduleKind,
        },
        session::Text,
    },
};
use monocode_core::{
    automations::{Automation, DueAutomationRun, Service},
    files::path_key,
};

/// The access mode an automation's sessions get; an unknown mode counts as
/// full access, so it is never run from here.
pub(in crate::shell::control) fn mode(id: &str) -> AccessMode {
    AccessMode::from_id(id).unwrap_or(AccessMode::FullAccess)
}

/// Whether a client may run, turn on or save an automation with `mode`:
/// never with full access.
pub(in crate::shell::control) fn allowed(mode: AccessMode) -> Result<(), ControlError> {
    if mode == AccessMode::FullAccess {
        return Err(ControlError::invalid_input(
            "Automations with full access cannot be run, turned on or saved over external control; change them in MonoCode.",
        ));
    }
    Ok(())
}

pub(in crate::shell::control) fn summary(automation: &Automation) -> AutomationSummary {
    AutomationSummary {
        id: automation.id.clone(),
        name: automation.name.clone(),
        project: automation.cwd.clone(),
        harness: automation.harness.clone(),
        model: automation.model.clone(),
        runtime_mode: mode(&automation.runtime_mode),
        enabled: automation.enabled,
        trigger: automation.trigger_label(),
        next_run_at: automation.enabled.then_some(automation.next_run_at),
        last_run_at: automation.last_run_at,
        last_run_status: automation.last_run_status.clone(),
    }
}

fn lock(service: &ServiceHandle) -> Result<std::sync::MutexGuard<'_, Service>, ControlError> {
    service
        .lock()
        .map_err(|_| ControlError::internal("Automations are unavailable"))
}

fn store_error(error: String) -> ControlError {
    if error.contains("not found") {
        ControlError::new(ErrorCode::NotFound, error)
    } else {
        ControlError::invalid_input(error)
    }
}

/// The automation called `id`.
pub(in crate::shell::control) fn find(
    service: &ServiceHandle,
    id: &str,
) -> Result<Automation, ControlError> {
    lock(service)?
        .list()
        .map_err(ControlError::internal)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::NotFound,
                format!("There is no automation {id}; see automation_list."),
            )
        })
}

fn schedules(automation: &Automation) -> (Vec<Schedule>, Vec<String>) {
    let mut schedules = Vec::new();
    let mut events = Vec::new();
    for trigger in automation.triggers() {
        if trigger.kind != "time" {
            events.push(trigger.label());
            continue;
        }
        let kind = match trigger.schedule_kind.as_str() {
            "hourly" => ScheduleKind::Hourly,
            "daily" => ScheduleKind::Daily,
            "weekly" => ScheduleKind::Weekly,
            _ => ScheduleKind::Weekdays,
        };
        schedules.push(Schedule {
            kind,
            minute: (kind == ScheduleKind::Hourly).then_some(trigger.minute),
            time: (kind != ScheduleKind::Hourly).then(|| trigger.time.clone()),
            day_of_week: (kind == ScheduleKind::Weekly).then_some(trigger.day_of_week),
        });
    }
    (schedules, events)
}

fn read(
    automation: &Automation,
    runs: Vec<monocode_core::automations::AutomationRun>,
    limit: usize,
) -> AutomationReadOutput {
    let (schedules, events) = schedules(automation);
    let omitted = runs.len().saturating_sub(limit);
    AutomationReadOutput {
        automation: summary(automation),
        prompt: Text {
            text: automation.prompt.chars().take(MAX_PROMPT_CHARS).collect(),
            truncated: automation.prompt.chars().count() > MAX_PROMPT_CHARS,
        },
        workspace_mode: match automation.workspace_mode.as_str() {
            "current" => AutomationWorkspace::Current,
            "existing" => AutomationWorkspace::Existing,
            _ => AutomationWorkspace::Worktree,
        },
        schedules,
        events,
        runs: runs
            .into_iter()
            .take(limit)
            .map(|run| AutomationRunInfo {
                id: run.id,
                trigger: run.trigger,
                status: run.status,
                scheduled_for: run.scheduled_for,
                started_at: run.started_at,
                completed_at: run.completed_at,
                error: run.error,
            })
            .collect(),
        runs_omitted: u32::try_from(omitted).unwrap_or(u32::MAX),
    }
}

impl Shell {
    /// The automations service, once the app has opened it.
    pub(in crate::shell::control) fn automation_service(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<ServiceHandle, ControlError> {
        if let Some(service) = self.automation_state.service.clone() {
            return Ok(service);
        }
        self.initialize_automations(cx);
        Err(ControlError::internal("Automations are still opening; try again.").retryable(true))
    }

    /// Runs `work` against the service on a worker, then refreshes the
    /// Automations view.
    fn automation_work(
        &mut self,
        work: impl FnOnce(&ServiceHandle) -> Result<Value, ControlError> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let service = self.automation_service(cx)?;
        let work = cx
            .background_executor()
            .spawn(async move { work(&service) });
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.refresh_automations(cx));
            result
        })))
    }

    pub(in crate::shell::control) fn refresh_automations(&self, cx: &mut Context<Self>) {
        if let Some(view) = &self.automations {
            view.update(cx, |view, cx| view.refresh(cx));
        }
    }

    pub(super) fn control_automation_list(
        &mut self,
        input: AutomationListInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_AUTOMATIONS_PER_PAGE,
            MAX_AUTOMATIONS_PER_PAGE,
        )?;
        let project = input.project.as_deref().map(path_key);
        self.automation_work(
            move |service| {
                let all: Vec<_> = lock(service)?
                    .list()
                    .map_err(ControlError::internal)?
                    .iter()
                    .filter(|a| project.as_ref().is_none_or(|p| path_key(&a.cwd) == *p))
                    .map(summary)
                    .collect();
                let (automations, next_cursor) = ops::page(&all, input.cursor.as_deref(), limit)?;
                output(&AutomationListOutput {
                    automations,
                    next_cursor,
                })
            },
            cx,
        )
    }

    pub(super) fn control_automation_read(
        &mut self,
        input: AutomationReadInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(input.runs, DEFAULT_RUNS, MAX_RUNS)?;
        self.automation_work(
            move |service| {
                let automation = find(service, &input.automation_id)?;
                let runs = lock(service)?.runs(&automation.id).map_err(store_error)?;
                output(&read(&automation, runs, limit))
            },
            cx,
        )
    }

    pub(super) fn control_automation_run(
        &mut self,
        id: String,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let service = self.automation_service(cx)?;
        let work = cx.background_executor().spawn(async move {
            let automation = find(&service, &id)?;
            allowed(mode(&automation.runtime_mode))?;
            let run = lock(&service)?
                .run_now(&automation.id, now_ms())
                .map_err(store_error)?;
            Ok::<_, ControlError>(DueAutomationRun { automation, run })
        });
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let due = work.await?;
            let out = AutomationRunOutput {
                automation_id: due.automation.id.clone(),
                run_id: due.run.id.clone(),
            };
            this.update(cx, |this, cx| {
                this.launch_automation(due, false, cx);
                this.refresh_automations(cx);
            })
            .map_err(|_| ControlError::internal("The window closed"))?;
            output(&out)
        })))
    }

    pub(super) fn control_automation_set_enabled(
        &mut self,
        input: AutomationSetEnabledInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        self.automation_work(
            move |service| {
                let automation = find(service, &input.automation_id)?;
                if input.enabled {
                    allowed(mode(&automation.runtime_mode))?;
                }
                let saved = lock(service)?
                    .set_enabled(&automation.id, input.enabled)
                    .map_err(store_error)?;
                output(&summary(&saved))
            },
            cx,
        )
    }

    pub(super) fn control_automation_delete(
        &mut self,
        id: String,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        self.automation_work(
            move |service| {
                let automation = find(service, &id)?;
                lock(service)?.delete(&automation.id).map_err(store_error)?;
                output(&AutomationDeleteOutput {
                    automation_id: automation.id,
                    name: automation.name,
                })
            },
            cx,
        )
    }
}

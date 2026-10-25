//! Executors: each command runs against the shell's own state, one file
//! per domain.
mod app;
pub(super) mod automation;
mod automation_edit;
pub(super) mod changes;
mod configure;
pub(super) mod discard;
mod dispatch;
mod layout;
mod lifecycle;
pub(super) mod model;
mod mono;
mod notes;
mod organize;
mod project;
pub(super) mod question;
mod queue;
mod rail;
mod search;
mod session;
pub(super) mod settings;
mod tab;
mod topic;
pub(super) mod vcs;
mod workspace;
mod worktree;

use super::*;
use monocode_control::Command;
use monocode_core::remote::is_remote_path;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

/// An operation's result, now or once background work finishes. Executors
/// return `Result<Exec, ControlError>`, so a refusal before any work starts
/// is `?`; [`Shell::exec_control`] answers it.
pub(super) enum Exec {
    Ready(Result<Value, ControlError>),
    Later(gpui::Task<Result<Value, ControlError>>),
}

/// An output as the JSON the wire carries.
pub(super) fn output<T: Serialize>(output: &T) -> Result<Value, ControlError> {
    serde_json::to_value(output).map_err(|e| ControlError::internal(e.to_string()))
}

/// A result that is ready now.
fn ready<T: Serialize>(result: Result<T, ControlError>) -> Result<Exec, ControlError> {
    Ok(Exec::Ready(Ok(output(&result?)?)))
}

/// Who calls, for operations that record it.
pub(super) struct Caller<'a> {
    /// Who the call is from.
    pub(super) caller: &'a CallerKind,
    /// The call's request ID, when it has one.
    pub(super) request_id: Option<&'a str>,
    /// What the person confirmed, for a call they confirm first.
    pub(super) pin: Option<&'a vcs::Pin>,
}

impl Caller<'_> {
    /// What makes a repeat of this call find what its first run created:
    /// `control:<caller>:<request id>`, or a fresh key for a call without
    /// a request ID, which creates new work every time.
    pub(super) fn request_key(&self) -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let caller = self.caller.name();
        self.request_id.map_or_else(
            || {
                format!(
                    "control:{caller}:auto-{}-{}",
                    now_ms(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )
            },
            |id| format!("control:{caller}:{id}"),
        )
    }
}

impl Shell {
    pub(super) fn exec_control(
        &mut self,
        grant: &Grant,
        caller: &Caller,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Exec {
        self.exec_command(grant, caller, command, window, cx)
            .unwrap_or_else(|error| Exec::Ready(Err(error)))
    }

    fn exec_command(
        &mut self,
        grant: &Grant,
        caller: &Caller,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        match command {
            Command::AppInfo(_) => ready(Ok(self.control_app_info(grant))),
            Command::ModelList(input) => self.control_model_list(input, cx),
            Command::ProjectList(input) => ready(self.control_project_list(&input)),
            Command::ProjectSelect(input) => ready(self.control_project_select(&input, window, cx)),
            Command::ProjectAdd(input) => ready(self.control_project_add(&input, window, cx)),
            Command::WorkspaceRead(_) => ready(Ok(self.control_workspace(cx))),
            Command::SessionList(input) => ready(self.control_session_list(&input, cx)),
            Command::SessionRead(input) => self.control_session_read(input, cx),
            Command::SessionStatus(input) => self.control_session_status(input.session_id, cx),
            Command::SessionStart(input) => self.control_session_start(&input, caller, cx),
            Command::SessionSend(input) => ready(self.control_session_send(input, cx)),
            Command::SessionWait(input) => self.control_session_wait(&input, cx),
            Command::SessionStop(input) => ready(self.control_session_stop(input.session_id, cx)),
            Command::TabOpen(input) => ready(self.control_tab_open(&input, window, cx)),
            Command::TabFocus(input) => ready(self.control_tab_focus(&input, window, cx)),
            Command::TabClose(input) => ready(self.control_tab_close(&input, window, cx)),
            Command::SessionConfigure(input) => ready(self.control_session_configure(input, cx)),
            Command::SessionDraft(input) => ready(self.control_session_draft(&input, cx)),
            Command::SessionRename(input) => ready(self.control_session_rename(&input, cx)),
            Command::SessionArchive(input) => {
                self.control_session_archive(input.session_id, true, cx)
            }
            Command::SessionRestore(input) => {
                self.control_session_archive(input.session_id, false, cx)
            }
            Command::FolderList(input) => ready(self.control_folder_list(&input)),
            Command::SessionMove(input) => self.control_session_move(input, cx),
            Command::QuestionList(input) => ready(self.control_question_list(&input, cx)),
            Command::QuestionAnswer(input) => ready(self.control_question_answer(&input, cx)),
            Command::QueueList(input) => ready(self.control_queue_list(input.session_id, cx)),
            Command::QueueRemove(input) => ready(self.control_queue_remove(&input, cx)),
            Command::QueueSteer(input) => ready(self.control_queue_steer(&input, cx)),
            Command::QueueResume(input) => ready(self.control_queue_resume(input.session_id, cx)),
            Command::QueueEdit(input) => ready(self.control_queue_edit(input, cx)),
            Command::WorktreeList(input) => self.control_worktree_list(&input, cx),
            Command::WorktreeCreate(input) => self.control_worktree_create(input, cx),
            Command::ProjectUpdate(input) => ready(self.control_project_update(&input, cx)),
            Command::ProjectArchive(input) => {
                ready(self.control_project_archive(&input, window, cx))
            }
            Command::ProjectUnarchive(input) => ready(self.control_project_unarchive(&input, cx)),
            Command::ProjectReorder(input) => ready(self.control_project_reorder(&input, cx)),
            Command::TabMove(input) => ready(self.control_tab_move(&input, window, cx)),
            Command::PaneSplit(input) => ready(self.control_pane_split(&input, window, cx)),
            Command::LayoutSet(input) => ready(Ok(self.control_layout_set(&input, window, cx))),
            Command::SettingsRead(_) => ready(Ok(self.control_settings_read())),
            Command::SettingsUpdate(input) => ready(self.control_settings_update(input, cx)),
            Command::SearchSessions(input) => self.control_search_sessions(&input, cx),
            Command::SearchProject(input) => self.control_search_project(input, cx),
            Command::NotesList(input) => self.control_notes_list(&input, cx),
            Command::NotesRead(input) => self.control_notes_read(&input, cx),
            Command::NotesWrite(input) => self.control_notes_write(&input, caller, cx),
            Command::ChangesRead(input) => self.control_changes_read(&input, cx),
            Command::SessionCompact(input) => {
                ready(self.control_session_compact(input.session_id, cx))
            }
            Command::SessionHandoff(input) => self.control_session_handoff(input, caller, cx),
            Command::SessionDelete(input) => {
                self.control_session_delete(input.session_id, window, cx)
            }
            Command::ProjectDelete(input) => self.control_project_delete(&input, window, cx),
            Command::GitStage(input) => self.control_git_stage(&input, cx),
            Command::GitUnstage(input) => self.control_git_unstage(&input, cx),
            Command::GitCommit(input) => self.control_git_commit(input, cx),
            Command::GitDiscard(input) => self.control_git_discard(&input, caller.pin, cx),
            Command::GitPush(input) => self.control_git_push(&input, caller.pin, cx),
            Command::AutomationList(input) => self.control_automation_list(input, cx),
            Command::AutomationRead(input) => self.control_automation_read(input, cx),
            Command::AutomationRun(input) => self.control_automation_run(input.automation_id, cx),
            Command::AutomationSetEnabled(input) => self.control_automation_set_enabled(input, cx),
            Command::AutomationUpsert(input) => self.control_automation_upsert(input, cx),
            Command::AutomationDelete(input) => {
                self.control_automation_delete(input.automation_id, cx)
            }
            Command::MonoList(_) => ready(self.control_mono_list(cx)),
            Command::MonoOpen(input) => ready(self.control_mono_open(&input.mono_id, window, cx)),
            Command::MonoSend(input) => ready(self.control_mono_send(input, cx)),
        }
    }
}

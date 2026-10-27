//! What an agent session's call means where it leaves something out: as for
//! Operator, an absent `project` is the session's own project, not the
//! window's current one (`Operation::project_slot`). A Mono's is its one
//! assigned project, and a project it names is the assigned project that
//! path or name picks (`Mono::project`); a new note it writes without one
//! belongs to its first assigned project, or to none when it has none, as
//! its old `notes.write` did.
//! External clients keep the current project. Defaults rewrite the call
//! before its receipt is looked up, so the receipt and its fingerprint record
//! the call as it ran.
use super::*;
use monocode_control::Command;
use monocode_core::mono::Mono;

/// The assigned project of `mono` that `project` names, or its only one
/// (`Mono::project`), refused in the bridge's words: a project it does not
/// work on is out of its scope; none or several to choose from is a call
/// that must name one.
pub(super) fn mono_project(mono: &Mono, project: Option<&str>) -> Result<String, ControlError> {
    mono.project(project).map_err(|error| {
        let code = if project.is_some() {
            ErrorCode::ScopeDenied
        } else {
            ErrorCode::InvalidInput
        };
        ControlError::new(code, error)
    })
}

impl Shell {
    /// Fills in what `call` leaves out for its caller; a Mono without a
    /// project to fill in is refused, in the bridge's words (MO-15).
    pub(super) fn control_defaults(
        &self,
        call: &mut Pending,
        cx: &App,
    ) -> Result<(), ControlError> {
        if let CallerKind::Mono { mono, .. } = &call.caller {
            let Some(mono) = self.caller_mono(mono, cx) else {
                return Err(ControlError::new(ErrorCode::NotFound, caller::MONO_GONE));
            };
            // The old notes.write put a new note in the first assigned
            // project, however many the Mono has, and in none when it has
            // none (`write_note` with the Mono's projects).
            let note = matches!(call.command, Command::NotesWrite(_));
            if let Some(slot) = call.command.project_slot() {
                *slot = match (note, slot.as_deref(), mono.projects.first()) {
                    (true, None, first) => first.cloned(),
                    (_, named, _) => Some(mono_project(mono, named)?),
                };
            }
            return Ok(());
        }
        let Some(project) = self.caller_project(&call.caller, cx) else {
            return Ok(());
        };
        if let Some(slot) = call.command.project_slot().filter(|slot| slot.is_none()) {
            *slot = Some(project);
        }
        Ok(())
    }

    /// The project a caller's calls act on by default: a session caller's
    /// session's project; `None` for an external client, whose default is
    /// the window's current project, and for a Mono, whose default is its
    /// assigned project ([`Self::control_defaults`]).
    pub(super) fn caller_project(&self, caller: &CallerKind, cx: &App) -> Option<String> {
        match caller {
            CallerKind::Session(id) if self.session_exists(*id).is_ok() => {
                Some(self.session_metadata(*id, cx).summary.cwd.clone())
            }
            CallerKind::Session(_) | CallerKind::Mono { .. } | CallerKind::External(_) => None,
        }
    }

    /// Mono `mono`, while it exists.
    pub(super) fn caller_mono<'a>(&'a self, mono: &str, cx: &'a App) -> Option<&'a Mono> {
        self.monos.as_ref()?.read(cx).roster.get(mono)
    }
}

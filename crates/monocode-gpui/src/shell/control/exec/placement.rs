//! Where `session_start` and `tab_open` put a session, as Operator placed
//! its launches, with typed options and projects compared by `path_key`.
use super::*;
use monocode_control::ops::dispatch::Placement;
use monocode_core::files::path_key;

/// The pane a placement names is not an open session pane of the project.
fn not_open() -> ControlError {
    ControlError::invalid_input("besideSessionId must be an open session pane in this project")
}

impl Shell {
    /// A new tab (`None`), or right of or below `beside`, else `default`:
    /// an open session pane of `project`.
    pub(super) fn control_placement(
        &self,
        placement: Option<Placement>,
        beside: Option<i64>,
        default: Option<i64>,
        project: &str,
        cx: &App,
    ) -> Result<Option<(i64, Edge)>, ControlError> {
        let edge = match placement.unwrap_or_default() {
            Placement::Tab => return Ok(None),
            Placement::Right => Edge::Right,
            Placement::Down => Edge::Bottom,
        };
        let id = beside.or(default).ok_or_else(not_open)?;
        let known = self.sessions.contains_key(&id) || self.session_summaries.contains_key(&id);
        if self.workspace.tab_of(id).is_none()
            || !known
            || path_key(&self.session_metadata(id, cx).summary.cwd) != path_key(project)
        {
            return Err(not_open());
        }
        Ok(Some((id, edge)))
    }

    /// The window's focused session pane when it is in `project` and not
    /// `except`.
    pub(super) fn control_focused_pane(
        &self,
        project: &str,
        except: Option<i64>,
        cx: &App,
    ) -> Option<i64> {
        self.workspace
            .active
            .filter(|pane| Some(*pane) != except && self.session_summaries.contains_key(pane))
            .filter(|pane| {
                path_key(&self.session_metadata(*pane, cx).summary.cwd) == path_key(project)
            })
    }
}

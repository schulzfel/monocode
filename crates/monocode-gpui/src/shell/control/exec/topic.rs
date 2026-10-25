//! What a followed topic shows right now, reduced to a fingerprint, so a
//! change that leaves the topic's read unchanged sends no event; and whether
//! a topic names something that exists.
use super::*;
use monocode_control::{
    Topic,
    ops::{project::ProjectListInput, session::SessionListInput},
};
use std::hash::{Hash, Hasher};

/// The page size the projects and project-sessions resources read.
const RESOURCE_PAGE: u32 = 200;

fn fingerprint(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// A read's JSON, or its error, as one comparable string.
fn shown<T: serde::Serialize>(read: Result<T, ControlError>) -> String {
    match read {
        Ok(output) => serde_json::to_string(&output).unwrap_or_default(),
        Err(error) => error.to_string(),
    }
}

impl Shell {
    /// Checks that `topic` names a project or session clients can read.
    pub(in crate::shell::control) fn control_topic_target(
        &self,
        topic: &Topic,
        cx: &App,
    ) -> Result<(), ControlError> {
        match topic {
            Topic::Workspace | Topic::Projects => Ok(()),
            Topic::ProjectSessions { project } => self.control_project(Some(project)).map(|_| ()),
            Topic::Session { id } | Topic::SessionTranscript { id } => {
                self.control_session(*id, cx)
            }
        }
    }

    /// A fingerprint of what reading `topic` shows now. `None` when that
    /// cannot be told cheaply: the topic then counts as changed.
    pub(in crate::shell::control) fn control_fingerprint(
        &self,
        topic: &Topic,
        cx: &App,
    ) -> Option<u64> {
        let page = Some(RESOURCE_PAGE);
        match topic {
            Topic::Workspace => Some(fingerprint(shown(Ok(self.control_workspace(cx))))),
            Topic::Projects => Some(fingerprint(shown(self.control_project_list(
                &ProjectListInput {
                    limit: page,
                    ..ProjectListInput::default()
                },
            )))),
            Topic::ProjectSessions { project } => {
                let input = SessionListInput {
                    project: Some(project.clone()),
                    limit: page,
                    ..SessionListInput::default()
                };
                Some(fingerprint(shown(self.control_session_list(&input, cx))))
            }
            Topic::Session { id } => {
                let read = self
                    .control_session(*id, cx)
                    .map(|()| self.control_status_now(*id, cx).0);
                Some(fingerprint(shown(read)))
            }
            Topic::SessionTranscript { id } => {
                // The transcript is read from the store; the live rows show
                // when it grows or streams.
                let view = self.sessions.get(id)?.read(cx);
                let rows = view.transcript.read(cx).source_rows();
                let tail: Vec<(&str, &str)> = rows[rows.len().saturating_sub(2)..]
                    .iter()
                    .map(|row| (row.role.as_str(), row.text.as_str()))
                    .collect();
                Some(fingerprint((view.busy, rows.len(), tail)))
            }
        }
    }
}

//! What the person is asked before a confirmed call runs: the client, the
//! exact target, and what will happen to it. A call's target is checked
//! first (`control_check`): it must exist and the call must be allowed to
//! touch it, and a discard or push pins what it showed, so the call acts on
//! nothing else. Only a call the person is asked about gets its prompt text
//! (`question`).
use super::exec::{automation, discard, vcs};
use super::*;
use monocode_control::{
    Command,
    ops::{queue::QueueItemInput, vcs::GitPathsInput},
};

/// A confirmed call's target, checked.
pub(super) struct Checked {
    /// What the activity log names.
    pub(super) target: String,
    /// What the prompt says about it.
    subject: Subject,
}

/// What a prompt names, as the check found it.
enum Subject {
    Session {
        title: String,
    },
    Project {
        label: String,
        path: String,
        sessions: usize,
    },
    Queued {
        title: String,
        text: String,
    },
    Discard {
        label: String,
        plan: discard::Plan,
    },
    Push {
        label: String,
        to: String,
        branch: String,
        head: String,
    },
    Automation {
        name: String,
    },
    Other {
        name: &'static str,
    },
}

impl Checked {
    /// What the call may act on once confirmed: a discard its files, a push
    /// its branch at the commit it showed.
    pub(super) fn into_pin(self) -> Option<vcs::Pin> {
        match self.subject {
            Subject::Discard { plan, .. } => Some(vcs::Pin::Discard(plan)),
            Subject::Push { branch, head, .. } => Some(vcs::Pin::Push { branch, head }),
            _ => None,
        }
    }
}

/// What the person is asked.
pub(super) struct Question {
    pub(super) message: String,
    pub(super) detail: String,
    /// The confirming button.
    pub(super) action: &'static str,
}

/// The most paths a discard prompt lists by name, per kind of change.
const LISTED_PATHS: usize = 8;
/// The most characters of a queued message a prompt quotes.
const QUOTED_CHARS: usize = 200;

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// `paths` under `heading`, one per line, the rest counted.
fn listed(heading: &str, paths: &[&str]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = paths
        .iter()
        .take(LISTED_PATHS)
        .map(|p| format!("  {p}"))
        .collect();
    if paths.len() > LISTED_PATHS {
        lines.push(format!("  and {} more", paths.len() - LISTED_PATHS));
    }
    format!("{heading}\n{}\n\n", lines.join("\n"))
}

/// What to ask `client` about the call `checked` describes.
pub(super) fn question(client: &str, checked: &Checked) -> Question {
    let (message, detail, action) = match &checked.subject {
        Subject::Session { title } => (
            format!("“{client}” wants to delete the session “{title}”. Delete it?"),
            "This conversation and its saved settings will be removed permanently.".into(),
            "Delete",
        ),
        Subject::Project {
            label,
            path,
            sessions,
        } => {
            let sessions = match sessions {
                0 => String::new(),
                n => format!(
                    "Its {} will be deleted. ",
                    plural(*n, "saved conversation", "saved conversations")
                ),
            };
            (
                format!(
                    "“{client}” wants to delete the project “{label}” from MonoCode. Delete it?"
                ),
                format!(
                    "{sessions}No files are deleted: the folder {path} stays on disk, and opening it again brings the project back empty."
                ),
                "Delete",
            )
        }
        Subject::Queued { title, text } => (
            format!("“{client}” wants to remove a queued message from “{title}”. Remove it?"),
            format!("“{text}”\n\nIt will never be sent. This cannot be undone."),
            "Remove",
        ),
        Subject::Discard { label, plan } => {
            let reverted = plan.named(discard::Change::Revert);
            let deleted = plan.named(discard::Change::Delete);
            let what = match (reverted.len(), deleted.len()) {
                (n, 0) => format!("revert {}", plural(n, "file", "files")),
                (0, n) => format!("delete {}", plural(n, "untracked file", "untracked files")),
                (r, d) => format!(
                    "revert {} and delete {}",
                    plural(r, "file", "files"),
                    plural(d, "untracked file", "untracked files")
                ),
            };
            (
                format!("“{client}” wants to discard changes in “{label}”: {what}. Discard them?"),
                format!(
                    "{}{}This cannot be undone.",
                    listed("Reverted to their staged or committed version:", &reverted),
                    listed("Deleted (untracked):", &deleted)
                ),
                "Discard",
            )
        }
        Subject::Push {
            label,
            to,
            branch,
            head,
        } => (
            format!("“{client}” wants to push the branch “{branch}” of “{label}”. Push it?"),
            format!(
                "Its commits up to {} are published to {to}, where anyone with access to the remote can see them.",
                vcs::short(head)
            ),
            "Push",
        ),
        Subject::Automation { name } => (
            format!("“{client}” wants to delete the automation “{name}”. Delete it?"),
            "Its run history will be deleted too. Sessions it started stay.".into(),
            "Delete",
        ),
        Subject::Other { name } => (
            format!("“{client}” wants to run {name}. Allow it?"),
            "It may delete, discard or publish work.".into(),
            "Allow",
        ),
    };
    Question {
        message,
        detail,
        action,
    }
}

impl Shell {
    /// Checks the target of confirmed `command`, and pins what it may act on.
    pub(super) fn control_check(
        &mut self,
        command: &Command,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<Checked, ControlError>> {
        let ready = gpui::Task::ready;
        match command {
            Command::SessionDelete(input) => ready(
                self.deletable_session(input.session_id, cx)
                    .map(|title| Checked {
                        target: format!("session {}, “{title}”", input.session_id),
                        subject: Subject::Session { title },
                    }),
            ),
            Command::ProjectDelete(input) => ready(self.project_check(&input.project)),
            Command::QueueRemove(input) => ready(self.queue_check(input, cx)),
            Command::GitDiscard(input) => self.discard_check(input, cx),
            Command::GitPush(input) => self.push_check(input.project.as_deref(), cx),
            Command::AutomationDelete(input) => self.automation_check(&input.automation_id, cx),
            other => ready(Ok(Checked {
                target: other.name().into(),
                subject: Subject::Other { name: other.name() },
            })),
        }
    }

    fn project_check(&self, project: &str) -> Result<Checked, ControlError> {
        let path = self.control_local_project(Some(project))?;
        Ok(Checked {
            target: path.clone(),
            subject: Subject::Project {
                label: self.project_label(&path),
                sessions: self.project_sessions(&path).len(),
                path,
            },
        })
    }

    /// A queued message that exists and is not being sent.
    fn queue_check(&self, input: &QueueItemInput, cx: &App) -> Result<Checked, ControlError> {
        let id = input.session_id;
        let missing = || {
            ControlError::new(
                ErrorCode::NotFound,
                format!(
                    "Session {id} has no queued message {}; see queue_list.",
                    input.message_id
                ),
            )
        };
        let view = self.sessions.get(&id).ok_or_else(missing)?.read(cx);
        let (messages, _, _) = view.queued();
        let message = messages
            .iter()
            .find(|m| m.id == input.message_id)
            .ok_or_else(missing)?;
        Ok(Checked {
            target: format!("session {id}, {}", input.message_id),
            subject: Subject::Queued {
                title: self.session_metadata(id, cx).display_title(),
                text: message.label().chars().take(QUOTED_CHARS).collect(),
            },
        })
    }

    /// The files a discard would revert and delete, checked on a worker.
    fn discard_check(
        &self,
        input: &GitPathsInput,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<Checked, ControlError>> {
        let target = self
            .control_local_project(input.project.as_deref())
            .and_then(|root| Ok((root, vcs::files(input)?)));
        let (path, files) = match target {
            Ok(target) => target,
            Err(error) => return gpui::Task::ready(Err(error)),
        };
        let label = self.project_label(&path);
        cx.background_executor().spawn(async move {
            let plan = discard::plan(&path, &files)?;
            Ok(Checked {
                target: path,
                subject: Subject::Discard { label, plan },
            })
        })
    }

    /// The branch, commit and remote a push would publish to.
    fn push_check(
        &self,
        project: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<Checked, ControlError>> {
        let path = match self.control_local_project(project) {
            Ok(path) => path,
            Err(error) => return gpui::Task::ready(Err(error)),
        };
        let label = self.project_label(&path);
        cx.background_executor().spawn(async move {
            let (branch, head, to) = vcs::push_target(&path)?;
            Ok(Checked {
                target: format!("{path}, {branch}"),
                subject: Subject::Push {
                    label,
                    to,
                    branch,
                    head,
                },
            })
        })
    }

    fn automation_check(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<Checked, ControlError>> {
        let service = match self.automation_service(cx) {
            Ok(service) => service,
            Err(error) => return gpui::Task::ready(Err(error)),
        };
        let id = id.to_owned();
        cx.background_executor().spawn(async move {
            let found = automation::find(&service, &id)?;
            Ok(Checked {
                target: format!("automation {id}, “{}”", found.name),
                subject: Subject::Automation { name: found.name },
            })
        })
    }
}

//! Organising sessions: titles, the archive and sidebar folders.
use crate::{Hints, Operation, Scope, ops::session::SessionInput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest session title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// The longest folder name accepted, in characters.
pub const MAX_FOLDER_NAME_CHARS: usize = 100;

/// `session_rename`: a session's title.
#[derive(Debug)]
pub struct SessionRename;

/// The session and its new title.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRenameInput {
    /// The session's id.
    pub session_id: i64,
    /// The title (1 to 200 characters).
    pub title: String,
}

/// The renamed session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionRenameOutput {
    /// The session's id.
    pub session_id: i64,
    /// Its title now.
    pub title: String,
}

impl Operation for SessionRename {
    const NAME: &'static str = "session_rename";
    const TITLE: &'static str = "Rename session";
    const SUBJECT: &'static [&'static str] = &["title", "sessionId"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Rename a session in the sidebar and its tab.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const LOCAL_ONLY: bool = true;
    type Input = SessionRenameInput;
    type Output = SessionRenameOutput;

    fn target_session(input: &SessionRenameInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionRenameOutput) -> String {
        format!("Session {} is now “{}”", output.session_id, output.title)
    }
}

/// A session's archive state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionArchiveOutput {
    /// The session's id.
    pub session_id: i64,
    /// Whether it is archived now.
    pub archived: bool,
}

fn archive_summary(output: &SessionArchiveOutput) -> String {
    format!(
        "Session {} {}",
        output.session_id,
        if output.archived {
            "archived"
        } else {
            "restored"
        }
    )
}

/// `session_archive`: move a session to the archive.
#[derive(Debug)]
pub struct SessionArchive;

/// Which session, and whether to stop it first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionArchiveInput {
    /// The session's id.
    pub session_id: i64,
    /// Stop its running turn and pause its queue first, so a working
    /// session is archived too instead of refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<bool>,
}

impl Operation for SessionArchive {
    const NAME: &'static str = "session_archive";
    const TITLE: &'static str = "Archive session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Archive an idle session: its pane closes and it leaves the sidebar. A session that is working is refused with session_busy; stop it with session_stop first, or pass `stop` to stop it and archive it in one call. Nothing is deleted; session_restore brings it back.";
    const HINTS: Hints = Hints::DISCARD;
    const NOT_ON_SELF: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = SessionArchiveInput;
    type Output = SessionArchiveOutput;

    fn target_session(input: &SessionArchiveInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionArchiveOutput) -> String {
        archive_summary(output)
    }
}

/// `session_restore`: bring a session back from the archive.
#[derive(Debug)]
pub struct SessionRestore;

impl Operation for SessionRestore {
    const NAME: &'static str = "session_restore";
    const TITLE: &'static str = "Restore session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Restore an archived session to its project's sidebar.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const LOCAL_ONLY: bool = true;
    type Input = SessionInput;
    type Output = SessionArchiveOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionArchiveOutput) -> String {
        archive_summary(output)
    }
}

/// A project to act on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectInput {
    /// A local project's path or key; when absent, the calling agent
    /// session's project, or for other clients the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// A sidebar folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Its id, for `session_move`.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The sessions in it, in order.
    pub session_ids: Vec<i64>,
}

/// `folder_list`: a project's sidebar folders.
#[derive(Debug)]
pub struct FolderList;

/// A project's folders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderListOutput {
    /// The project.
    pub project: String,
    /// Its folders, in sidebar order.
    pub folders: Vec<Folder>,
}

impl Operation for FolderList {
    const NAME: &'static str = "folder_list";
    const TITLE: &'static str = "List folders";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str =
        "List a project's sidebar folders and the sessions in each, for session_move.";
    const HINTS: Hints = Hints::READ;
    type Input = ProjectInput;
    type Output = FolderListOutput;

    fn project_slot(input: &mut ProjectInput) -> Option<&mut Option<String>> {
        Some(&mut input.project)
    }

    fn summary(output: &FolderListOutput) -> String {
        format!("{} folders in {}", output.folders.len(), output.project)
    }
}

/// `session_move`: put a session in a sidebar folder.
#[derive(Debug)]
pub struct SessionMove;

/// The session and where it goes: exactly one of `folderId`,
/// `newFolderName` and `removeFromFolder`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionMoveInput {
    /// The session's id.
    pub session_id: i64,
    /// An existing folder of the session's project, from `folder_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// A new folder to create for it (1 to 100 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_folder_name: Option<String>,
    /// Take it out of its folder instead; a folder left empty goes too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_from_folder: Option<bool>,
}

/// Where the session is now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionMoveOutput {
    /// The session's id.
    pub session_id: i64,
    /// Its folder's id; absent once it was taken out of its folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// Its folder's name; absent once it was taken out of its folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_name: Option<String>,
}

impl Operation for SessionMove {
    const NAME: &'static str = "session_move";
    const TITLE: &'static str = "Move session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Move a session into a sidebar folder of its project: an existing one by folderId, or a new one named newFolderName. removeFromFolder takes it out of its folder instead. A new name creates another folder each time unless the call carries a requestId.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = SessionMoveInput;
    type Output = SessionMoveOutput;

    fn target_session(input: &SessionMoveInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionMoveOutput) -> String {
        match &output.folder_name {
            Some(name) => format!("Session {} is in “{name}”", output.session_id),
            None => format!("Session {} is in no folder", output.session_id),
        }
    }
}

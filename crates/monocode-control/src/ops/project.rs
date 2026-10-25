//! Projects in the rail.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `project_list`: projects in rail order, then archived ones.
#[derive(Debug)]
pub struct ProjectList;

/// Projects per page unless `limit` says otherwise.
pub const DEFAULT_PROJECTS_PER_PAGE: u32 = 50;
/// The largest `limit` accepted.
pub const MAX_PROJECTS_PER_PAGE: u32 = 200;

/// Paging for `project_list`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectListInput {
    /// `nextCursor` from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Projects per page (1 to 200, default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Where a project's files are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLocation {
    /// A folder on this computer.
    Local,
    /// A folder on a connected machine (`remote://…`).
    Remote,
}

/// One project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// The project's path, as the app stores it.
    pub path: String,
    /// The normalized path the app compares projects by.
    pub key: String,
    /// The display name.
    pub name: String,
    /// The project color as `#rrggbb`.
    pub color: String,
    /// Whether it is the project shown in the window.
    pub current: bool,
    /// Whether it was archived from the rail.
    pub archived: bool,
    /// Where its files are.
    pub location: ProjectLocation,
}

/// One page of projects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListOutput {
    /// Projects on this page, rail order first, then archived.
    pub projects: Vec<ProjectSummary>,
    /// How many projects there are in all.
    pub total: u32,
    /// Pass as `cursor` for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Operation for ProjectList {
    const NAME: &'static str = "project_list";
    const TITLE: &'static str = "List projects";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List MonoCode's projects in rail order, then archived ones, with each project's name, color, whether it is current, and whether it is local or on a remote machine. Paged with `cursor` and `limit`.";
    const HINTS: Hints = Hints::READ;
    type Input = ProjectListInput;
    type Output = ProjectListOutput;

    fn summary(output: &ProjectListOutput) -> String {
        let current = output
            .projects
            .iter()
            .find(|p| p.current)
            .map_or(String::new(), |p| format!("; current: {}", p.name));
        let more = if output.next_cursor.is_some() {
            "; more with nextCursor"
        } else {
            ""
        };
        format!(
            "{} of {} projects{current}{more}",
            output.projects.len(),
            output.total
        )
    }
}

/// `project_select`: make a project the current one.
#[derive(Debug)]
pub struct ProjectSelect;

/// Which project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSelectInput {
    /// A project's path or key, as `project_list` reports it.
    pub project: String,
}

/// The current project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSelectOutput {
    /// The project's path.
    pub project: String,
    /// Whether it was not current before.
    pub changed: bool,
}

impl Operation for ProjectSelect {
    const NAME: &'static str = "project_select";
    const TITLE: &'static str = "Select project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Make a local project in MonoCode's rail the current one, as clicking it does.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectSelectInput;
    type Output = ProjectSelectOutput;

    fn summary(output: &ProjectSelectOutput) -> String {
        format!(
            "{} {}",
            if output.changed {
                "Switched to"
            } else {
                "Already in"
            },
            output.project
        )
    }
}

/// `project_add`: register an existing local folder as a project.
#[derive(Debug)]
pub struct ProjectAdd;

/// The folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectAddInput {
    /// An absolute path to an existing folder on this computer.
    pub path: String,
    /// Also make it the current project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

/// The registered project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAddOutput {
    /// The project's path, with links resolved.
    pub project: String,
    /// Whether it was new; `false` when it was already in the rail.
    pub added: bool,
    /// Whether it is the current project now.
    pub current: bool,
}

impl Operation for ProjectAdd {
    const NAME: &'static str = "project_add";
    const TITLE: &'static str = "Add project";
    const SUBJECT: &'static [&'static str] = &["path"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Add an existing local folder to MonoCode's projects, optionally making it current. Adding a folder that is already a project changes nothing and reports `added: false`.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectAddInput;
    type Output = ProjectAddOutput;

    fn summary(output: &ProjectAddOutput) -> String {
        format!(
            "{} {}{}",
            if output.added {
                "Added"
            } else {
                "Already a project:"
            },
            output.project,
            if output.current { " (current)" } else { "" }
        )
    }
}

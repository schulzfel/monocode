//! The project rail: a project's name, colour and mascot, archiving and order.
//! Nothing here touches the project's files.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest project name accepted, in characters.
pub const MAX_PROJECT_NAME_CHARS: usize = 200;

/// `project_update`: a project's name, colour or mascot.
#[derive(Debug)]
pub struct ProjectUpdate;

/// The project and what to change; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectUpdateInput {
    /// The project's path or key, as `project_list` reports it.
    pub project: String,
    /// The name shown in the rail (at most 200 characters); empty shows the
    /// folder name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A colour as `#rrggbb`; empty picks the automatic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// One of `invader`, `ghost`, `robot`, `cat`, `skull`, `crab`,
    /// `mushroom`, `rocket`, `dino` or `frog`; empty picks the automatic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mascot: Option<String>,
}

/// The project's appearance now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdateOutput {
    /// The project's path.
    pub project: String,
    /// Its name as shown.
    pub name: String,
    /// Its colour as `#rrggbb`.
    pub color: String,
    /// Its mascot.
    pub mascot: String,
}

impl Operation for ProjectUpdate {
    const NAME: &'static str = "project_update";
    const TITLE: &'static str = "Update project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Change how a project appears in MonoCode's rail: its name, colour or mascot. The folder on disk is not touched.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectUpdateInput;
    type Output = ProjectUpdateOutput;

    fn summary(output: &ProjectUpdateOutput) -> String {
        format!(
            "{} is “{}”, {}, {}",
            output.project, output.name, output.color, output.mascot
        )
    }
}

/// A project named by path or key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectTarget {
    /// The project's path or key, as `project_list` reports it.
    pub project: String,
}

/// A project's rail membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArchiveOutput {
    /// The project's path.
    pub project: String,
    /// Whether it is archived now.
    pub archived: bool,
}

fn archive_summary(output: &ProjectArchiveOutput) -> String {
    format!(
        "{} {}",
        output.project,
        if output.archived {
            "archived"
        } else {
            "back in the rail"
        }
    )
}

/// `project_archive`: take a project out of the rail.
#[derive(Debug)]
pub struct ProjectArchive;

impl Operation for ProjectArchive {
    const NAME: &'static str = "project_archive";
    const TITLE: &'static str = "Archive project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Archive a project: its tabs close and it leaves the rail. Its sessions and its folder on disk are kept; project_unarchive brings it back.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectTarget;
    type Output = ProjectArchiveOutput;

    fn summary(output: &ProjectArchiveOutput) -> String {
        archive_summary(output)
    }
}

/// `project_unarchive`: put an archived project back in the rail.
#[derive(Debug)]
pub struct ProjectUnarchive;

impl Operation for ProjectUnarchive {
    const NAME: &'static str = "project_unarchive";
    const TITLE: &'static str = "Unarchive project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Put an archived project back in the rail (project_list with archived projects shows them).";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectTarget;
    type Output = ProjectArchiveOutput;

    fn summary(output: &ProjectArchiveOutput) -> String {
        archive_summary(output)
    }
}

/// `project_reorder`: the rail's order.
#[derive(Debug)]
pub struct ProjectReorder;

/// Every project in the rail, in the new order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectReorderInput {
    /// All the rail's projects by path or key, each once, in the new order.
    pub order: Vec<String>,
}

/// The rail's order now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReorderOutput {
    /// The projects' paths in rail order.
    pub order: Vec<String>,
}

impl Operation for ProjectReorder {
    const NAME: &'static str = "project_reorder";
    const TITLE: &'static str = "Reorder projects";
    const SUBJECT: &'static [&'static str] = &["order"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Reorder the project rail. order must name every project in the rail (not archived ones) exactly once.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectReorderInput;
    type Output = ProjectReorderOutput;

    fn summary(output: &ProjectReorderOutput) -> String {
        format!("{} projects reordered", output.order.len())
    }
}

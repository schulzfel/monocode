//! `project_list` (the rail's projects in order, then archived ones),
//! `project_select` and `project_add`.
use super::*;
use monocode_control::ops::{
    self,
    project::{
        DEFAULT_PROJECTS_PER_PAGE, MAX_PROJECTS_PER_PAGE, ProjectAddInput, ProjectAddOutput,
        ProjectListInput, ProjectListOutput, ProjectLocation, ProjectSelectInput,
        ProjectSelectOutput, ProjectSummary,
    },
};
use monocode_core::files::path_key;

impl Shell {
    pub(super) fn control_project_list(
        &self,
        input: &ProjectListInput,
    ) -> Result<ProjectListOutput, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_PROJECTS_PER_PAGE,
            MAX_PROJECTS_PER_PAGE,
        )?;
        let workspace = &self.workspace;
        let current = path_key(&workspace.project);
        let archived = workspace
            .archived_projects
            .iter()
            .filter(|path| !workspace.projects.contains(path));
        let all: Vec<_> = workspace
            .projects
            .iter()
            .map(|path| (path, false))
            .chain(archived.map(|path| (path, true)))
            .map(|(path, archived)| {
                let key = path_key(path);
                ProjectSummary {
                    path: path.clone(),
                    name: self.project_label(path),
                    color: format!("#{:06x}", self.appearance(path).color(path)),
                    current: !archived && key == current,
                    archived,
                    location: if is_remote_path(path) {
                        ProjectLocation::Remote
                    } else {
                        ProjectLocation::Local
                    },
                    key,
                }
            })
            .collect();
        let (projects, next_cursor) = ops::page(&all, input.cursor.as_deref(), limit)?;
        Ok(ProjectListOutput {
            projects,
            total: u32::try_from(all.len()).unwrap_or(u32::MAX),
            next_cursor,
        })
    }
}

impl Shell {
    pub(super) fn control_project_select(
        &mut self,
        input: &ProjectSelectInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ProjectSelectOutput, ControlError> {
        let project = self.control_local_project(Some(&input.project))?;
        let changed = path_key(&project) != path_key(&self.workspace.project);
        // Already current changes nothing, as the request is idempotent.
        if changed {
            self.select_project(project.clone(), window, cx);
        }
        Ok(ProjectSelectOutput { project, changed })
    }

    /// Registers an existing local folder, never through the folder picker.
    pub(super) fn control_project_add(
        &mut self,
        input: &ProjectAddInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ProjectAddOutput, ControlError> {
        if is_remote_path(&input.path) {
            return Err(super::session::remote_unsupported());
        }
        let path = std::path::Path::new(&input.path);
        if !path.is_absolute() {
            return Err(ControlError::invalid_input(
                "path must be an absolute path to a folder on this computer.",
            ));
        }
        let folder = path.canonicalize().map_err(|e| {
            ControlError::new(
                ErrorCode::NotFound,
                format!("{} cannot be opened: {e}", input.path),
            )
        })?;
        if !folder.is_dir() {
            return Err(ControlError::invalid_input(format!(
                "{} is not a folder.",
                input.path
            )));
        }
        let canonical = folder.to_string_lossy().into_owned();
        let existing = self
            .workspace
            .projects
            .iter()
            .find(|p| {
                path_key(p) == path_key(&canonical)
                    || std::path::Path::new(p.as_str())
                        .canonicalize()
                        .is_ok_and(|p| p == folder)
            })
            .cloned();
        let added = existing.is_none();
        let project = existing.unwrap_or(canonical);
        if added {
            // Adds it to the rail (bringing an archived one back) and keeps it.
            self.unarchive_project(&project, cx);
        }
        if input.select == Some(true) && path_key(&project) != path_key(&self.workspace.project) {
            self.select_project(project.clone(), window, cx);
        }
        Ok(ProjectAddOutput {
            current: path_key(&project) == path_key(&self.workspace.project),
            project,
            added,
        })
    }
}

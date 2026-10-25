//! `project_update`, `project_archive`, `project_unarchive` and
//! `project_reorder`: the rail, through `set_project_appearance`,
//! `archive_project`, `unarchive_project` and `reorder_projects`. None of
//! them touches the project's folder.
use super::session::remote_unsupported;
use super::*;
use monocode_control::ops::rail::{
    MAX_PROJECT_NAME_CHARS, ProjectArchiveOutput, ProjectReorderInput, ProjectReorderOutput,
    ProjectTarget, ProjectUpdateInput, ProjectUpdateOutput,
};
use monocode_core::{
    files::path_key,
    project_appearance::{MASCOTS, ProjectAppearance},
};

/// `#rrggbb` as a colour; empty is the automatic colour.
fn color(text: &str) -> Result<Option<u32>, ControlError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .map(Some)
        .ok_or_else(|| ControlError::invalid_input("color must be #rrggbb, or empty"))
}

fn mascot(text: &str) -> Result<Option<String>, ControlError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    MASCOTS
        .contains(&text)
        .then(|| Some(text.to_owned()))
        .ok_or_else(|| {
            ControlError::invalid_input(format!("mascot must be one of {}", MASCOTS.join(", ")))
        })
}

fn shown(path: &str, appearance: &ProjectAppearance) -> ProjectUpdateOutput {
    ProjectUpdateOutput {
        project: path.to_owned(),
        name: appearance.label(path),
        color: format!("#{:06x}", appearance.color(path)),
        mascot: MASCOTS[appearance.mascot(path) % MASCOTS.len()].to_owned(),
    }
}

impl Shell {
    pub(super) fn control_project_update(
        &mut self,
        input: &ProjectUpdateInput,
        cx: &mut Context<Self>,
    ) -> Result<ProjectUpdateOutput, ControlError> {
        let path = self.control_local_project(Some(&input.project))?;
        if input
            .name
            .as_ref()
            .is_some_and(|n| n.trim().chars().count() > MAX_PROJECT_NAME_CHARS)
        {
            return Err(ControlError::invalid_input(format!(
                "name must be at most {MAX_PROJECT_NAME_CHARS} characters"
            )));
        }
        let color = input.color.as_deref().map(color).transpose()?;
        let mascot = input.mascot.as_deref().map(mascot).transpose()?;
        let name = input.name.clone();
        self.set_project_appearance(
            &path,
            |appearance| {
                if let Some(name) = name {
                    appearance.name = name;
                }
                if let Some(color) = color {
                    appearance.color = color;
                }
                if let Some(mascot) = mascot {
                    appearance.mascot = mascot;
                }
            },
            cx,
        );
        Ok(shown(&path, &self.appearance(&path)))
    }

    pub(super) fn control_project_archive(
        &mut self,
        input: &ProjectTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ProjectArchiveOutput, ControlError> {
        let key = path_key(&input.project);
        if let Some(path) = self
            .workspace
            .archived_projects
            .iter()
            .find(|p| p.as_str() == input.project || path_key(p) == key)
        {
            return Ok(ProjectArchiveOutput {
                project: path.clone(),
                archived: true,
            });
        }
        let path = self.control_local_project(Some(&input.project))?;
        self.archive_project(&path, window, cx);
        Ok(ProjectArchiveOutput {
            project: path,
            archived: true,
        })
    }

    pub(super) fn control_project_unarchive(
        &mut self,
        input: &ProjectTarget,
        cx: &mut Context<Self>,
    ) -> Result<ProjectArchiveOutput, ControlError> {
        if let Ok(path) = self.control_project(Some(&input.project)) {
            return Ok(ProjectArchiveOutput {
                project: path,
                archived: false,
            });
        }
        let key = path_key(&input.project);
        let path = self
            .workspace
            .archived_projects
            .iter()
            .find(|p| p.as_str() == input.project || path_key(p) == key)
            .cloned()
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::NotFound,
                    format!(
                        "{} is not an archived project; see project_list.",
                        input.project
                    ),
                )
            })?;
        if is_remote_path(&path) {
            return Err(remote_unsupported());
        }
        self.unarchive_project(&path, cx);
        Ok(ProjectArchiveOutput {
            project: path,
            archived: false,
        })
    }

    pub(super) fn control_project_reorder(
        &mut self,
        input: &ProjectReorderInput,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReorderOutput, ControlError> {
        let mut order = Vec::with_capacity(input.order.len());
        for wanted in &input.order {
            let path = self.control_project(Some(wanted))?;
            if order.contains(&path) {
                return Err(ControlError::invalid_input(format!(
                    "{wanted} is named twice; order must name each project once"
                )));
            }
            order.push(path);
        }
        if order.len() != self.workspace.projects.len() {
            return Err(ControlError::invalid_input(format!(
                "order names {} of the rail's {} projects; name every one (see project_list)",
                order.len(),
                self.workspace.projects.len()
            )));
        }
        self.reorder_projects(order, cx);
        Ok(ProjectReorderOutput {
            order: self.workspace.projects.clone(),
        })
    }
}

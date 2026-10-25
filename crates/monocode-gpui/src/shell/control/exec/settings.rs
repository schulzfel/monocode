//! `settings_read` and `settings_update`: the allowlisted preferences only,
//! applied through `update_preferences`, the Settings page's own path.
use super::*;
use crate::settings::{ModelChoice, PALETTES, Preferences, UI_SCALE_PERCENTS};
use monocode_control::ops::settings::{
    Alerts, DiffView, FavoriteModel, FileSettings, FollowUpMode, SettingsUpdateInput,
    SettingsUpdateOutput, SettingsValues, Theme, TranscriptLayout,
};
use std::collections::HashMap;

/// The most favourite models accepted.
const MAX_FAVORITES: usize = 50;
/// The longest model id accepted.
const MAX_MODEL_ID: usize = 256;

/// The allowlisted preferences as control reports them.
pub(in crate::shell::control) fn values(p: &Preferences) -> SettingsValues {
    SettingsValues {
        theme: Theme::from_id(&p.theme).unwrap_or(Theme::Dark),
        workspace_theme: p.workspace_theme.clone(),
        accent_color: p.accent_color.clone(),
        ui_scale: p.ui_scale,
        transcript_layout: TranscriptLayout::from_id(&p.transcript_layout)
            .unwrap_or(TranscriptLayout::Chat),
        diff_view: DiffView::from_id(&p.diff_view).unwrap_or(DiffView::Editor),
        tab_animations: p.tab_animations,
        alerts: Alerts {
            sounds: p.sounds,
            notifications: p.notifications,
        },
        files: FileSettings {
            word_wrap: p.file_word_wrap,
            autosave: p.autosave,
            format_on_save: p.format_on_save,
        },
        follow_up: FollowUpMode::from_id(&p.follow_up).unwrap_or(FollowUpMode::Steer),
        default_provider: p.default_provider.clone(),
        default_models: p
            .default_models
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        favorite_models: p
            .favorite_models
            .iter()
            .map(|m| FavoriteModel {
                provider: m.provider.clone(),
                model: m.model.clone(),
            })
            .collect(),
        show_remaining_usage: p.show_remaining_usage,
    }
}

fn provider(id: &str) -> Result<String, ControlError> {
    Kind::parse(id).map(|k| k.id().to_owned()).ok_or_else(|| {
        ControlError::invalid_input(format!("There is no provider {id}; see model_list."))
    })
}

fn model_id(id: &str) -> Result<String, ControlError> {
    let id = id.trim();
    if id.is_empty() || id.len() > MAX_MODEL_ID {
        return Err(ControlError::invalid_input(
            "A model id is 1 to 256 characters.",
        ));
    }
    Ok(id.to_owned())
}

fn accent(text: &str) -> Result<Option<String>, ControlError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let valid = text
        .strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    valid
        .then(|| Some(text.to_lowercase()))
        .ok_or_else(|| ControlError::invalid_input("accentColor must be #rrggbb, or empty"))
}

/// Checks `input` and turns it into a change, before anything changes.
fn change(input: SettingsUpdateInput) -> Result<impl FnOnce(&mut Preferences), ControlError> {
    if let Some(id) = &input.workspace_theme
        && !PALETTES.iter().any(|(palette, ..)| palette == id)
    {
        let ids: Vec<_> = PALETTES.iter().map(|(id, ..)| *id).collect();
        return Err(ControlError::invalid_input(format!(
            "workspaceTheme must be one of {}",
            ids.join(", ")
        )));
    }
    if input
        .ui_scale
        .is_some_and(|s| !UI_SCALE_PERCENTS.contains(&s))
    {
        return Err(ControlError::invalid_input(
            "uiScale must be 50 to 200 in steps of 10",
        ));
    }
    let accent = input.accent_color.as_deref().map(accent).transpose()?;
    let default_provider = input
        .default_provider
        .as_deref()
        .map(|id| (!id.trim().is_empty()).then(|| provider(id)).transpose())
        .transpose()?;
    let default_models = input
        .default_models
        .map(|models| {
            models
                .iter()
                .map(|(p, m)| Ok((provider(p)?, model_id(m)?)))
                .collect::<Result<HashMap<_, _>, ControlError>>()
        })
        .transpose()?;
    if input
        .favorite_models
        .as_ref()
        .is_some_and(|f| f.len() > MAX_FAVORITES)
    {
        return Err(ControlError::invalid_input(format!(
            "favoriteModels takes at most {MAX_FAVORITES} models"
        )));
    }
    let favorites = input
        .favorite_models
        .map(|favorites| {
            favorites
                .iter()
                .map(|f| {
                    Ok(ModelChoice {
                        provider: provider(&f.provider)?,
                        model: f.model.as_deref().map(model_id).transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, ControlError>>()
        })
        .transpose()?;
    Ok(move |p: &mut Preferences| {
        let set = |slot: &mut String, value: Option<&str>| {
            if let Some(value) = value {
                *slot = value.into();
            }
        };
        set(&mut p.theme, input.theme.map(Theme::id));
        if let Some(id) = input.workspace_theme {
            p.apply_workspace_theme(id);
        }
        if let Some(accent) = accent {
            p.accent_color = accent;
        }
        if let Some(scale) = input.ui_scale {
            p.ui_scale = scale;
        }
        set(
            &mut p.transcript_layout,
            input.transcript_layout.map(TranscriptLayout::id),
        );
        set(&mut p.diff_view, input.diff_view.map(DiffView::id));
        set(&mut p.follow_up, input.follow_up.map(FollowUpMode::id));
        let flags = [
            (&mut p.tab_animations, input.tab_animations),
            (&mut p.show_remaining_usage, input.show_remaining_usage),
            (&mut p.sounds, input.alerts.as_ref().and_then(|a| a.sounds)),
            (
                &mut p.notifications,
                input.alerts.as_ref().and_then(|a| a.notifications),
            ),
            (
                &mut p.file_word_wrap,
                input.files.as_ref().and_then(|f| f.word_wrap),
            ),
            (
                &mut p.autosave,
                input.files.as_ref().and_then(|f| f.autosave),
            ),
            (
                &mut p.format_on_save,
                input.files.as_ref().and_then(|f| f.format_on_save),
            ),
        ];
        for (slot, value) in flags {
            if let Some(value) = value {
                *slot = value;
            }
        }
        if let Some(provider) = default_provider {
            p.default_provider = provider;
        }
        if let Some(models) = default_models {
            p.default_models = models;
        }
        if let Some(favorites) = favorites {
            p.favorite_models = favorites;
        }
    })
}

/// The allowlisted fields whose values differ, by their wire names.
fn changed(before: &SettingsValues, after: &SettingsValues) -> Vec<String> {
    let fields = |values: &SettingsValues| match serde_json::to_value(values) {
        Ok(Value::Object(fields)) => fields,
        _ => serde_json::Map::new(),
    };
    let (before, after) = (fields(before), fields(after));
    let mut keys: Vec<_> = before.keys().chain(after.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    let mut changed = Vec::new();
    for key in keys {
        let (old, new) = (before.get(&key), after.get(&key));
        match (
            old.and_then(Value::as_object),
            new.and_then(Value::as_object),
        ) {
            (Some(old), Some(new)) if SettingsValues::GROUPS.contains(&key.as_str()) => {
                for (inner, value) in new {
                    if old.get(inner) != Some(value) {
                        changed.push(format!("{key}.{inner}"));
                    }
                }
            }
            _ if old != new => changed.push(key),
            _ => {}
        }
    }
    changed
}

impl Shell {
    pub(super) fn control_settings_read(&self) -> SettingsValues {
        values(&self.workspace.preferences)
    }

    pub(super) fn control_settings_update(
        &mut self,
        input: SettingsUpdateInput,
        cx: &mut Context<Self>,
    ) -> Result<SettingsUpdateOutput, ControlError> {
        let change = change(input)?;
        let before = values(&self.workspace.preferences);
        self.update_preferences(change, cx);
        // An open Settings page shows the change and keeps it.
        self.sync_settings(cx);
        let settings = values(&self.workspace.preferences);
        Ok(SettingsUpdateOutput {
            changed: changed(&before, &settings),
            settings,
        })
    }
}

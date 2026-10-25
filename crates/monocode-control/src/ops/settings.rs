//! The allowlisted preferences: harmless appearance and behaviour keys only.
//!
//! This list is closed on purpose. Provider binary paths, hooks, the default
//! access mode, skill paths, keybindings, project actions, e-mail masking,
//! accounts, MCP servers, mobile, remote, updates and the external-control
//! policy itself are never readable or writable here (proposal section 7.4).
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The window's theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the operating system.
    System,
    /// Dark.
    Dark,
    /// Light.
    Light,
}

/// How prompts show in the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptLayout {
    /// Prompts at full width.
    Full,
    /// Prompts as right-aligned chat bubbles.
    Chat,
}

/// How changes are reviewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffView {
    /// In the editor.
    Editor,
    /// In a combined diff.
    Unified,
}

/// What a message sent during a turn does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpMode {
    /// Wait for the turn to finish.
    Queue,
    /// Go into the running turn.
    Steer,
}

/// `id()` and `from_id()` for a setting whose identifiers are its wire names.
macro_rules! ids {
    ($name:ident { $($variant:ident => $id:literal),+ $(,)? }) => {
        impl $name {
            /// The identifier, as serialized and as the app stores it.
            #[must_use]
            pub fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)+
                }
            }
            /// The value `id` names, if any.
            #[must_use]
            pub fn from_id(id: &str) -> Option<Self> {
                match id {
                    $($id => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

ids!(Theme { System => "system", Dark => "dark", Light => "light" });
ids!(TranscriptLayout { Full => "full", Chat => "chat" });
ids!(DiffView { Editor => "editor", Unified => "unified" });
ids!(FollowUpMode { Queue => "queue", Steer => "steer" });

/// A model starred in the model picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FavoriteModel {
    /// The provider, such as `claude`.
    pub provider: String,
    /// The model id; the provider's default model when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Sounds and system notifications.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Alerts {
    /// Whether the app plays sounds.
    pub sounds: bool,
    /// Whether the app shows system notifications.
    pub notifications: bool,
}

/// How files are edited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSettings {
    /// Whether long lines wrap.
    pub word_wrap: bool,
    /// Whether edited files save on their own.
    pub autosave: bool,
    /// Whether saving formats the file.
    pub format_on_save: bool,
}

/// The allowlisted preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsValues {
    /// `system`, `dark` or `light`.
    pub theme: Theme,
    /// The colour preset's id.
    pub workspace_theme: String,
    /// The accent colour as `#rrggbb`, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// The interface scale in percent: 50 to 200 in steps of 10.
    pub ui_scale: u32,
    /// `full` or `chat`.
    pub transcript_layout: TranscriptLayout,
    /// `editor` or `unified`.
    pub diff_view: DiffView,
    /// Whether tabs animate.
    pub tab_animations: bool,
    /// Sounds and notifications.
    pub alerts: Alerts,
    /// File editing.
    pub files: FileSettings,
    /// `queue` or `steer`.
    pub follow_up: FollowUpMode,
    /// The provider new chats use, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    /// The default model per provider.
    pub default_models: BTreeMap<String, String>,
    /// Starred models, in order.
    pub favorite_models: Vec<FavoriteModel>,
    /// Whether usage limits show what remains.
    pub show_remaining_usage: bool,
}

impl SettingsValues {
    /// The fields that hold others: `settings_update` changes their fields
    /// one by one and names them so (`files.autosave`); every other field is
    /// replaced whole.
    pub const GROUPS: [&'static str; 2] = ["alerts", "files"];
}

/// `settings_read`: the allowlisted preferences.
#[derive(Debug)]
pub struct SettingsRead;

/// No arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsReadInput {}

impl Operation for SettingsRead {
    const NAME: &'static str = "settings_read";
    const TITLE: &'static str = "Read settings";
    const SCOPE: Scope = Scope::Settings;
    const DESCRIPTION: &'static str = "Read MonoCode's allowlisted preferences: theme, colour preset, accent colour, interface scale, transcript layout, diff view, word wrap, tab animations, sounds, notifications, autosave, format on save, follow-up mode, default provider and models, favourite models and remaining-usage display. Nothing else is exposed.";
    const HINTS: Hints = Hints::READ;
    type Input = SettingsReadInput;
    type Output = SettingsValues;

    fn summary(output: &SettingsValues) -> String {
        format!(
            "Theme {}, scale {}%, follow-ups {}",
            output.theme.id(),
            output.ui_scale,
            output.follow_up.id()
        )
    }
}

/// `settings_update`: change allowlisted preferences.
#[derive(Debug)]
pub struct SettingsUpdate;

/// The preferences to change; absent fields stay as they are. Lists and
/// maps replace the whole value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsUpdateInput {
    /// `system`, `dark` or `light`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// A colour preset's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_theme: Option<String>,
    /// An accent colour as `#rrggbb`; empty uses the preset's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// The interface scale in percent: 50 to 200 in steps of 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale: Option<u32>,
    /// `full` or `chat`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_layout: Option<TranscriptLayout>,
    /// `editor` or `unified`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_view: Option<DiffView>,
    /// Whether tabs animate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_animations: Option<bool>,
    /// Sounds and notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alerts: Option<AlertsUpdate>,
    /// File editing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<FileSettingsUpdate>,
    /// `queue` or `steer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<FollowUpMode>,
    /// A provider id for new chats; empty clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    /// The default model per provider id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_models: Option<BTreeMap<String, String>>,
    /// Starred models, in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite_models: Option<Vec<FavoriteModel>>,
    /// Whether usage limits show what remains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_remaining_usage: Option<bool>,
}

/// Sounds and notifications to change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlertsUpdate {
    /// Whether the app plays sounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounds: Option<bool>,
    /// Whether the app shows system notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<bool>,
}

/// File editing settings to change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileSettingsUpdate {
    /// Whether long lines wrap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_wrap: Option<bool>,
    /// Whether edited files save on their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autosave: Option<bool>,
    /// Whether saving formats the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_on_save: Option<bool>,
}

/// The preferences after the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateOutput {
    /// The fields that changed, as named in the input (`files.autosave`).
    pub changed: Vec<String>,
    /// All allowlisted preferences now.
    pub settings: SettingsValues,
}

impl Operation for SettingsUpdate {
    const NAME: &'static str = "settings_update";
    const TITLE: &'static str = "Update settings";
    const SUBJECT: &'static [&'static str] = &[crate::SET_FIELDS];
    const SCOPE: Scope = Scope::Settings;
    const DESCRIPTION: &'static str = "Change MonoCode's allowlisted preferences (see settings_read); they apply at once, as in Settings. Other preferences cannot be changed here.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = SettingsUpdateInput;
    type Output = SettingsUpdateOutput;

    fn summary(output: &SettingsUpdateOutput) -> String {
        if output.changed.is_empty() {
            "No preferences changed".into()
        } else {
            format!("Changed {}", output.changed.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::object_schema;
    use serde_json::Value;

    #[test]
    fn setting_ids_are_their_wire_names() {
        fn check<T: Serialize + Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            id: fn(T) -> &'static str,
            from_id: fn(&str) -> Option<T>,
        ) {
            for &value in all {
                assert_eq!(serde_json::to_value(value).unwrap(), id(value));
                assert_eq!(from_id(id(value)), Some(value));
            }
            assert_eq!(from_id("unknown"), None);
        }
        check(
            &[Theme::System, Theme::Dark, Theme::Light],
            Theme::id,
            Theme::from_id,
        );
        check(
            &[TranscriptLayout::Full, TranscriptLayout::Chat],
            TranscriptLayout::id,
            TranscriptLayout::from_id,
        );
        check(
            &[DiffView::Editor, DiffView::Unified],
            DiffView::id,
            DiffView::from_id,
        );
        check(
            &[FollowUpMode::Queue, FollowUpMode::Steer],
            FollowUpMode::id,
            FollowUpMode::from_id,
        );
    }

    /// The groups are the update's fields whose own type has fields, as
    /// its schema says: maps such as `defaultModels` are replaced whole.
    #[test]
    fn the_groups_are_the_fields_with_fields_of_their_own() {
        let schema = object_schema::<SettingsUpdateInput>();
        let defs = &schema["$defs"];
        let has_fields = |property: &Value| {
            let reference = property["$ref"]
                .as_str()
                .or_else(|| {
                    property["anyOf"]
                        .as_array()?
                        .iter()
                        .find_map(|choice| choice["$ref"].as_str())
                })
                .and_then(|r| r.strip_prefix("#/$defs/"));
            reference.is_some_and(|name| defs[name]["properties"].is_object())
        };
        let mut groups: Vec<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, property)| has_fields(property))
            .map(|(name, _)| name.as_str())
            .collect();
        groups.sort_unstable();
        assert_eq!(groups, SettingsValues::GROUPS);
    }
}

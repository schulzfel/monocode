//! `model_list`: Operator's model discovery (`models_catalog`), filtered
//! and capped, with each model's settings and the access modes. Discovery
//! runs on a worker, as Operator's does.
use super::*;
use monocode_control::ops::{
    self,
    model::{
        AccessModeInfo, DEFAULT_MODELS_PER_PROVIDER, MAX_MODELS_PER_PROVIDER, ModelListInput,
        ModelListOutput, ModelSetting, ModelSummary, ProviderModels, SettingOption,
    },
};
use monocode_core::accounts::Profile;

/// Reads each provider's availability and models, in `models_catalog`'s shape.
pub(in crate::shell) type Source = fn(Vec<(Kind, Profile)>) -> Result<Value, String>;

impl Shell {
    pub(super) fn control_model_list(
        &mut self,
        input: ModelListInput,
        caller: &Caller,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_MODELS_PER_PROVIDER,
            MAX_MODELS_PER_PROVIDER,
        )?;
        let kinds = match input.provider.as_deref() {
            None => Kind::ALL.to_vec(),
            Some(id) => match Kind::parse(id) {
                Some(kind) => vec![kind],
                None => {
                    return Err(ControlError::invalid_input(format!(
                        "There is no provider {id}; call model_list without provider to see them all"
                    )));
                }
            },
        };
        // Each provider's account for the project, as Operator picks it: an
        // agent session's own provider lists with the session's account, the
        // others with the account its project selects. A Mono's main chat
        // (or habit run) lists its own provider with its account, the others
        // with the account selected for no project, as the bridge did.
        let project = match caller.caller {
            CallerKind::Mono { .. } => String::new(),
            CallerKind::Session(_) | CallerKind::External(_) => self
                .caller_project(caller.caller, cx)
                .unwrap_or_else(|| self.workspace.project.clone()),
        };
        let view = match caller.caller {
            CallerKind::Session(id) => self.sessions.get(id).cloned(),
            CallerKind::Mono { session, mono, .. } => self
                .monos
                .as_ref()
                .and_then(|m| m.read(cx).source_view(*session, mono)),
            CallerKind::External(_) => None,
        };
        let own = view.map(|view| {
            let view = view.read(cx);
            (view.summary.provider.clone(), view.settings.account.clone())
        });
        let accounts = crate::accounts::get(cx);
        let profiles = kinds
            .into_iter()
            .map(|kind| {
                let profile = match &own {
                    Some((provider, account)) if provider == kind.id() => account.clone(),
                    _ => accounts
                        .as_ref()
                        .map(|center| center.read(cx).selected(kind, &project))
                        .unwrap_or_default(),
                };
                (kind, profile)
            })
            .collect();
        let source = self.control.models;
        let search = input.search;
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let catalog = source(profiles)
                .map_err(|e| ControlError::new(ErrorCode::ProviderUnavailable, e))?;
            output(&shape(&catalog, search.as_deref(), limit))
        })))
    }
}

/// A string field of `value`, or empty.
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

/// The settings a model in `models_catalog`'s shape offers.
fn settings(model: &Value) -> Vec<ModelSetting> {
    let options = |setting: &Value| {
        setting["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|option| SettingOption {
                value: text(option, "value"),
                label: text(option, "label"),
            })
            .collect()
    };
    model["settings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|setting| ModelSetting {
            id: text(setting, "id"),
            label: text(setting, "label"),
            default_value: text(setting, "defaultValue"),
            options: options(setting),
        })
        .collect()
}

/// The access modes `models_catalog` describes (`runtimeModes`).
fn access_modes(catalog: &Value) -> Vec<AccessModeInfo> {
    catalog["runtimeModes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            Some(AccessModeInfo {
                id: serde_json::from_value(mode["id"].clone()).ok()?,
                label: text(mode, "label"),
                description: text(mode, "description"),
            })
        })
        .collect()
}

/// Filters `catalog` by `search` and keeps at most `limit` models per
/// provider, counting the rest as omitted.
pub(in crate::shell::control) fn shape(
    catalog: &Value,
    search: Option<&str>,
    limit: usize,
) -> ModelListOutput {
    let needle = search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let providers = catalog["harnesses"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|harness| {
            let id = text(harness, "id");
            let matching: Vec<_> = harness["models"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|model| ModelSummary {
                    id: text(model, "id"),
                    name: text(model, "name"),
                    settings: settings(model),
                })
                .filter(|model| {
                    needle.as_ref().is_none_or(|needle| {
                        model.id.to_lowercase().contains(needle)
                            || model.name.to_lowercase().contains(needle)
                    })
                })
                .collect();
            let omitted = matching.len().saturating_sub(limit);
            ProviderModels {
                label: Kind::parse(&id).map_or_else(|| id.clone(), |k| k.label().into()),
                id,
                available: harness["available"].as_bool().unwrap_or(false),
                models: matching.into_iter().take(limit).collect(),
                omitted: u32::try_from(omitted).unwrap_or(u32::MAX),
                error: harness["error"].as_str().map(str::to_owned),
            }
        })
        .collect();
    ModelListOutput {
        providers,
        access_modes: access_modes(catalog),
    }
}

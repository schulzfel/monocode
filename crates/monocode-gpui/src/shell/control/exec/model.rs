//! `model_list`: Operator's model discovery (`models_catalog`), filtered
//! and capped. Discovery runs on a worker, as Operator's does.
use super::*;
use monocode_control::ops::{
    self,
    model::{
        DEFAULT_MODELS_PER_PROVIDER, MAX_MODELS_PER_PROVIDER, ModelListInput, ModelListOutput,
        ModelSummary, ProviderModels,
    },
};
use monocode_core::accounts::Profile;

/// Reads each provider's availability and models, in `models_catalog`'s shape.
pub(in crate::shell) type Source = fn(Vec<(Kind, Profile)>) -> Result<Value, String>;

impl Shell {
    pub(super) fn control_model_list(
        &mut self,
        input: ModelListInput,
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
        // Each provider's account for the current project, as Operator picks it.
        let project = self.workspace.project.clone();
        let accounts = crate::accounts::get(cx);
        let profiles = kinds
            .into_iter()
            .map(|kind| {
                let profile = accounts
                    .as_ref()
                    .map(|center| center.read(cx).selected(kind, &project))
                    .unwrap_or_default();
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
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
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
    ModelListOutput { providers }
}

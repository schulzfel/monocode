//! Providers and their models.
use crate::{AccessMode, Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `model_list`: providers, whether each is available, and a short list of
/// its models.
#[derive(Debug)]
pub struct ModelList;

/// The most models listed per provider unless `limit` says otherwise.
pub const DEFAULT_MODELS_PER_PROVIDER: u32 = 20;
/// The largest `limit` accepted.
pub const MAX_MODELS_PER_PROVIDER: u32 = 1_000;

/// Filters for `model_list`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelListInput {
    /// Only this provider, by id (for example `claude` or `codex`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Only models whose id or name contains this text, ignoring case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// The most models listed per provider (1 to 1000, default 20). The rest
    /// are counted in `omitted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    /// The id to pass when starting a session.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The settings it offers, for `effort` and `modelSettings`.
    #[serde(default)]
    pub settings: Vec<ModelSetting>,
}

/// A setting a model offers, such as its reasoning effort.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelSetting {
    /// The key to pass in `modelSettings`.
    pub id: String,
    /// Its display name.
    pub label: String,
    /// The value it has unless a call sets it.
    pub default_value: String,
    /// The values it accepts.
    pub options: Vec<SettingOption>,
}

/// One value a model setting accepts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingOption {
    /// The value to pass.
    pub value: String,
    /// Its display name.
    pub label: String,
}

/// An access mode a session can run with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessModeInfo {
    /// The mode, to pass as `access`.
    pub id: AccessMode,
    /// Its display name.
    pub label: String,
    /// What it allows.
    pub description: String,
}

/// One provider and its models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    /// The provider's id.
    pub id: String,
    /// The provider's display name.
    pub label: String,
    /// Whether its command-line tool is installed.
    pub available: bool,
    /// Models matching the filters, at most `limit`.
    pub models: Vec<ModelSummary>,
    /// Matching models left out by `limit`.
    pub omitted: u32,
    /// Why the model list could not be read, if it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Providers in the app's order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelListOutput {
    /// Each provider and its models.
    pub providers: Vec<ProviderModels>,
    /// The access modes a session can be started or configured with.
    #[serde(default)]
    pub access_modes: Vec<AccessModeInfo>,
}

impl Operation for ModelList {
    const NAME: &'static str = "model_list";
    const TITLE: &'static str = "List models";
    const SUBJECT: &'static [&'static str] = &["provider", "search"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List the agent providers MonoCode knows, whether each is installed, and a short list of each provider's models. Each model lists the settings it accepts, and `accessModes` the access modes. Filter with `provider` and `search`; `omitted` counts models left out by `limit`.";
    const HINTS: Hints = Hints::READ;
    type Input = ModelListInput;
    type Output = ModelListOutput;

    fn summary(output: &ModelListOutput) -> String {
        let available = output.providers.iter().filter(|p| p.available).count();
        let listed: usize = output.providers.iter().map(|p| p.models.len()).sum();
        let omitted: u64 = output.providers.iter().map(|p| u64::from(p.omitted)).sum();
        format!(
            "{} providers ({available} available), {listed} models listed, {omitted} omitted",
            output.providers.len()
        )
    }
}

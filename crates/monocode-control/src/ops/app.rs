//! The app itself: version, data folder mode and this client's grant.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `app_info`: what is running and what this client may do.
#[derive(Debug)]
pub struct AppInfo;

/// `app_info` takes no arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppInfoInput {}

/// Which data folder the app opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataMode {
    /// The isolated development folder.
    Dev,
    /// The installed desktop app's data.
    Monocode,
}

/// The running app and this client's grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoOutput {
    /// The app's version.
    pub app_version: String,
    /// The control protocol's major version.
    pub protocol_version: u32,
    /// Which data folder the app opened.
    pub data_mode: DataMode,
    /// The operating system: `macos`, `windows` or `linux`.
    pub platform: String,
    /// The scopes granted to this client.
    pub scopes: Vec<Scope>,
    /// Whether external control is turned on.
    pub control_enabled: bool,
}

impl Operation for AppInfo {
    const NAME: &'static str = "app_info";
    const TITLE: &'static str = "Read app info";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Show the running MonoCode app: its version, which data folder it uses, the platform and the scopes granted to this client.";
    const HINTS: Hints = Hints::READ;
    type Input = AppInfoInput;
    type Output = AppInfoOutput;

    fn summary(output: &AppInfoOutput) -> String {
        let scopes: Vec<_> = output.scopes.iter().map(|s| s.id()).collect();
        format!(
            "MonoCode {} on {} ({} data); scopes: {}",
            output.app_version,
            output.platform,
            match output.data_mode {
                DataMode::Dev => "development",
                DataMode::Monocode => "MonoCode",
            },
            if scopes.is_empty() {
                "none".into()
            } else {
                scopes.join(", ")
            }
        )
    }
}

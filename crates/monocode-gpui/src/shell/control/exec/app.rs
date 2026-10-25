//! `app_info`.
use super::*;
use monocode_control::{PROTOCOL_VERSION, ops::app::AppInfoOutput};

impl Shell {
    pub(super) fn control_app_info(&self, grant: &Grant) -> AppInfoOutput {
        AppInfoOutput {
            app_version: crate::notifications::updater::version().into(),
            protocol_version: PROTOCOL_VERSION,
            data_mode: crate::data_location::mode(),
            platform: std::env::consts::OS.into(),
            scopes: grant.scopes.clone(),
            control_enabled: self.control.policy.enabled,
        }
    }
}

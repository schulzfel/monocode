//! Provider-neutral tool presentation, persisted as a native `tool` row.
use serde::{Deserialize, Serialize};
mod label;
mod mcp;
mod presentation;
pub mod tasks;
pub use label::ToolKind;
pub(crate) use mcp::title as mcp_title;
pub use presentation::{CommandStep, ToolPresentation};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: String,
    pub output: String,
    pub state: ToolState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<tasks::Panel>,
    /// Descriptive provider/desktop metadata, separate from actual arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<serde_json::Value>,
}
impl ToolCall {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input: String::new(),
            output: String::new(),
            state: ToolState::Running,
            panel: None,
            display: None,
        }
    }
    pub fn is_compaction(&self) -> bool {
        self.name == "contextCompaction"
    }
    pub fn append_output(&mut self, delta: &str) {
        self.output.push_str(delta);
        self.bound_output();
    }
    pub fn bound_output(&mut self) {
        // Keep a useful tail without retaining unbounded command output in the UI.
        const LIMIT: usize = 64 * 1024;
        if self.output.len() > LIMIT {
            let mut at = self.output.len() - LIMIT;
            while !self.output.is_char_boundary(at) {
                at += 1;
            }
            self.output = format!("… earlier output omitted …\n{}", &self.output[at..]);
        }
    }
    pub fn summary(&self) -> (String, String) {
        let (action, target, _) = self.described();
        (action, target)
    }
    /// [`Self::summary`] of a built-in tool, which consumers may sort and
    /// count calls by; `None` for an MCP server's, whose action never names
    /// a built-in one, whatever it reads ("Search · Linear").
    pub fn builtin_summary(&self) -> Option<(String, String)> {
        let (action, target, builtin) = self.described();
        builtin.then_some((action, target))
    }
    fn described(&self) -> (String, String, bool) {
        match self.panel() {
            Some(tasks::Panel::Tasks(list)) => return ("Tasks".into(), list.progress(), true),
            Some(tasks::Panel::Agent(run)) => return ("Agent".into(), run.title().into(), true),
            None => {}
        }
        let shown = self.presentation();
        let builtin = shown.kind == ToolKind::BuiltIn;
        (shown.action, shown.target, builtin)
    }
}

impl ToolCall {
    /// Provider metadata affects display only. The persisted arguments stay intact.
    pub fn presentation(&self) -> ToolPresentation {
        presentation::present(self, None)
    }
    pub fn presentation_in(&self, cwd: Option<&str>) -> ToolPresentation {
        presentation::present(self, cwd)
    }
    /// The MCP server and tool this call ran, in its provider's naming
    /// (Claude's `mcp__<server>__<tool>`, Codex's `<server>:<tool>`, an ACP
    /// title `<server>/<tool>`); `None` for any other tool.
    pub fn mcp_tool(&self) -> Option<(&str, &str)> {
        let input = serde_json::from_str::<serde_json::Value>(&self.input).unwrap_or_default();
        let desktop = self.display.as_ref().unwrap_or(&input["__desktop"]);
        mcp::identity(&self.name, &presentation::semantic_kind(desktop))
    }
    /// Show the provider's actual input rather than the desktop adapter envelope.
    pub fn input_detail(&self) -> String {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.input) else {
            return self.input.clone();
        };
        let raw = if value["__desktop"].is_object() {
            value
                .get("rawInput")
                .filter(|raw| !raw.is_null())
                .unwrap_or(&value["__desktop"])
        } else {
            &value
        };
        raw.as_str().map(str::to_owned).unwrap_or_else(|| {
            serde_json::to_string_pretty(raw).unwrap_or_else(|_| self.input.clone())
        })
    }
}

/// Strip a recognized shell launcher for display without executing its script.
pub fn shell_command(command: &str) -> String {
    presentation::shell_command(command)
}

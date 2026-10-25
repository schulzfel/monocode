//! Whose tool a call is, and its caption as one line of text.
use super::ToolPresentation;

/// Whose tool a call is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolKind {
    /// The provider's own file, search, shell and agent tools: the
    /// transcript, activity summaries and checkpoints sort calls by their
    /// actions.
    #[default]
    BuiltIn,
    /// A tool an MCP server defines, or Claude's loader for them: its action
    /// never names a built-in one, whatever it reads. `source` is another
    /// server's display name ("Linear"), shown subtly beside the action;
    /// MonoCode's own tools have none.
    Mcp { source: Option<String> },
}

impl ToolPresentation {
    /// Another MCP server's display name ("Linear"), for its own tools.
    pub fn source(&self) -> Option<&str> {
        match &self.kind {
            ToolKind::Mcp { source } => source.as_deref(),
            ToolKind::BuiltIn => None,
        }
    }
    /// The action of a built-in tool, which callers may sort the call by.
    pub fn builtin_action(&self) -> Option<&str> {
        (self.kind == ToolKind::BuiltIn).then_some(self.action.as_str())
    }
    /// The caption as one line of text: "Read src/lib.rs", "Create issue
    /// Fix login · Linear".
    pub fn label(&self) -> String {
        self.label_with(&self.action)
    }
    /// [`Self::label`] with `action` shown in place of the call's own, such
    /// as "Search" for a find with a pattern.
    pub fn label_with(&self, action: &str) -> String {
        let label = [action, self.target.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        match self.source() {
            Some(source) if !label.is_empty() => format!("{label} · {source}"),
            _ => label,
        }
    }
}

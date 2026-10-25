//! What a tool row says beyond its action and target: another MCP server's
//! name as a muted tail, and the one line screen readers hear.
use crate::theme::content;
use gpui::{AnyElement, div, prelude::*, px};
use monocode_core::tool::{ToolPresentation, ToolState};

/// A chain's own description, else the action and target, then the source:
/// "Create issue Fix login · Linear".
pub(super) fn caption(action: &str, shown: &ToolPresentation, chain: bool) -> String {
    match (&shown.description, chain) {
        (Some(description), true) => description.clone(),
        _ => shown.label_with(action),
    }
}

/// "Completed tool call: Remove project repost-sh/monorepo".
pub(super) fn aria_label(state: &ToolState, caption: &str) -> String {
    let state = match state {
        ToolState::Running => "Running",
        ToolState::Completed => "Completed",
        ToolState::Failed => "Failed",
        ToolState::Cancelled => "Cancelled",
    };
    format!("{state} tool call: {caption}")
}

/// The server another MCP tool belongs to, right after the action and more
/// muted than it: "Create issue · Linear".
pub(super) fn source(shown: &ToolPresentation, start: usize) -> Option<AnyElement> {
    let source = shown.source()?.to_owned();
    Some(
        div()
            .debug_selector(move || format!("tool-source-{start}"))
            .flex_shrink_0()
            .text_size(px(12.))
            .text_color(content(0.4))
            .child(format!("· {source}"))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Body, TranscriptView};
    use gpui::TestAppContext;
    use monocode_core::{tool::ToolCall, transcript::Row};

    fn tool_row(id: &str, name: &str, input: &str) -> Row {
        let mut call = ToolCall::new(id, name);
        call.input = input.into();
        call.state = ToolState::Completed;
        Row {
            role: "tool".into(),
            text: serde_json::to_string(&call).unwrap(),
            first: true,
            sent_at: None,
        }
    }

    #[gpui::test]
    fn mcp_tool_rows_show_titles_not_tool_names(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        let captions = window
            .update(cx, |view, _, cx| {
                let user = Row {
                    role: "user".into(),
                    text: "Tidy the rail".into(),
                    first: true,
                    sent_at: None,
                };
                view.patch(
                    0,
                    vec![
                        user,
                        tool_row(
                            "remove",
                            "mcp__monocode__project_delete",
                            r#"{"project":"/Users/x/github.com/repost-sh/monorepo"}"#,
                        ),
                        tool_row(
                            "issue",
                            "mcp__linear__create_issue",
                            r#"{"title":"Fix login"}"#,
                        ),
                    ],
                    cx,
                );
                view.work_folds.insert(1, true);
                view.phases.insert(1, true);
                view.rebuild(0, cx);
                view.rows
                    .iter()
                    .filter_map(|row| match &row.body {
                        Body::Tool { call, .. } => {
                            let shown = call.presentation_in(view.file_link_base.as_deref());
                            let caption = caption(&shown.action, &shown, false);
                            Some(aria_label(&call.state, &caption))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(
            captions,
            [
                "Completed tool call: Remove project repost-sh/monorepo",
                "Completed tool call: Create issue Fix login · Linear",
            ]
        );
        assert!(captions.iter().all(|caption| !caption.contains("mcp__")));
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("tool-target-1").is_some());
        assert!(visual.debug_bounds("tool-source-1").is_none());
        let target = visual.debug_bounds("tool-target-2").expect("issue target");
        let source = visual.debug_bounds("tool-source-2").expect("server tail");
        assert!(source.right() <= target.left());
        assert!((source.center().y - target.center().y).abs() <= px(2.));
        assert!(source.right() <= visual.debug_bounds("tool-2").unwrap().right());
    }
}

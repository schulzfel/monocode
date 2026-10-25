//! Settings → General → MCP server's clients: each approved client with
//! when it was last used, its scopes as compact switches, and Revoke;
//! clients the person denied recently, with Allow; and Recent activity, a
//! disclosure that opens onto the calls clients made, newest first, with
//! Clear. With nothing to list there is no group at all.
use crate::{
    settings::{
        ControlActivity, ExternalControlEdit, ExternalControlStatus, SettingsView,
        widgets::{accent, red, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, Role, SharedString, div, prelude::*, px};
use monocode_control::{ApprovedClient, Scope};
use monocode_core::usage::now_ms;

/// The most activity entries the group shows; the log keeps more.
const SHOWN: usize = 50;

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub(in crate::settings) fn since(at_ms: i64, now_ms: i64) -> String {
    let minutes = (now_ms - at_ms).max(0) / 60_000;
    match minutes {
        0 => "just now".into(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => match minutes / 1440 {
            1 => "1 day ago".into(),
            days => format!("{days} days ago"),
        },
    }
}

impl SettingsView {
    /// The clients group, or nothing when there is nothing to list.
    pub(super) fn mcp_clients_group(
        &mut self,
        status: &ExternalControlStatus,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if status.clients.is_empty() && status.denied.is_empty() && status.activity.is_empty() {
            return None;
        }
        let now = now_ms();
        let mut rows: Vec<AnyElement> = status
            .clients
            .iter()
            .enumerate()
            .map(|(ix, client)| self.mcp_client(ix, client, now, cx))
            .collect();
        for (ix, name) in status.denied.iter().enumerate() {
            let first = rows.is_empty();
            rows.push(self.mcp_denied(ix, name, first, cx));
        }
        if !status.activity.is_empty() {
            let first = rows.is_empty();
            rows.push(self.mcp_activity_toggle(first, cx));
            if self.mcp_server.activity_open {
                rows.extend(
                    status
                        .activity
                        .iter()
                        .take(SHOWN)
                        .enumerate()
                        .map(|(ix, entry)| self.mcp_activity_row(ix, entry, now)),
                );
            }
        }
        Some(self.group(
            Some("mcp-clients"),
            false,
            "Clients",
            Some("Each client keeps the scopes turned on here. Settings, Source control and Destructive stay off unless you turn them on.".into()),
            None,
            rows,
        ))
    }

    fn mcp_client(
        &self,
        ix: usize,
        client: &ApprovedClient,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chips = Scope::ALL.iter().map(|&scope| {
            let on = client.scopes.contains(&scope);
            let name = client.name.clone();
            div()
                .id(SharedString::from(format!(
                    "mcp-client-{ix}-{}",
                    scope.id()
                )))
                .debug_selector(move || format!("mcp-client-{ix}-{}", scope.id()))
                .role(Role::Switch)
                .aria_label(format!("{} for {}", scope.label(), client.name))
                .aria_toggled(if on {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .rounded_md()
                .border_1()
                .px(px(8.))
                .py(px(2.))
                .text_size(px(11.))
                .line_height(px(16.))
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.border_color(accent(0.5))
                            .bg(accent(0.15))
                            .text_color(content(1.))
                    } else {
                        d.border_color(content(0.1)).text_color(content(0.45))
                    }
                })
                .child(scope.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let edit = ExternalControlEdit::Scope {
                        client: name.clone(),
                        scope,
                        on: !on,
                    };
                    this.edit_control(edit, cx);
                }))
                .into_any_element()
        });
        let name = client.name.clone();
        let revoke = secondary_button(
            SharedString::from(format!("mcp-client-revoke-{ix}")),
            "Revoke",
            true,
            false,
        )
        .debug_selector(move || format!("mcp-client-revoke-{ix}"))
        .child("Revoke")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.edit_control(ExternalControlEdit::Revoke(name.clone()), cx);
        }));
        let mut controls: Vec<AnyElement> = chips.collect();
        controls.push(revoke.into_any_element());
        self.row(
            None,
            ix == 0,
            client.name.clone(),
            Some(format!("Last used {}", since(client.last_seen, now)).into()),
            controls,
        )
    }

    /// A client the person denied recently: refused without asking for ten
    /// minutes, unless allowed here.
    fn mcp_denied(&self, ix: usize, name: &str, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let client = name.to_owned();
        let allow = secondary_button(
            SharedString::from(format!("mcp-client-allow-{ix}")),
            "Allow",
            false,
            false,
        )
        .debug_selector(move || format!("mcp-client-allow-{ix}"))
        .child("Allow")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.edit_control(ExternalControlEdit::Allow(client.clone()), cx);
        }));
        self.row(
            None,
            first,
            name.to_owned(),
            Some("Denied · not asked again for ten minutes unless you allow it".into()),
            vec![allow.into_any_element()],
        )
    }

    /// Recent activity's row: open or close it, and Clear.
    fn mcp_activity_toggle(&self, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let open = self.mcp_server.activity_open;
        let clear = secondary_button("mcp-activity-clear", "Clear", false, false)
            .debug_selector(|| "mcp-activity-clear".into())
            .child("Clear")
            .on_click(cx.listener(|this, _, _, cx| {
                this.edit_control(ExternalControlEdit::ClearActivity, cx);
            }));
        let disclose = secondary_button(
            "mcp-activity-toggle",
            if open {
                "Hide activity"
            } else {
                "Show activity"
            },
            false,
            false,
        )
        .debug_selector(|| "mcp-activity-toggle".into())
        .aria_expanded(open)
        .child(
            icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                14.,
            )
            .text_color(content(0.7)),
        )
        .child(if open { "Hide" } else { "Show" })
        .on_click(cx.listener(|this, _, _, cx| {
            this.mcp_server.activity_open = !this.mcp_server.activity_open;
            cx.notify();
        }));
        self.row(
            Some("mcp-activity"),
            first,
            "Recent activity",
            Some(
                format!("The last calls clients made, newest first; up to {SHOWN} are listed.")
                    .into(),
            ),
            vec![clear.into_any_element(), disclose.into_any_element()],
        )
    }

    fn mcp_activity_row(&self, ix: usize, entry: &ControlActivity, now: i64) -> AnyElement {
        let title = match &entry.target {
            Some(target) => format!("{} · {target}", entry.operation),
            None => entry.operation.clone(),
        };
        let ok = entry.outcome == "ok";
        let result = div()
            .debug_selector(move || format!("mcp-activity-{ix}"))
            .text_size(px(12.))
            .line_height(px(19.5))
            .text_color(if ok { content(0.6) } else { red(1.) })
            .child(SharedString::from(if ok {
                "OK".to_owned()
            } else {
                entry.outcome.replace('_', " ")
            }))
            .into_any_element();
        self.row(
            None,
            false,
            title,
            Some(format!("{} · {}", entry.client, since(entry.at, now)).into()),
            vec![result],
        )
    }
}

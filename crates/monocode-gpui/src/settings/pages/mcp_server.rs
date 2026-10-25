//! Settings → General → MCP server: the switch, the URL clients connect to
//! with its Copy button, and Add to buttons for Claude Code, Codex, Cursor
//! and VS Code. The approved clients and recent activity follow while it is
//! on (`mcp_clients`). Only this group changes the policy.
use crate::{
    input::{self, Input},
    settings::{
        ExternalControlEdit, ExternalControlStatus, Handoff, SettingsEvent, SettingsView,
        mcp_server::{CLIENTS, Install},
        widgets::{red, secondary_button},
    },
    theme::*,
};
use gpui::{
    AnyElement, Context, Entity, FontWeight, SharedString, Subscription, Task, div, prelude::*, px,
};
use std::time::Duration;

/// How long a copy button says "Copied".
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// The ports the port field accepts: the unprivileged ones.
const PORTS: std::ops::RangeInclusive<u16> = 1024..=65535;

/// What the group remembers between frames.
#[derive(Default)]
pub(in crate::settings) struct McpServerState {
    /// The copy button that just copied, while it says so.
    pub(in crate::settings) copied: Option<&'static str>,
    copied_reset: Option<Task<()>>,
    /// Whether Recent activity is open.
    pub(in crate::settings) activity_open: bool,
    /// The URL, in a read-only field so it can be selected.
    pub(in crate::settings) url: Option<Entity<Input>>,
    /// The port field offered when the server cannot listen.
    pub(in crate::settings) port: Option<Entity<Input>>,
    /// Whether the port field holds something that is not a port.
    port_invalid: bool,
    _port_events: Vec<Subscription>,
}

impl SettingsView {
    pub(super) fn edit_control(&mut self, edit: ExternalControlEdit, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::ExternalControl(edit));
        cx.notify();
    }

    /// The MCP server group; while it is on, its clients' group too.
    pub(super) fn mcp_server_groups(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(status) = self.ctx.external_control.clone() else {
            return Vec::new();
        };
        let toggle = self.toggle(
            "mcp-server",
            "MCP server",
            status.enabled,
            false,
            cx,
            |this, on, cx| this.edit_control(ExternalControlEdit::Enabled(on), cx),
        );
        let mut rows = vec![self.row(
            Some("mcp-server"),
            true,
            "MCP server",
            Some("Let MCP clients on this computer, like Claude Code, Codex, Cursor or VS Code, use MonoCode.".into()),
            vec![toggle],
        )];
        if status.enabled {
            rows.push(self.mcp_url_row(&status, cx));
            if let (Some(url), None) = (&status.url, &status.error) {
                rows.push(self.mcp_add_row(url, cx));
            }
        }
        let mut groups = vec![self.group(None, false, "MCP server", None, None, rows)];
        if status.enabled {
            groups.extend(self.mcp_clients_group(&status, cx));
        }
        groups
    }

    /// Copies `text` and says so on the button `id` for a moment.
    fn mcp_copy(&mut self, id: &'static str, text: &str, cx: &mut Context<Self>) {
        Handoff::copy(text, cx);
        self.mcp_server.copied = Some(id);
        self.mcp_server.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.mcp_server.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// A button that copies `text`, or opens it as a link.
    fn mcp_button(
        &self,
        id: &'static str,
        label: &'static str,
        install: Install,
        text: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let copied = install == Install::Copy && self.mcp_server.copied == Some(id);
        let path = match install {
            Install::Copy if copied => "check",
            Install::Copy => "copy",
            Install::Open => "external-link",
        };
        secondary_button(
            SharedString::from(format!("mcp-server-{id}")),
            label,
            false,
            false,
        )
        .debug_selector(move || format!("mcp-server-{id}"))
        .child(icon(path, 14.).text_color(content(0.7)))
        .child(if copied { "Copied" } else { label })
        .on_click(cx.listener(move |this, _, _, cx| match install {
            Install::Copy => this.mcp_copy(id, &text, cx),
            Install::Open => Handoff::open(&text, cx),
        }))
        .into_any_element()
    }

    /// The URL with its Copy button; when the server cannot listen, why,
    /// with a field to choose another port.
    fn mcp_url_row(
        &mut self,
        status: &ExternalControlStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(url) = status.url.clone().filter(|_| status.error.is_none()) else {
            return self.mcp_port_row(status, cx);
        };
        let field = self.mcp_url_field(&url, cx);
        let copy = self.mcp_button("copy-url", "Copy", Install::Copy, url, cx);
        self.row(
            Some("mcp-server-url"),
            false,
            "Server URL",
            Some("MCP clients on this computer connect here.".into()),
            vec![field, copy],
        )
    }

    fn mcp_url_field(&mut self, url: &str, cx: &mut Context<Self>) -> AnyElement {
        let field = self.mcp_server.url.clone().unwrap_or_else(|| {
            let field = cx.new(|cx| {
                let mut input = Input::field("Server URL", "", content(0.35), cx);
                input.set_font_size(12.);
                input.set_read_only(true);
                input
            });
            self.mcp_server.url = Some(field.clone());
            field
        });
        if field.read(cx).content.as_ref() != url {
            field.update(cx, |input, cx| input.set_text(url.to_owned(), cx));
        }
        // Wide enough for the whole URL in the monospaced face.
        let width = url.chars().count() as f32 * 7.5 + 18.;
        div()
            .debug_selector(|| "mcp-server-url".into())
            .w(px(width))
            .h(px(26.))
            .rounded_md()
            .border_1()
            .border_color(content(0.1))
            .px(px(8.))
            .flex()
            .items_center()
            .font_family(crate::markdown::MONO)
            .child(field)
            .into_any_element()
    }

    /// Why the server is not serving, and a field for another port.
    fn mcp_port_row(
        &mut self,
        status: &ExternalControlStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = self.mcp_port_field(status.port, cx);
        let apply = secondary_button("mcp-server-port-apply", "Use port", false, false)
            .debug_selector(|| "mcp-server-port-apply".into())
            .child("Use port")
            .on_click(cx.listener(|this, _, _, cx| this.mcp_apply_port(cx)));
        let reason = if self.mcp_server.port_invalid {
            format!("Enter a port from {} to {}.", PORTS.start(), PORTS.end())
        } else {
            status
                .error
                .clone()
                .unwrap_or_else(|| "The MCP server is starting.".into())
        };
        let label = div().child("Server URL").child(
            div()
                .debug_selector(|| "mcp-server-error".into())
                .mt_1()
                .text_size(px(12.))
                .line_height(px(19.5))
                .font_weight(FontWeight::NORMAL)
                .text_color(red(1.))
                .child(reason),
        );
        self.row(
            Some("mcp-server-url"),
            false,
            label,
            None,
            vec![field, apply.into_any_element()],
        )
    }

    fn mcp_port_field(&mut self, port: u16, cx: &mut Context<Self>) -> AnyElement {
        let field = self.mcp_server.port.clone().unwrap_or_else(|| {
            let field = cx.new(|cx| {
                let mut input = Input::field("Port", "Port", content(0.35), cx);
                input.set_font_size(12.);
                input.set_max_chars(5);
                input.set_text(port.to_string(), cx);
                input
            });
            self.mcp_server._port_events = vec![
                cx.subscribe(&field, |this, _, _: &input::Changed, cx| {
                    this.mcp_server.port_invalid = false;
                    cx.notify();
                }),
                cx.subscribe(&field, |this, _, _: &input::Submitted, cx| {
                    this.mcp_apply_port(cx);
                }),
            ];
            self.mcp_server.port = Some(field.clone());
            field
        });
        div()
            .debug_selector(|| "mcp-server-port".into())
            .w(px(72.))
            .h(px(26.))
            .rounded_md()
            .border_1()
            .border_color(content(0.15))
            .px(px(8.))
            .flex()
            .items_center()
            .font_family(crate::markdown::MONO)
            .child(field)
            .into_any_element()
    }

    /// Serves on the port in the field, if it is one.
    fn mcp_apply_port(&mut self, cx: &mut Context<Self>) {
        let text = self
            .mcp_server
            .port
            .as_ref()
            .map(|field| field.read(cx).content.to_string())
            .unwrap_or_default();
        match text.trim().parse::<u16>() {
            Ok(port) if PORTS.contains(&port) => {
                self.mcp_server.port_invalid = false;
                self.edit_control(ExternalControlEdit::Port(port), cx);
            }
            _ => {
                self.mcp_server.port_invalid = true;
                cx.notify();
            }
        }
    }

    /// One button per client: a command to copy, or a link to open.
    fn mcp_add_row(&mut self, url: &str, cx: &mut Context<Self>) -> AnyElement {
        let buttons = CLIENTS
            .iter()
            .map(|c| self.mcp_button(c.id, c.label, c.install, (c.text)(url), cx))
            .collect();
        self.row(
            Some("mcp-server-add"),
            false,
            "Add to",
            Some("Claude Code and Codex copy a terminal command; Cursor and VS Code open to add MonoCode.".into()),
            buttons,
        )
    }
}

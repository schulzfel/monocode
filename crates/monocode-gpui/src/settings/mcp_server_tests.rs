//! Settings → General → MCP server, and its absence from Connections.
use super::*;
use gpui::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point,
    size,
};
use monocode_control::{ApprovedClient, Scope};
use std::{cell::RefCell, rc::Rc};

const URL: &str = "http://127.0.0.1:6262/mcp";

fn status(enabled: bool, clients: &[&str]) -> ExternalControlStatus {
    ExternalControlStatus {
        enabled,
        url: enabled.then(|| URL.to_owned()),
        port: 6262,
        error: None,
        clients: clients
            .iter()
            .map(|name| ApprovedClient {
                name: (*name).into(),
                scopes: Scope::DEFAULT_GRANT.to_vec(),
                approved_at: 0,
                last_seen: 0,
            })
            .collect(),
        activity: Vec::new(),
        denied: Vec::new(),
    }
}

/// What the page asked for: its edits, the text it copied, the links it
/// opened.
#[derive(Clone, Default)]
struct Seen {
    edits: Rc<RefCell<Vec<ExternalControlEdit>>>,
    copied: Rc<RefCell<Vec<String>>>,
    opened: Rc<RefCell<Vec<String>>>,
}

struct Page {
    window: WindowHandle<SettingsView>,
    visual: VisualTestContext,
    seen: Seen,
}

impl Page {
    fn open(section: Section, status: ExternalControlStatus, cx: &mut TestAppContext) -> Self {
        let seen = Seen::default();
        let (copied, opened) = (seen.copied.clone(), seen.opened.clone());
        cx.update(|cx| {
            cx.set_global(Handoff {
                copy: Rc::new(move |text, _| copied.borrow_mut().push(text.to_owned())),
                open: Rc::new(move |url, _| opened.borrow_mut().push(url.to_owned())),
            });
        });
        let edits = seen.edits.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1200.), px(4000.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    let view = cx.new(|cx| {
                        let context = SettingsContext {
                            external_control: Some(status),
                            ..Default::default()
                        };
                        SettingsView::new(section, Preferences::default(), context, cx)
                    });
                    cx.subscribe(&view, move |_, event: &SettingsEvent, _| {
                        if let SettingsEvent::ExternalControl(edit) = event {
                            edits.borrow_mut().push(edit.clone());
                        }
                    })
                    .detach();
                    view
                },
            )
            .unwrap()
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        let visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        Self {
            window,
            visual,
            seen,
        }
    }

    fn shows(&mut self, selector: &'static str) -> bool {
        self.visual.debug_bounds(selector).is_some()
    }

    fn click(&mut self, selector: &'static str) {
        let bounds = self.visual.debug_bounds(selector).unwrap();
        self.visual
            .simulate_click(bounds.center(), Default::default());
        self.visual.run_until_parked();
    }

    fn set(&mut self, status: ExternalControlStatus) {
        self.window
            .update(&mut self.visual, |view, _, cx| {
                view.ctx.external_control = Some(status);
                cx.notify();
            })
            .unwrap();
        self.visual.run_until_parked();
    }
}

#[gpui::test]
fn off_shows_only_the_switch_and_turning_it_on_is_an_edit(cx: &mut TestAppContext) {
    let mut page = Page::open(Section::General, status(false, &["Claude Code"]), cx);
    assert!(page.shows("settings-row-mcp-server"));
    assert!(page.shows("settings-toggle-mcp-server"));
    for hidden in [
        "settings-row-mcp-server-url",
        "settings-row-mcp-server-add",
        "mcp-client-0-read",
        "settings-row-mcp-activity",
    ] {
        assert!(!page.shows(hidden), "off: no {hidden}");
    }
    page.click("settings-toggle-mcp-server");
    assert_eq!(
        *page.seen.edits.borrow(),
        [ExternalControlEdit::Enabled(true)]
    );
}

#[gpui::test]
fn on_shows_the_url_and_add_to_and_clients_only_when_there_are_any(cx: &mut TestAppContext) {
    let mut page = Page::open(Section::General, status(true, &[]), cx);
    assert!(page.shows("settings-row-mcp-server-url"));
    assert!(page.shows("mcp-server-url"));
    assert!(page.shows("settings-row-mcp-server-add"));
    assert!(!page.shows("mcp-client-0-read"), "no clients: no list");
    assert!(!page.shows("settings-row-mcp-activity"), "nothing to list");
    let shown = page
        .window
        .update(&mut page.visual, |view, _, cx| {
            view.mcp_server
                .url
                .as_ref()
                .map(|field| field.read(cx).content.to_string())
        })
        .unwrap();
    assert_eq!(shown.as_deref(), Some(URL), "the whole URL, selectable");

    page.set(ExternalControlStatus {
        denied: vec!["Cursor".into()],
        ..status(true, &["Claude Code"])
    });
    for chip in [
        "mcp-client-0-read",
        "mcp-client-0-navigate",
        "mcp-client-0-dispatch",
        "mcp-client-0-manage",
        "mcp-client-0-settings",
        "mcp-client-0-vcs",
        "mcp-client-0-destructive",
    ] {
        assert!(page.shows(chip), "{chip}");
    }
    page.click("mcp-client-0-settings");
    page.click("mcp-client-revoke-0");
    page.click("mcp-client-allow-0");
    assert_eq!(
        *page.seen.edits.borrow(),
        [
            ExternalControlEdit::Scope {
                client: "Claude Code".into(),
                scope: Scope::Settings,
                on: true,
            },
            ExternalControlEdit::Revoke("Claude Code".into()),
            ExternalControlEdit::Allow("Cursor".into()),
        ]
    );
}

#[gpui::test]
fn copy_and_every_add_to_button_hand_out_the_exact_text_or_link(cx: &mut TestAppContext) {
    let mut page = Page::open(Section::General, status(true, &[]), cx);
    page.click("mcp-server-copy-url");
    let copied = page
        .window
        .update(&mut page.visual, |view, _, _| view.mcp_server.copied)
        .unwrap();
    assert_eq!(copied, Some("copy-url"), "the button says Copied");
    page.click("mcp-server-claude-code");
    page.click("mcp-server-codex");
    page.click("mcp-server-cursor");
    page.click("mcp-server-vscode");
    assert_eq!(
        *page.seen.copied.borrow(),
        [
            URL,
            "claude mcp add --transport http monocode http://127.0.0.1:6262/mcp",
            "codex mcp add monocode --url http://127.0.0.1:6262/mcp",
        ]
    );
    assert_eq!(
        *page.seen.opened.borrow(),
        [
            // base64 of {"url":"http://127.0.0.1:6262/mcp"}
            "cursor://anysphere.cursor-deeplink/mcp/install?name=monocode&config=eyJ1cmwiOiJodHRwOi8vMTI3LjAuMC4xOjYyNjIvbWNwIn0=",
            // encodeURIComponent of {"name":"monocode","type":"http","url":"http://127.0.0.1:6262/mcp"}
            "vscode:mcp/install?%7B%22name%22%3A%22monocode%22%2C%22type%22%3A%22http%22%2C%22url%22%3A%22http%3A%2F%2F127.0.0.1%3A6262%2Fmcp%22%7D",
        ]
    );
    assert!(page.seen.edits.borrow().is_empty(), "nothing changed");
}

#[gpui::test]
fn a_taken_port_is_shown_in_the_url_row_with_a_field_for_another(cx: &mut TestAppContext) {
    let taken = ExternalControlStatus {
        url: None,
        error: Some("Port 6262 is in use by another app. Choose another port.".into()),
        ..status(true, &[])
    };
    let mut page = Page::open(Section::General, taken, cx);
    assert!(page.shows("mcp-server-error"));
    assert!(page.shows("mcp-server-port"));
    assert!(!page.shows("mcp-server-url"));
    assert!(
        !page.shows("settings-row-mcp-server-add"),
        "nothing to add yet"
    );
    let typed = |text: &'static str, page: &mut Page| {
        page.window
            .update(&mut page.visual, |view, _, cx| {
                let field = view.mcp_server.port.clone().unwrap();
                field.update(cx, |input, cx| input.set_text(text.into(), cx));
            })
            .unwrap();
        page.click("mcp-server-port-apply");
    };
    typed("80", &mut page);
    assert!(page.seen.edits.borrow().is_empty(), "not a port it takes");
    typed("7311", &mut page);
    assert_eq!(*page.seen.edits.borrow(), [ExternalControlEdit::Port(7311)]);
}

#[gpui::test]
fn recent_activity_opens_newest_first_and_clears(cx: &mut TestAppContext) {
    let row = |at: i64, operation: &str, outcome: &str| ControlActivity {
        at,
        client: "Claude Code".into(),
        operation: operation.into(),
        target: None,
        outcome: outcome.into(),
    };
    let busy = ExternalControlStatus {
        activity: vec![
            row(2, "session_stop", "scope_denied"),
            row(1, "project_list", "ok"),
        ],
        ..status(true, &["Claude Code"])
    };
    let mut page = Page::open(Section::General, busy, cx);
    assert!(page.shows("settings-row-mcp-activity"));
    assert!(!page.shows("mcp-activity-0"), "closed until opened");
    page.click("mcp-activity-toggle");
    let first = page.visual.debug_bounds("mcp-activity-0").unwrap();
    let second = page.visual.debug_bounds("mcp-activity-1").unwrap();
    assert!(first.top() < second.top(), "newest first");
    page.click("mcp-activity-clear");
    assert_eq!(
        *page.seen.edits.borrow(),
        [ExternalControlEdit::ClearActivity]
    );
}

#[gpui::test]
fn connections_no_longer_shows_the_mcp_server(cx: &mut TestAppContext) {
    let mut page = Page::open(Section::Connections, status(true, &["Claude Code"]), cx);
    for gone in [
        "settings-toggle-mcp-server",
        "settings-toggle-external-control",
        "mcp-server-url",
        "external-control-command",
        "mcp-client-0-read",
        "external-control-0-read",
    ] {
        assert!(!page.shows(gone), "Connections shows {gone}");
    }
}

#[test]
fn last_used_reads_as_a_short_relative_time() {
    use pages::mcp_since as since;
    let minute = 60_000;
    assert_eq!(since(0, 30_000), "just now");
    assert_eq!(since(0, 5 * minute), "5 min ago");
    assert_eq!(since(0, 3 * 60 * minute), "3 h ago");
    assert_eq!(since(0, 24 * 60 * minute), "1 day ago");
    assert_eq!(since(0, 3 * 24 * 60 * minute), "3 days ago");
    assert_eq!(since(10, 0), "just now");
}

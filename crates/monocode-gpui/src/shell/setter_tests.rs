//! R5 and R7: the UI's own paths and the new setters change the window and
//! what is saved the same way.
use super::*;
use crate::settings::{Preferences, SettingsEvent};
use gpui::{TestAppContext, WindowHandle};
use serde_json::Value;

/// What the window applied and what it saved.
fn applied(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> (Preferences, Value) {
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            let saved: Value = serde_json::from_str(&shell.latest_state.lock().unwrap()).unwrap();
            (
                cx.global::<Preferences>().clone(),
                saved["preferences"].clone(),
            )
        })
        .unwrap()
}

fn change(p: &mut Preferences) {
    p.ui_scale = 120;
    p.sounds = false;
    p.show_remaining_usage = true;
    p.follow_up = "queue".into();
}

#[gpui::test]
fn a_settings_page_change_and_update_preferences_apply_and_save_alike(cx: &mut TestAppContext) {
    let (page, _, _) = crate::shell::tests::window(cx);
    page.update(cx, |shell, window, cx| {
        shell.open_settings(crate::settings::Section::General, window, cx);
        let view = shell.settings.clone().unwrap();
        let mut prefs = shell.workspace.preferences.clone();
        change(&mut prefs);
        view.update(cx, |_, cx| cx.emit(SettingsEvent::Changed(Box::new(prefs))));
    })
    .unwrap();
    let by_page = applied(page, cx);
    let (setter, _, _) = crate::shell::tests::window(cx);
    setter
        .update(cx, |shell, _, cx| shell.update_preferences(change, cx))
        .unwrap();
    let by_setter = applied(setter, cx);
    assert_eq!(by_page, by_setter);
    assert_eq!(by_setter.0.ui_scale, 120, "applied as the app-wide global");
    assert_eq!(by_setter.1["ui_scale"], 120, "and saved with the workspace");
}

#[gpui::test]
fn the_layout_events_and_setters_agree(cx: &mut TestAppContext) {
    let layout = |window: WindowHandle<Shell>, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| {
                (
                    shell.workspace.classic_layout,
                    shell.workspace.new_layout_boxy,
                )
            })
            .unwrap()
    };
    let (page, _, _) = crate::shell::tests::window(cx);
    page.update(cx, |shell, window, cx| {
        shell.open_settings(crate::settings::Section::Appearance, window, cx);
        let view = shell.settings.clone().unwrap();
        let classic = shell.workspace.classic_layout;
        view.update(cx, |_, cx| {
            cx.emit(SettingsEvent::NewLayout(classic));
            cx.emit(SettingsEvent::NewLayoutBoxy(true));
        });
    })
    .unwrap();
    cx.run_until_parked();
    let (setter, _, _) = crate::shell::tests::window(cx);
    setter
        .update(cx, |shell, _, cx| {
            let classic = shell.workspace.classic_layout;
            shell.set_classic_layout(!classic, cx);
            shell.set_classic_layout(!classic, cx);
            shell.set_layout_boxy(true, cx);
        })
        .unwrap();
    assert_eq!(
        layout(page, cx),
        layout(setter, cx),
        "a repeated set is a no-op"
    );
}

#[gpui::test]
fn the_appearance_controls_and_set_project_appearance_save_alike(cx: &mut TestAppContext) {
    let project = "/work/monocode";
    let saved = |window: WindowHandle<Shell>, cx: &mut TestAppContext| {
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let _ = cx;
                let state: Value =
                    serde_json::from_str(&shell.latest_state.lock().unwrap()).unwrap();
                (
                    shell.appearance(project),
                    state["project_appearance"][project].clone(),
                )
            })
            .unwrap()
    };
    let (controls, _, _) = crate::shell::tests::window(cx);
    let (view, events) = controls
        .update(cx, |shell, window, cx| {
            let mut events = Vec::new();
            let view = shell.project_appearance_controls(project, &mut events, window, cx);
            (view, events)
        })
        .unwrap();
    cx.run_until_parked();
    let mut appearance = controls
        .update(cx, |shell, _, _| shell.appearance(project))
        .unwrap();
    appearance.name = "  MonoCode  ".into();
    appearance.mascot = Some("crab".into());
    view.update(cx, |_, cx| {
        cx.emit(crate::project_settings::Event::Changed(appearance));
    });
    let by_controls = saved(controls, cx);
    drop(events);
    let (setter, _, _) = crate::shell::tests::window(cx);
    setter
        .update(cx, |shell, _, cx| {
            shell.set_project_appearance(
                project,
                |a| {
                    a.name = "  MonoCode  ".into();
                    a.mascot = Some("crab".into());
                },
                cx,
            );
        })
        .unwrap();
    let by_setter = saved(setter, cx);
    assert_eq!(by_controls, by_setter);
    assert_eq!(
        by_setter.0.name, "MonoCode",
        "normalized as the controls are"
    );
}

#[gpui::test]
fn reorder_projects_moves_named_projects_through_their_own_slots_as_a_drop_does(
    cx: &mut TestAppContext,
) {
    let (window, _, _) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project("/work/third");
            let [a, b, c] =
                ["/work/another-project", "/work/monocode", "/work/third"].map(str::to_owned);
            assert_eq!(shell.workspace.projects, [a.clone(), b.clone(), c.clone()]);
            // A drop inside a group reorders only the dragged subset.
            shell.apply_project_order(vec![c.clone(), a.clone()]);
            assert_eq!(shell.workspace.projects, [c.clone(), b.clone(), a.clone()]);
            // External control names every project.
            shell.reorder_projects(vec![b.clone(), a.clone(), c.clone()], cx);
            assert_eq!(shell.workspace.projects, [b.clone(), a.clone(), c.clone()]);
            let saved: Value = serde_json::from_str(&shell.latest_state.lock().unwrap()).unwrap();
            assert_eq!(saved["projects"], serde_json::json!([b, a, c]), "and saved");
        })
        .unwrap();
}

//! The three executors against the fixture shell.
use super::tests::{approve, call, setup};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    Command, PROTOCOL_VERSION, Scope,
    ops::{
        app::{AppInfoInput, AppInfoOutput, DataMode},
        model::{ModelListInput, ModelListOutput},
        project::{ProjectListInput, ProjectListOutput, ProjectLocation},
    },
};
use monocode_core::accounts::Profile;
use serde_json::json;

/// Model discovery without provider processes: each requested provider
/// answers with three models; `cursor` is not installed.
fn fixture_models(profiles: Vec<(Kind, Profile)>) -> Result<Value, String> {
    let harnesses: Vec<_> = profiles
        .into_iter()
        .map(|(kind, _)| {
            let available = kind != Kind::Cursor;
            json!({
                "id": kind.id(),
                "available": available,
                "models": if available {
                    json!([
                        {"id": format!("{}-sonnet", kind.id()), "name": "Sonnet 5"},
                        {"id": format!("{}-opus", kind.id()), "name": "Opus 5"},
                        {"id": format!("{}-haiku", kind.id()), "name": "Haiku 5"},
                    ])
                } else {
                    json!([])
                },
                "error": Value::Null,
            })
        })
        .collect();
    Ok(json!({"harnesses": harnesses}))
}

fn failing_models(_: Vec<(Kind, Profile)>) -> Result<Value, String> {
    Err("Model discovery failed".into())
}

fn models(
    window: gpui::WindowHandle<Shell>,
    input: ModelListInput,
    cx: &mut TestAppContext,
) -> Result<ModelListOutput, ControlError> {
    call(window, "Claude Code", None, Command::ModelList(input), cx)
        .map(|v| serde_json::from_value(v).unwrap())
}

fn projects(
    window: gpui::WindowHandle<Shell>,
    cursor: Option<&str>,
    limit: Option<u32>,
    cx: &mut TestAppContext,
) -> Result<ProjectListOutput, ControlError> {
    let input = ProjectListInput {
        cursor: cursor.map(str::to_owned),
        limit,
    };
    call(window, "Claude Code", None, Command::ProjectList(input), cx)
        .map(|v| serde_json::from_value(v).unwrap())
}

#[gpui::test]
fn app_info_reports_the_app_and_this_clients_grant(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(
        window,
        &[("Claude Code", &[Scope::Read, Scope::Navigate])],
        cx,
    );
    let info: AppInfoOutput = serde_json::from_value(
        call(
            window,
            "Claude Code",
            None,
            Command::AppInfo(AppInfoInput {}),
            cx,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(info.app_version, crate::notifications::updater::version());
    assert_eq!(info.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        info.data_mode,
        DataMode::Dev,
        "tests never open MonoCode data"
    );
    assert_eq!(info.platform, std::env::consts::OS);
    assert_eq!(info.scopes, [Scope::Read, Scope::Navigate]);
    assert!(info.control_enabled);
}

#[gpui::test]
fn project_list_pages_the_rail_in_order_then_archived(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Claude Code", &[Scope::Read])], cx);
    window
        .update(cx, |shell, _, _| {
            shell.workspace.add_project("remote://box/srv/app");
            shell.workspace.archived_projects = vec!["/work/old".into()];
            shell.workspace.project_appearance.insert(
                "/work/monocode".into(),
                monocode_core::project_appearance::ProjectAppearance {
                    name: "MonoCode".into(),
                    color: Some(0x459cf7),
                    ..Default::default()
                },
            );
        })
        .unwrap();
    let first = projects(window, None, Some(2), cx).unwrap();
    assert_eq!(first.total, 4);
    let paths: Vec<_> = first.projects.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, ["/work/another-project", "/work/monocode"]);
    let current = &first.projects[1];
    assert!(current.current && !current.archived);
    assert_eq!(
        (current.name.as_str(), current.color.as_str()),
        ("MonoCode", "#459cf7")
    );
    assert_eq!(
        current.key,
        monocode_core::files::path_key("/work/monocode")
    );
    assert_eq!(first.projects[0].name, "another-project");
    assert!(first.projects[0].color.starts_with('#') && first.projects[0].color.len() == 7);
    assert_eq!(first.next_cursor.as_deref(), Some("2"));

    let rest = projects(window, first.next_cursor.as_deref(), Some(2), cx).unwrap();
    let tail: Vec<_> = rest
        .projects
        .iter()
        .map(|p| (p.path.as_str(), p.location, p.archived))
        .collect();
    assert_eq!(
        tail,
        [
            ("remote://box/srv/app", ProjectLocation::Remote, false),
            ("/work/old", ProjectLocation::Local, true),
        ]
    );
    assert_eq!(rest.next_cursor, None);
    let bad = projects(window, Some("99"), None, cx).unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidInput);
    let zero = projects(window, None, Some(0), cx).unwrap_err();
    assert_eq!(zero.code, ErrorCode::InvalidInput);
}

#[gpui::test]
fn model_list_filters_caps_and_counts_omitted(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Claude Code", &[Scope::Read])], cx);
    window
        .update(cx, |shell, _, _| shell.control.models = fixture_models)
        .unwrap();
    let all = models(window, ModelListInput::default(), cx).unwrap();
    let ids: Vec<_> = all.providers.iter().map(|p| p.id.as_str()).collect();
    let expected: Vec<_> = Kind::ALL.iter().map(|k| k.id()).collect();
    assert_eq!(ids, expected, "every provider, in the app's order");
    let claude = &all.providers[0];
    assert_eq!(
        (claude.label.as_str(), claude.available),
        ("Claude Code", true)
    );
    assert_eq!((claude.models.len(), claude.omitted), (3, 0));
    let cursor = all.providers.iter().find(|p| p.id == "cursor").unwrap();
    assert!(!cursor.available && cursor.models.is_empty());

    let one = models(
        window,
        ModelListInput {
            provider: Some("codex".into()),
            search: Some("  OPUS ".into()),
            limit: None,
        },
        cx,
    )
    .unwrap();
    assert_eq!(one.providers.len(), 1, "only the asked provider is queried");
    assert_eq!(one.providers[0].models[0].id, "codex-opus");
    assert_eq!(one.providers[0].models.len(), 1);

    let capped = models(
        window,
        ModelListInput {
            provider: Some("claude".into()),
            search: None,
            limit: Some(2),
        },
        cx,
    )
    .unwrap();
    assert_eq!(capped.providers[0].models.len(), 2);
    assert_eq!(capped.providers[0].omitted, 1);

    let unknown = models(
        window,
        ModelListInput {
            provider: Some("nope".into()),
            ..Default::default()
        },
        cx,
    )
    .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::InvalidInput);
    let too_many = models(
        window,
        ModelListInput {
            limit: Some(101),
            ..Default::default()
        },
        cx,
    )
    .unwrap_err();
    assert_eq!(too_many.code, ErrorCode::InvalidInput);

    window
        .update(cx, |shell, _, _| shell.control.models = failing_models)
        .unwrap();
    let failed = models(window, ModelListInput::default(), cx).unwrap_err();
    assert_eq!(failed.code, ErrorCode::ProviderUnavailable);
}

#[test]
fn shaping_keeps_errors_and_ignores_malformed_rows() {
    let catalog = json!({"harnesses": [
        {"id": "claude", "available": true, "models": [{"id": "a", "name": "A"}], "error": "signed out"},
        {"id": "mystery", "models": "not a list"},
    ]});
    let shaped = exec::model::shape(&catalog, None, 20);
    assert_eq!(shaped.providers[0].error.as_deref(), Some("signed out"));
    assert_eq!(shaped.providers[1].label, "mystery");
    assert!(!shaped.providers[1].available && shaped.providers[1].models.is_empty());
    assert!(
        exec::model::shape(&json!({}), None, 20)
            .providers
            .is_empty()
    );
}

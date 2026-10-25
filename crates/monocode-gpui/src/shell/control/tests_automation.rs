//! Automations against the fixture shell's in-memory service. Runs launch
//! in a project folder that does not exist, so no provider ever starts.
use super::tests::{run, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    AccessMode, Command, Scope,
    ops::automation::{
        AutomationInput, AutomationListInput, AutomationListOutput, AutomationReadInput,
        AutomationReadOutput, AutomationRunOutput, AutomationSetEnabledInput, AutomationSummary,
        AutomationUpsertInput, AutomationUpsertOutput, Schedule, ScheduleKind,
    },
};
use monocode_core::automations::{AutomationUpsert, Service};

const HERE: &str = "/work/monocode";

pub(super) fn service(
    window: gpui::WindowHandle<Shell>,
    cx: &mut TestAppContext,
) -> Arc<Mutex<Service>> {
    window
        .update(cx, |shell, _, _| shell.automation_state.service.clone())
        .unwrap()
        .expect("the fixture opens its in-memory automations")
}

/// Saves a manual automation that is off, with `runtime` as its run mode.
pub(super) fn seed(
    window: gpui::WindowHandle<Shell>,
    name: &str,
    runtime: &str,
    cx: &mut TestAppContext,
) -> String {
    let mut draft = AutomationUpsert::new(HERE.into(), "codex".into(), "gpt-5".into());
    draft.name = name.into();
    draft.prompt = "Review the open pull requests".into();
    draft.runtime_mode = runtime.into();
    draft.enabled = false;
    draft.sync_triggers(now_ms());
    service(window, cx)
        .lock()
        .unwrap()
        .upsert(draft)
        .unwrap()
        .id
}

fn stored(
    window: gpui::WindowHandle<Shell>,
    id: &str,
    cx: &mut TestAppContext,
) -> AutomationSummary {
    let all = service(window, cx).lock().unwrap().list().unwrap();
    exec::automation::summary(all.iter().find(|a| a.id == id).unwrap())
}

/// A new automation's required fields.
fn creating() -> AutomationUpsertInput {
    AutomationUpsertInput {
        name: Some("Morning triage".into()),
        prompt: Some("Triage new issues".into()),
        harness: Some("codex".into()),
        model: Some("gpt-5".into()),
        ..AutomationUpsertInput::default()
    }
}

fn upsert(id: Option<&str>, mode: Option<AccessMode>) -> Command {
    let input = match id {
        Some(id) => AutomationUpsertInput {
            automation_id: Some(id.to_owned()),
            ..AutomationUpsertInput::default()
        },
        None => creating(),
    };
    Command::AutomationUpsert(AutomationUpsertInput {
        runtime_mode: mode,
        ..input
    })
}

#[gpui::test]
fn automations_with_full_access_are_refused_and_others_run(cx: &mut TestAppContext) {
    let (window, ..) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let auto = seed(window, "Auto review", "auto", cx);
    let full = seed(window, "Full review", "full-access", cx);
    let refused = |error: ControlError| {
        assert_eq!(error.code, ErrorCode::InvalidInput, "{}", error.message);
        assert!(error.message.contains("full access"), "{}", error.message);
    };
    let enable = |id: &str, enabled| {
        Command::AutomationSetEnabled(AutomationSetEnabledInput {
            automation_id: id.to_owned(),
            enabled,
        })
    };

    // Full access is refused (7.4): nothing is run, turned on or saved.
    let run_full = Command::AutomationRun(AutomationInput {
        automation_id: full.clone(),
    });
    refused(run::<AutomationRunOutput>(window, run_full, cx).unwrap_err());
    refused(run::<AutomationSummary>(window, enable(&full, true), cx).unwrap_err());
    refused(
        run::<AutomationUpsertOutput>(window, upsert(None, Some(AccessMode::FullAccess)), cx)
            .unwrap_err(),
    );
    refused(
        run::<AutomationUpsertOutput>(
            window,
            upsert(Some(&auto), Some(AccessMode::FullAccess)),
            cx,
        )
        .unwrap_err(),
    );
    let runs = service(window, cx).lock().unwrap().runs(&full).unwrap();
    assert!(runs.is_empty(), "a refused run records nothing");
    assert!(!stored(window, &full, cx).enabled);
    assert_eq!(service(window, cx).lock().unwrap().list().unwrap().len(), 2);
    assert_eq!(
        stored(window, &auto, cx).runtime_mode,
        AccessMode::Auto,
        "a refused save changes nothing"
    );
    // Turning off always works.
    let off: AutomationSummary = run(window, enable(&full, false), cx).unwrap();
    assert!(!off.enabled);

    // Any other mode is allowed, auto included.
    let run_auto = Command::AutomationRun(AutomationInput {
        automation_id: auto.clone(),
    });
    let on: AutomationSummary = run(window, enable(&auto, true), cx).unwrap();
    assert!(on.enabled);
    let started: AutomationRunOutput = run(window, run_auto, cx).unwrap();
    assert_eq!(started.automation_id, auto);
    let runs = service(window, cx).lock().unwrap().runs(&auto).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, started.run_id);
    assert_eq!(
        runs[0].status, "failed",
        "the fixture's project folder is not on disk"
    );
    let logged = window
        .update(cx, |shell, _, _| {
            let last = shell.control.activity.entries().last().unwrap();
            (last.operation.clone(), last.target.clone())
        })
        .unwrap();
    assert_eq!(
        logged,
        (
            "automation_run".to_owned(),
            Some(format!("automation {auto}"))
        )
    );
}

#[gpui::test]
fn automations_are_listed_read_created_and_changed(cx: &mut TestAppContext) {
    // Creating one schedules its prompt, which needs dispatch too.
    let (window, ..) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let first = seed(window, "Nightly review", "supervised", cx);
    let listed: AutomationListOutput = run(
        window,
        Command::AutomationList(AutomationListInput::default()),
        cx,
    )
    .unwrap();
    assert_eq!(listed.automations.len(), 1);
    assert_eq!(listed.automations[0].runtime_mode, AccessMode::Supervised);
    assert_eq!(listed.automations[0].trigger, "Manual only");
    let elsewhere: AutomationListOutput = run(
        window,
        Command::AutomationList(AutomationListInput {
            project: Some("/work/another-project".into()),
            ..AutomationListInput::default()
        }),
        cx,
    )
    .unwrap();
    assert!(elsewhere.automations.is_empty());

    let daily = Schedule {
        kind: ScheduleKind::Daily,
        minute: None,
        time: Some("07:30".into()),
        day_of_week: None,
    };
    let created: AutomationUpsertOutput = run(
        window,
        Command::AutomationUpsert(AutomationUpsertInput {
            schedules: Some(vec![daily.clone()]),
            enabled: Some(false),
            ..creating()
        }),
        cx,
    )
    .unwrap();
    assert!(created.created);
    assert_eq!(created.automation.project, HERE);
    assert_eq!(created.automation.runtime_mode, AccessMode::Supervised);
    assert_eq!(created.automation.trigger, "Daily at 07:30");

    let renamed: AutomationUpsertOutput = run(
        window,
        Command::AutomationUpsert(AutomationUpsertInput {
            automation_id: Some(created.automation.id.clone()),
            name: Some("Early triage".into()),
            ..AutomationUpsertInput::default()
        }),
        cx,
    )
    .unwrap();
    assert!(!renamed.created);
    assert_eq!(renamed.automation.name, "Early triage");
    assert_eq!(
        renamed.automation.trigger, "Daily at 07:30",
        "unnamed fields are kept"
    );

    service(window, cx)
        .lock()
        .unwrap()
        .run_now(&first, now_ms())
        .unwrap();
    let read: AutomationReadOutput = run(
        window,
        Command::AutomationRead(AutomationReadInput {
            automation_id: first.clone(),
            runs: None,
        }),
        cx,
    )
    .unwrap();
    assert_eq!(read.prompt.text, "Review the open pull requests");
    assert_eq!(read.runs.len(), 1);
    assert_eq!(read.runs[0].trigger, "manual");
    let read: AutomationReadOutput = run(
        window,
        Command::AutomationRead(AutomationReadInput {
            automation_id: renamed.automation.id,
            runs: None,
        }),
        cx,
    )
    .unwrap();
    assert_eq!(read.schedules, [daily]);

    let bad = run::<AutomationUpsertOutput>(
        window,
        Command::AutomationUpsert(AutomationUpsertInput {
            automation_id: Some(first.clone()),
            schedules: Some(vec![Schedule {
                kind: ScheduleKind::Daily,
                minute: None,
                time: Some("25:99".into()),
                day_of_week: None,
            }]),
            ..AutomationUpsertInput::default()
        }),
        cx,
    );
    assert_eq!(
        bad.unwrap_err().code,
        ErrorCode::InvalidInput,
        "the service validates schedules"
    );
    let missing = run::<AutomationReadOutput>(
        window,
        Command::AutomationRead(AutomationReadInput {
            automation_id: "nope".into(),
            runs: None,
        }),
        cx,
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
}

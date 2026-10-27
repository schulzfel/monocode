//! `control_grant` for a Mono's calls: every operation of the catalog is
//! refused to a Mono unless it is one a Mono may call, and the sessions'
//! decisions stay theirs. What only a Mono may call is refused to everyone
//! else.
use super::gate::{HABIT_MUTATIONS, MONO_OPS, READ_ONLY};
use super::tests::{CLIENT, approve, sessions, setup};
use super::tests_gate::{grant, sample};
use super::*;
use gpui::TestAppContext;
use monocode_control::catalog;

#[gpui::test]
fn a_mono_may_call_only_a_monos_operations_and_sessions_keep_theirs(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, b) = sessions(window, cx);
    // A Mono whose main chat is session 9001, assigned the project of the
    // session the calls name.
    let chat = 9001;
    let mono = window
        .update(cx, |shell, _, cx| {
            shell.mono_manager().update(cx, |monos, _| {
                let id = monos
                    .roster
                    .create(vec!["/work/another-project".into()])
                    .id
                    .clone();
                monos.roster.get_mut(&id).unwrap().session_id = Some(chat);
                id
            })
        })
        .unwrap();
    let me = CallerKind::Mono {
        session: chat,
        mono,
        run: false,
    };
    let thread = CallerKind::Session(b);
    let mut allowed = Vec::new();
    for descriptor in catalog() {
        let name = descriptor.name;
        let command = sample(&descriptor, a);
        let denied = format!("Monos cannot use {name}");
        match grant(window, &me, &command, cx) {
            Ok(_) => allowed.push(name),
            Err(error) if MONO_OPS.contains(&name) => {
                // Its chat is not loaded here, so it may only read.
                assert!(!descriptor.hints.read_only, "{name}: {error:?}");
                assert_eq!(error.message, READ_ONLY, "{name}");
            }
            Err(error) => {
                assert_eq!(
                    (error.code, error.message.as_str()),
                    (ErrorCode::ScopeDenied, denied.as_str()),
                    "{name}"
                );
                if let Err(error) = grant(window, &thread, &command, cx) {
                    assert_ne!(error.message, denied, "a session may call {name}");
                }
            }
        }
    }
    let reads: Vec<_> = catalog()
        .into_iter()
        .filter(|d| d.hints.read_only && MONO_OPS.contains(&d.name))
        .map(|d| d.name)
        .collect();
    assert_eq!(allowed, reads);
    assert!(
        MONO_OPS
            .iter()
            .all(|op| catalog().iter().any(|d| d.name == *op))
    );
}

/// Every operation only a Mono may call is refused to external clients and
/// to agent sessions, in the same words; a Mono's chat passes that rule, and
/// the habit mutations its runs may not call are the real operations.
#[gpui::test]
fn only_a_mono_may_call_a_monos_own_operations(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, b) = sessions(window, cx);
    let chat = 9002;
    let mono = window
        .update(cx, |shell, _, cx| {
            shell.mono_manager().update(cx, |monos, _| {
                let id = monos.roster.create(Vec::new()).id.clone();
                monos.roster.get_mut(&id).unwrap().session_id = Some(chat);
                id
            })
        })
        .unwrap();
    let me = CallerKind::Mono {
        session: chat,
        mono,
        run: false,
    };
    let others = [CallerKind::External(CLIENT.into()), CallerKind::Session(b)];
    let mut mono_only = Vec::new();
    for descriptor in catalog().into_iter().filter(|d| d.mono_only) {
        let name = descriptor.name;
        let command = sample(&descriptor, a);
        assert!(command.mono_only(), "{name}");
        for caller in &others {
            let refused = grant(window, caller, &command, cx).unwrap_err();
            assert_eq!(
                (refused.code, refused.message),
                (
                    ErrorCode::ScopeDenied,
                    format!("{name} is only available in a Mono's chat")
                ),
                "{caller:?}"
            );
        }
        match grant(window, &me, &command, cx) {
            Ok(_) => assert!(descriptor.hints.read_only, "{name}"),
            // Its chat is not loaded here, so it may only read.
            Err(error) => assert_eq!(error.message, READ_ONLY, "{name}"),
        }
        assert!(MONO_OPS.contains(&name), "{name} is in a Mono's allowlist");
        mono_only.push(name);
    }
    assert_eq!(mono_only.len(), 16);
    assert!(
        HABIT_MUTATIONS
            .iter()
            .all(|op| mono_only.contains(op) && *op != "mono_habit_list")
    );
}

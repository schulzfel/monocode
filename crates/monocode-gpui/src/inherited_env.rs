//! A window started from inside a MonoCode session (its terminal, or an
//! agent's shell) inherits that session's control variables, which are its
//! credential: every child the window starts without an environment of its
//! own (an orchestration worker, a remote session, a terminal) would call
//! the other window as that session. The window forgets them first thing;
//! the `app` and `mcp` commands, which need them, run before it does.

/// Forgets the control variables this process inherited
/// (`app_mcp::CONTROL_ENV`). Call it before any thread starts.
pub(crate) fn forget() {
    for key in monocode_core::app_mcp::CONTROL_ENV {
        // SAFETY: `main` calls this before it starts any thread (and the
        // test calls it in a process of its own, on its only test thread),
        // so nothing reads or writes the environment at the same time.
        unsafe { std::env::remove_var(key) };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use monocode_core::app_mcp::CONTROL_ENV;
    use std::process::Command;

    /// Set in the copy of this test that runs as the window would.
    const CHILD: &str = "MONOCODE_TEST_FORGET_INHERITED";

    /// R18: a window started with another session's control variables
    /// forgets them, so a child it starts without any gets none; other
    /// variables stay. The check runs in a copy of this test binary, as
    /// changing the environment is only sound in a process of one thread.
    #[test]
    fn a_window_forgets_the_control_variables_it_inherited() {
        if std::env::var_os(CHILD).is_some() {
            super::forget();
            let child = Command::new("/usr/bin/env").output().unwrap();
            let env = String::from_utf8_lossy(&child.stdout);
            assert!(!env.contains("MONOCODE_CONTROL_"), "{env}");
            assert!(env.contains("MONOCODE_TEST_UNRELATED=kept"), "{env}");
            return;
        }
        let mut copy = Command::new(std::env::current_exe().unwrap());
        copy.args([
            "--exact",
            "inherited_env::tests::a_window_forgets_the_control_variables_it_inherited",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env("MONOCODE_TEST_UNRELATED", "kept");
        for key in CONTROL_ENV {
            copy.env(key, "inherited");
        }
        let ran = copy.output().unwrap();
        let out = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "{out}");
        assert!(out.contains("1 passed"), "{out}");
    }
}

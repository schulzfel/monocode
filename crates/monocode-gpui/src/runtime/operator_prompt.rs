//! The instructions `/operator` adds to every turn of its thread (spec
//! §10.1): which MonoCode tools this turn's agent has, as `Access` says,
//! and the thread's project.
use monocode_core::app_mcp::Access;

/// What a message of only `/operator` asks.
pub(crate) const EMPTY_REQUEST: &str =
    "Explain what you can do in MonoCode with your MonoCode tools.";

/// `prompt` after the Operator instructions for a turn with `access` in a
/// thread of project `cwd`; `exe` is the shell-quoted executable of the
/// `app` command.
pub(crate) fn wrap(access: Access, cwd: &str, exe: &str, prompt: &str) -> String {
    match access {
        Access::Mcp => format!(
            "You have MonoCode access in this thread through the `monocode` MCP tools, for example session_start, session_list, session_read, session_send, folder_list, session_move, notes_write and worktree_create. Work in this thread's project ({cwd}) unless the person names another. Respect the person's request and this session's access mode.\n\n{prompt}"
        ),
        Access::Cli => format!(
            "You have MonoCode app access in this thread. Use the CLI: {exe} app OPERATION --json JSON. Run {exe} app help for every operation and its inputs. Work in this thread's project ({cwd}) unless the person names another. App credentials are in the environment; never print them. Respect the person's request and this session's access mode.\n\n{prompt}"
        ),
        Access::Unavailable(reason) => format!(
            "MonoCode tools are unavailable in this run ({reason}); tell the person if they ask you to act in MonoCode.\n\n{prompt}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/work/monocode";
    const EXE: &str = "'/Applications/Mono Code.app/monocode-gpui'";

    /// T-P1: a provider that gets the relay is told its MCP tools.
    #[test]
    fn an_mcp_turn_is_told_its_tools() {
        assert_eq!(
            wrap(Access::Mcp, CWD, EXE, "List my notes"),
            "You have MonoCode access in this thread through the `monocode` MCP tools, for example session_start, session_list, session_read, session_send, folder_list, session_move, notes_write and worktree_create. Work in this thread's project (/work/monocode) unless the person names another. Respect the person's request and this session's access mode.\n\nList my notes"
        );
    }

    /// T-P1: a provider without MCP is told the `app` command and its help.
    #[test]
    fn a_cli_turn_is_told_the_app_command() {
        assert_eq!(
            wrap(Access::Cli, CWD, EXE, "List my notes"),
            "You have MonoCode app access in this thread. Use the CLI: '/Applications/Mono Code.app/monocode-gpui' app OPERATION --json JSON. Run '/Applications/Mono Code.app/monocode-gpui' app help for every operation and its inputs. Work in this thread's project (/work/monocode) unless the person names another. App credentials are in the environment; never print them. Respect the person's request and this session's access mode.\n\nList my notes"
        );
    }

    /// T-P1, §8.3: a turn without the control environment is told why.
    #[test]
    fn an_unavailable_turn_is_told_why() {
        assert_eq!(
            wrap(
                Access::Unavailable("MonoCode's control socket is not served"),
                CWD,
                EXE,
                "List my notes"
            ),
            "MonoCode tools are unavailable in this run (MonoCode's control socket is not served); tell the person if they ask you to act in MonoCode.\n\nList my notes"
        );
    }

    /// T-P1: what a message of only `/Operator` asks (OP-4).
    #[test]
    fn an_empty_request_asks_what_the_tools_can_do() {
        assert_eq!(
            EMPTY_REQUEST,
            "Explain what you can do in MonoCode with your MonoCode tools."
        );
    }
}

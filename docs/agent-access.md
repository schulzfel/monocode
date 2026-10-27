# Agent access with /operator

## MonoCode tools

Every local agent session MonoCode runs can see and drive the app with MonoCode's tools, with no setup. Claude Code, Codex and the ACP providers get them as the `monocode` MCP server ([MCP server](native/mcp.md#agents-inside-monocode)). Pi, OpenCode and Omp take no MCP servers, so their agents run the same tools through the local `app` CLI: `monocode-gpui app OPERATION --json JSON`, where `app help` lists every operation with its inputs. The CLI reads the session's credentials from its environment and acts only while the app runs. Remote sessions and orchestration workers get neither.

## Focus a thread with /operator

Type `/operator` at the start of a composer message to point that thread's agent at MonoCode. For example, `/operator start two Codex sessions: one to inspect the API and one to review the UI`, or `/operator list my notes`. The slash picker also offers this command. Its chip and menu item are sky blue, and the sent request plays a short amber sparkle; the transcript shows only the request text. MonoCode removes the command from the request and adds instructions on that turn and every later turn of the thread: which MonoCode tools the agent has, how to call them, and to work in this thread's project unless you name another. When the tools are unavailable in a run, for example because the app could not serve its control socket, the instructions say so and the window shows the reason.

## Read-only turns

In plan mode, on an orchestration lead's turns and with the read-only access mode, MonoCode tools that change something are refused with "This thread's access policy allows read-only app actions."; listing and reading still work. This applies to every session, with or without `/operator`. Otherwise whether a call runs is decided by the session's own permission mode, as for any of its tools.

## Tools

The CLI takes each tool's name as its operation and still accepts the old dotted names, shown in brackets, so existing scripts keep working.

Sessions started by an ordinary agent remain visible in project history.
Monos use the same tools, with their **Show started sessions** setting
controlling whether their launches appear in the user's history. This is
independent of navigation: `reveal` defaults to `false`, so a background launch
can stay visible in history without switching the user's current conversation.

- `model_list` (`models.list`) shows available providers, models, settings, and access modes.
- `session_start` (`sessions.start`) creates a session in the current project with a prompt. Set `reveal: true` to open its tab, or `placement: "right"` / `placement: "down"` to split the calling session's pane; `besideSessionId` selects another visible session pane in the project. Reuse the returned session ID as the next `besideSessionId` to build nested layouts. By default it submits the prompt; set `draft: true` to save it unsent without starting an agent turn. It accepts a provider, model, effort or other model settings, access mode, and current checkout or new worktree choice. Set `worktreeCwd` to a path from `worktree_list` for a specific existing checkout. Use `worktree_create` to create a worktree on a named new or existing local branch, then pass its path as `worktreeCwd`. Omit `access` to inherit the calling session's access mode, or set it explicitly to override. It returns the new session ID as soon as the pane and prompt are accepted, so the agent can move it into a folder immediately.
- `session_list` (`sessions.list`) shows project sessions and their archived status. `session_read` (`sessions.read`) returns up to three recent user/assistant exchanges, with a cursor for older exchanges and a per-message character cap. `session_send` (`sessions.send`) sends a follow-up to another session; the CLI's old name sends it only into an idle session, as `ifIdle: true` does: a session that is still loading gets it once loaded, a busy session or one with unsent text in its composer refuses it, and a saved draft also refuses it except for a Mono. `session_draft` (`sessions.draft`) saves an unsent message for the user to review. `folder_list` and `session_move` (`folders.list`, `folders.move`) organize project sessions in sidebar folders, including a new folder.
- `session_stop`, `session_archive` and `session_delete` (`sessions.stop`, `sessions.archive`, `sessions.delete`) take a `sessionId` to manage another session in the project. Monos can select an assigned project with `project`. Stop cancels the current turn and pauses queued messages; archive saves the conversation for later restoration, and stops it first with `stop: true` (always for `sessions.archive`); delete stops and permanently removes the conversation. Open files, terminals and worktrees are kept. These actions cannot target the caller, Mono chats, habit runs or orchestration workers. Reuse the same request ID when retrying a call.
- A session a Mono starts reports back to the Mono when it stops, unless it was saved as a draft; set `notifyOnComplete: false` to start it without a report, or `notifyOnComplete: true` on `session_send` to get one for a follow-up.
- When a Mono successfully stops, archives or deletes a session it is monitoring, its pending completion report for that session is dismissed, including any queued report. The Mono confirms the action in its current reply. Reports for other sessions and other Monos are kept. Rejected launches or follow-ups return an error without a later completion report.
- `notes_list` (`notes.list`) returns titles and short previews; `notes_read` (`notes.read`) returns one full note by ID.
- `notes_list` accepts `limit` and `cursor` (`offset` for the old name). `notes_write` (`notes.write`) creates a note from its
  `body`, with optional title and tags; pass an existing note `id` to update just
  its title, body or tags while preserving the other fields.

Retry an uncertain change with the same request ID (`requestId`, or the CLI's `--request-id`) and identical input.
Unknown fields and invalid options produce errors instead of being ignored.

## Orchestration workers

Orchestration workers keep their existing scoped `control` workflow and get no MonoCode tools: no MCP server and no `app` credentials. Only their lead manages them.

Return to the [README](../README.md) for a quick start.

# MCP server

MonoCode serves MCP over Streamable HTTP at `http://127.0.0.1:6262/mcp`
while the person has it on, the way desktop apps expose one. An agent in
another app (Claude Code, Codex, Cursor, VS Code, Claude Desktop) sees and
drives the running window: list and open projects and sessions, start and
message agents, organise the sidebar, use source control, run automations and
talk to Monos. Changes appear in the window as if the person made them.

## Turn it on

The MCP server is off by default, and only the app can turn it on.

```text
Settings → General → MCP server
  MCP server     on                            # serve MCP on localhost
  Server URL     http://127.0.0.1:6262/mcp     # Copy puts it on the clipboard
  Add to         Claude Code  Codex  Cursor  VS Code
```

Off, nothing listens on the port, and clients of the stdio relay are told
`control_disabled`. The app still listens on its control socket for its own
agent sessions ([Agents inside MonoCode](#agents-inside-monocode)), whatever
the switch says. While it is on, the same group lists approved clients with
when each was last used, their scopes and Revoke, clients denied in the last
10 minutes with Allow, and Recent activity, collapsed until opened. The log
records every call that may change something (refused or not) and every
approval and confirmation decision, at most 200 entries; reads are not
logged.

| Data the window opened | Default port |
| --- | --- |
| The installed app's (release builds, `--monocode-data`, `--monocode-db PATH`) | `6262` |
| Development (`--dev-data`, development builds) | `6263` |

The two differ so the installed app and a development build serve at once;
IANA has assigned neither. When another app holds the port, the Server URL
row says so and offers a port field; the port chosen there is kept in the
policy (`external-control.v1`, `port`).

The server listens on `127.0.0.1` only and answers MCP at `/mcp` (other paths
are `404`). Against DNS rebinding, as the MCP spec asks, a request is refused
with `403` unless its `Host` is `127.0.0.1:<port>` or `localhost:<port>`, and
one carrying an `Origin` other than `http://127.0.0.1:<port>` or
`http://localhost:<port>` is refused too; requests without `Origin` pass, as
non-browser clients send none.

Both protocol versions are served: 2025-11-25 sessions (`initialize`,
`Mcp-Session-Id`, server-sent events, `resources/subscribe`) and 2026-07-28
stateless requests (`server/discover`, per-request `_meta`,
`subscriptions/listen`).

## Connect a client

The Add to buttons do this with the window's own URL: Claude Code and Codex
copy the command, Cursor and VS Code open their install link. The examples
use the default port of the installed app's data.

### Claude Code

```sh
claude mcp add --transport http monocode http://127.0.0.1:6262/mcp
```

### Codex

```sh
codex mcp add monocode --url http://127.0.0.1:6262/mcp
```

### Cursor

```text
cursor://anysphere.cursor-deeplink/mcp/install?name=monocode&config=eyJ1cmwiOiJodHRwOi8vMTI3LjAuMC4xOjYyNjIvbWNwIn0=
```

`config` is the base64 of `{"url":"http://127.0.0.1:6262/mcp"}`, the server's
entry as Cursor's `mcp.json` writes a remote server. By hand, in
`~/.cursor/mcp.json`:

```json
{ "mcpServers": { "monocode": { "url": "http://127.0.0.1:6262/mcp" } } }
```

### VS Code

```text
vscode:mcp/install?%7B%22name%22%3A%22monocode%22%2C%22type%22%3A%22http%22%2C%22url%22%3A%22http%3A%2F%2F127.0.0.1%3A6262%2Fmcp%22%7D
```

The query is `{"name":"monocode","type":"http","url":"http://127.0.0.1:6262/mcp"}`
encoded as JavaScript's `encodeURIComponent` does.

### Check it with curl

```sh
curl -s -X POST http://127.0.0.1:6262/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
```

The answer is an event stream whose `data:` line carries the `initialize`
result with `serverInfo` `monocode`. Tool calls then ask for approval, below.

### Client name

Approvals, grants and the activity log are keyed on the client's
`clientInfo.name`: from `initialize` on 2025-11-25, from each request's
`_meta` on 2026-07-28. A request that names no client is named after the
first product of its `User-Agent`, without the version (`curl/8.7.1` is
`curl`), else `MCP client`.

## Advanced: stdio

`monocode-gpui mcp` is the same MCP server on standard input and output, a
relay that forwards each call over the data folder's control socket to the
running window. MonoCode's own agent sessions use it. Settings does not show
it; a client that only speaks stdio can run it. It alone offers
`app_launch`, which starts MonoCode when it is not running (only while the
MCP server is on) and waits for it.

```sh
claude mcp add monocode -- "/Applications/MonoCode.app/Contents/MacOS/monocode-gpui" mcp
```

```toml
# ~/.codex/config.toml
[mcp_servers.monocode]
command = "/Applications/MonoCode.app/Contents/MacOS/monocode-gpui"
args = ["mcp"]
```

```sh
monocode-gpui mcp                          # the data folder this build opens by default
monocode-gpui mcp --dev-data               # the isolated development folder
monocode-gpui mcp --monocode-data          # the installed MonoCode database
monocode-gpui mcp --monocode-db /path/to/monocode.db   # a specific database
monocode-gpui mcp --client "Review bot"    # the name shown in MonoCode's prompts
```

The relay talks to the window that opened the same data folder, so it picks
its folder exactly as the app does: the `.app` bundle's data mode first, then
development data for development builds and the installed database for
release builds. A development build (`npm run native:dev`) lives under
`<target>/native-dev/Monocode Native.app` and uses
`~/Library/Application Support/Monocode Development/`. A release build uses
`com.monocode.desktop/` under Application Support. A development folder
chosen with `MONOCODE_NATIVE_DATA_DIR` is not a flag: give the client the
same variable. See the [native README](README.md#data-and-configuration) for
every platform's folders.

`--client` replaces the MCP client's reported name. Approval, scopes,
confirmations and the activity log are the same as over HTTP.

## Approval and scopes

The first time a client name connects, the window asks
“Allow “Claude Code” to control MonoCode?” (Allow / Deny). That prompt is the
only question: Allow grants `read`, `navigate`, `dispatch` and `manage` at
once, and nothing is asked again per scope. No answer within 2 minutes is a
refusal (`user_declined`) and asks again next time. Deny is remembered for
10 minutes, or until Settings' Allow or Revoke for that name: the client's
hellos are refused with `user_declined` without a prompt and without
bringing the window forward. An approved client is remembered; each scope is
a switch in Settings.

| Scope | Allows | New client |
| --- | --- | --- |
| `read` | lists, reads, status, search, resources | on |
| `navigate` | select projects, open, focus, move and close tabs, panes, layout, open a Mono | on |
| `dispatch` | start, message, wait for, stop, compact and hand off sessions; answer questions; steer, edit and resume queued messages; run automations; message Monos | on |
| `manage` | add, archive and organise projects and sessions, worktrees, notes, edit automations (turning one on, creating one or setting its prompt also needs `dispatch`) | on |
| `settings` | the allowlisted preferences | off |
| `vcs` | stage, unstage and commit in local projects; discard and push, each confirmed | off |
| `destructive` | delete sessions, projects and automations, remove queued messages, each confirmed | off |

Policies saved before the `vcs` scope existed keep their scopes; clients
approved earlier start without `vcs`. Revoking a client removes its grant, and
its next connection asks again: over HTTP that is its next 2025-11-25
session, or its next 2026-07-28 request, since each stateless request says
hello on its first call. Revoking, taking away `read` or turning control off
also ends every subscription of that client: its connection is told why
(`scope_denied` or `control_disabled`) and closed. Turning the MCP server off
ends every HTTP session. Client identity is the self-reported name alone, so
approval guards against surprises, not impersonation.

## Access modes

Sessions a client starts, configures or hands off get the access mode it asks
for. Without one, `session_start` and `session_handoff` use the app's default
for new chats, and `session_configure` keeps the session's mode. The modes,
lowest first:

```text
read-only < supervised < auto-accept-edits < auto < full-access
```

A client never runs, turns on or saves an automation with full access
(`automation_run`, `automation_set_enabled`, `automation_upsert`): that fails
with `invalid_input`. Sessions started from a client record `mcp:<client>` as
their origin and never get Operator or plan capability. Like every local
session they get MonoCode's own MCP server
([Agents inside MonoCode](#agents-inside-monocode)); what they call through it
is decided by their access mode.

`session_configure` changes only that session: no default access, effort or
recent model is remembered for the person's new sessions.

## Confirmations

`session_delete`, `project_delete`, `automation_delete`, `queue_remove`,
`git_discard` and `git_push` ask the person in the window before they run.

```text
“Claude Code” wants to delete the session “Fix the parser”. Delete it?
  This conversation and its saved settings will be removed permanently.
  [Delete] [Cancel]
```

The prompt names the client and the exact target: the session or automation,
the project and its session count, the queued message's text, the files to
discard, the branch, commit and remote to push to. Cancel, or no answer
within 2 minutes, fails the call with `user_declined` and changes nothing. A
declined call keeps no receipt, so a retry with the same `requestId` asks
again. An answer after the 2 minutes does nothing.

`git_discard` accepts only files the repository reports as unstaged changes:
modified or deleted in the working tree, or untracked and not ignored. Each
file's folders, once links are resolved, must be inside the repository; a
link itself is reverted or deleted, never what it points to. Anything else
(an ignored, unchanged or missing file, a folder, a path outside) fails with
`invalid_input` before anyone is asked. The prompt lists which files go back
to their staged or committed version and which untracked files are deleted.
`git_push` shows the branch and its commit. Both act only on what the prompt
showed: when the files, the branch or its commit changed before the person
confirmed, the call fails with `request_conflict` (retryable) and changes
nothing. A push publishes that commit to the branch's upstream, or to the
default remote, which it then tracks.

Each client has at most one prompt open; a second confirmed call fails at once
with `request_conflict` (retryable). Turning external control off refuses
calls still waiting with `control_disabled`. The activity log records each
`confirm` and `decline` beside the call itself. `project_delete` never deletes
files: the folder stays on disk, and adding it again brings the project back
empty.

## Agents inside MonoCode

Every local agent session MonoCode runs gets this MCP server, named
`monocode`, with no setup and no approval prompt. An agent asked to "add
~/github.com/repost to MonoCode" calls `project_add`; the server's
instructions tell it that it runs inside MonoCode and that projects, tabs,
sessions and settings are managed through these tools, never by editing
MonoCode's database or files.

```text
<this executable> mcp --dev-data              # the stdio relay, started by the provider
MONOCODE_CONTROL_ENDPOINT=…/control/monocode-control.sock
MONOCODE_CONTROL_SESSION=42                   # the session the relay speaks for
MONOCODE_CONTROL_TOKEN=…                      # that session's credential for this app run
```

A session holds every scope, and MonoCode asks
nothing: no approval prompt for its hello and no confirmation before deletes,
discards or pushes. The MCP server may be on or off. Whether a call runs is
decided by the session's provider, as for any of its tools: Claude Code's
auto-mode classifier or its approval prompt in supervised mode, Codex
approvals, ACP permissions. Choose the session's access mode accordingly.

| Provider | How the server reaches it |
| --- | --- |
| Claude Code | `--mcp-config` on its command line; the server inherits Claude's environment |
| Codex | `-c mcp_servers.monocode.command/args/env_vars`; `env_vars` forwards the three variables by name |
| Cursor, Grok, Fx, Hermes, Antigravity, Devin (ACP) | `mcpServers` of `session/new` and `session/load`, with the variables in its `env` list |

The relay says hello as `session:<id>` with the session's credential. The
token is a digest of a secret made at app start and the session's id: it is
never stored, never on a command line, and a restarted app gives every
session a new one. A wrong token is refused with `scope_denied`, a session
that no longer exists with `not_found`; neither falls back to the external
approval. External clients cannot name themselves `session:…`.

A session cannot `session_stop`, `session_archive`, `session_delete`,
`session_configure` or `session_wait` its own session; those calls fail with
`invalid_input`. Everything else is allowed, including the same calls on
other sessions. The activity log in Settings lists a session's changes under
its title, and sessions it starts record it as their starter
(`startedBy: "session:<id>"`), as Operator launches do.

The transcript shows these calls by their titles, not their tool names:
`mcp__monocode__project_delete` reads as "Remove project repost-sh/monorepo",
the title followed by the project, session, path or prompt the call acts on.
Other MCP servers' tools read as their names in words with the server beside
them ("Create issue · Linear"), and Claude's deferred tool loader as "Load
tools Remove project, Archive project". The relay publishes the same titles to
every client as each tool's `title`. Only the display changes; the call is
recorded as the provider made it.

| Limitation | Why |
| --- | --- |
| Remote (`remote://`) sessions get no server | They run on another host, where neither the relay nor this socket exists |
| OpenCode, Pi and Omp sessions get no server | Those providers take no MCP configuration from MonoCode |
| A Grok or Fx session resumed in a new provider process gets no server | Their `session/resume` takes no MCP servers; the process that started the session keeps it |
| Text-only turns, and Devin turns that send a native slash command, get no server | They run without MonoCode's additions, as for native visuals |

## Tool reference

Every tool takes a JSON object and rejects unknown fields. Tools that change
something also accept `requestId` (`[A-Za-z0-9_-]{1,128}`): a retry with the
same ID and input returns the first result, and the same ID with other input
fails with `request_conflict`. A refusal and a failure marked `retryable`
keep no result, so the retry runs again. The app keeps each client's newest
200 results. Results carry structured content and a one-line text summary.

A scope in brackets is one some calls need as well; the tool's description
says which (for example, turning an automation on needs `dispatch` besides
`manage`, and is refused with `scope_denied` naming the missing one).

<!-- tools:start -->
| Tool | Scope | Hints | Confirmed | Purpose |
| --- | --- | --- | --- | --- |
| `app_info` | `read` | read-only, idempotent | no | Show the running MonoCode app: its version, which data folder it uses, the platform and the scopes granted to this client. |
| `model_list` | `read` | read-only, idempotent | no | List the agent providers MonoCode knows, whether each is installed, and a short list of each provider's models. |
| `project_list` | `read` | read-only, idempotent | no | List MonoCode's projects in rail order, then archived ones, with each project's name, color, whether it is current, and whether it is local or on a remote machine. |
| `project_select` | `navigate` | idempotent | no | Make a local project in MonoCode's rail the current one, as clicking it does. |
| `project_add` | `manage` | idempotent | no | Add an existing local folder to MonoCode's projects, optionally making it current. |
| `workspace_read` | `read` | read-only, idempotent | no | Show what MonoCode's window shows: the current project, the title tabs in order with stable ids for tab_focus and tab_close, the active tab's pane tree and focused pane, each pane's review tabs, the right panel and the layout. |
| `session_list` | `read` | read-only, idempotent | no | List the agent sessions of a local project (the current one by default), most recently active first, with each session's status, harness, model, archive state and who started it. |
| `session_read` | `read` | read-only, idempotent | no | Read a session's exchanges, newest last: each of the person's messages with the agent's last reply, cut to `maxChars`. |
| `session_status` | `read` | read-only, idempotent | no | Show what a session is doing: its status (idle, working, awaiting_approval, awaiting_answer, error…), pending approvals and questions, queued messages, context use, model and access mode. |
| `session_start` | `dispatch` | (none) | no | Start a new agent session in a local MonoCode project with a prompt, optionally choosing the harness, model, effort, access mode and a new or existing worktree. |
| `session_send` | `dispatch` | (none) | no | Send a follow-up message to a session. |
| `session_wait` | `dispatch` | read-only, idempotent | no | Wait until a session is idle, needs the person (an approval or a question), or failed, for at most `timeoutMs` (default 30 s, at most 60 s). |
| `session_stop` | `dispatch` | idempotent | no | Stop a session's running turn and pause its queued messages so they wait for the person. |
| `tab_open` | `navigate` | (none) | no | Show something in MonoCode's window: a session (`sessionId`, optionally placed `right` or `down` beside another pane), a file inside a local project (`path`, with an optional `:line:column`), the Inbox, Notes or Automations (`view`), or search. |
| `tab_focus` | `navigate` | idempotent | no | Make a title tab active (`tab`, an id from workspace_read) or focus a pane of a split (`pane`). |
| `tab_close` | `navigate` | idempotent | no | Close a title tab (`tab`) or one pane of a split (`pane`). |
| `session_configure` | `dispatch` | idempotent | no | Change an idle session's model, reasoning effort, other model settings or access mode; its next turn uses them. |
| `session_draft` | `dispatch` | idempotent | no | Leave an unsent message in an idle session's composer for the person to review and send. |
| `session_rename` | `manage` | idempotent | no | Rename a session in the sidebar and its tab. |
| `session_archive` | `manage` | destructive, idempotent | no | Archive an idle session: its pane closes and it leaves the sidebar. |
| `session_restore` | `manage` | idempotent | no | Restore an archived session to its project's sidebar. |
| `folder_list` | `manage` | read-only, idempotent | no | List a project's sidebar folders and the sessions in each, for session_move. |
| `session_move` | `manage` | (none) | no | Move a session into a sidebar folder of its project: an existing one by folderId, or a new one named newFolderName. |
| `question_list` | `read` | read-only, idempotent | no | List questions agents asked and are waiting to have answered, with their items and choices for question_answer, and the tool-permission approvals blocking sessions. |
| `question_answer` | `dispatch` | (none) | no | Answer the question a session is waiting on, as the question card in the app would: pick choices by id or label and type answers where allowed. |
| `queue_list` | `read` | read-only, idempotent | no | List the messages queued in a session to send after its running turn, in order, and whether the queue is paused. |
| `queue_remove` | `destructive` | destructive, idempotent | yes | Discard a queued message so it is never sent, after the person confirms in MonoCode. |
| `queue_steer` | `dispatch` | (none) | no | Send a queued message into the session's running turn now instead of after it, for providers that can take one (see session_send's steer mode). |
| `queue_resume` | `dispatch` | idempotent | no | Resume a paused queue, as the app's Resume or Retry button does: the interrupted turn continues and the queued messages follow. |
| `queue_edit` | `dispatch` | destructive, idempotent | no | Replace a queued message's text before it is sent; the text it had is gone. |
| `worktree_list` | `manage` | read-only, idempotent | no | List a local project's git worktrees and branches; session_start can work in one with worktreeCwd. |
| `worktree_create` | `manage` | (none) | no | Create a git worktree of a local project, on a new branch (from base, default HEAD) or an existing one, as the app's worktree menu does. |
| `project_update` | `manage` | idempotent | no | Change how a project appears in MonoCode's rail: its name, colour or mascot. |
| `project_archive` | `manage` | idempotent | no | Archive a project: its tabs close and it leaves the rail. |
| `project_unarchive` | `manage` | idempotent | no | Put an archived project back in the rail (project_list with archived projects shows them). |
| `project_reorder` | `manage` | idempotent | no | Reorder the project rail. |
| `tab_move` | `navigate` | (none) | no | Move a session's title tab, with all its panes, beside another pane (besideSessionId and edge), or detach a pane of a split into its own tab (no besideSessionId; optional index). |
| `pane_split` | `navigate` | (none) | no | Split a pane and show an existing session beside it, moving the session's pane there if it is open elsewhere. |
| `layout_set` | `navigate` | idempotent | no | Set the window's layout explicitly: Classic or Focused, the Focused layout's boxy frame, the Classic sidebar's tab, the right panel's open state and tab, and whether the primary sidebar shows. |
| `settings_read` | `settings` | read-only, idempotent | no | Read MonoCode's allowlisted preferences: theme, colour preset, accent colour, interface scale, transcript layout, diff view, word wrap, tab animations, sounds, notifications, autosave, format on save, follow-up mode, default provider and models, favourite models and remaining-usage display. |
| `settings_update` | `settings` | idempotent | no | Change MonoCode's allowlisted preferences (see settings_read); they apply at once, as in Settings. |
| `search_sessions` | `read` | read-only, idempotent | no | Search saved conversations by title and message text, as the app's search does; archived sessions are left out. |
| `search_project` | `read` | read-only, idempotent | no | Search the text of a local project's files (git grep where possible), as the app's project search does. |
| `notes_list` | `read` | read-only, idempotent | no | List notes of MonoCode's local projects, and notes that belong to no project. |
| `notes_read` | `read` | read-only, idempotent | no | Read one note's title, markdown text and tags. |
| `notes_write` | `manage` | (none) | no | Create a note in a local project, or change an existing note's title, text or tags. |
| `changes_read` | `read` | read-only, idempotent | no | Show what changed, read-only: a local project's uncommitted files with line counts, branch, ahead/behind and latest commits; or the files a session's last turn changed, as its review in the app shows them. |
| `session_compact` | `dispatch` | (none) | no | Compact an idle session's context window, as its /compact command does. |
| `session_handoff` | `dispatch` | (none) | no | Hand a session's last response to a new session on another harness or model, as the response's Hand off action does. |
| `session_delete` | `destructive` | destructive, idempotent | yes | Delete a session and its saved settings permanently, after the person confirms in MonoCode. |
| `project_delete` | `destructive` | destructive, idempotent | yes | Remove a local project from MonoCode and delete its saved sessions, after the person confirms in MonoCode. |
| `git_stage` | `vcs` | idempotent | no | Stage files of a local project (`git add`): the given paths, or every change with `all`. |
| `git_unstage` | `vcs` | idempotent | no | Unstage files of a local project (`git restore --staged`), keeping their changes: the given paths, or everything with `all`. |
| `git_commit` | `vcs` | (none) | no | Commit the staged changes of a local project with the given message. |
| `git_discard` | `vcs` | destructive, idempotent | yes | Discard unstaged changes in a local project, after the person confirms in MonoCode: tracked files go back to the index, untracked ones are deleted. |
| `git_push` | `vcs` | destructive, idempotent | yes | Push a local project's current branch to its upstream (or set the upstream on its remote on the first push), after the person confirms in MonoCode. |
| `automation_list` | `read` | read-only, idempotent | no | List automations (saved prompts that run on a schedule or on demand): name, project, harness, model, run mode, triggers, next and last run. |
| `automation_read` | `read` | read-only, idempotent | no | Read one automation: its definition (prompt, project, harness, model, run mode, workspace, schedules and event triggers) and its latest runs. |
| `automation_run` | `dispatch` | (none) | no | Run an automation now, as its Run now button does: a session starts with its prompt. |
| `automation_set_enabled` | `manage` (+ `dispatch`) | idempotent | no | Turn an automation's schedules on or off. |
| `automation_upsert` | `manage` (+ `dispatch`) | (none) | no | Create an automation, or change one by id: name, prompt, project, harness, model, run mode, workspace, time schedules and whether it is on. |
| `automation_delete` | `destructive` | destructive, idempotent | yes | Delete an automation and its run history, after the person confirms in MonoCode. |
| `mono_list` | `read` | read-only, idempotent | no | List the Monos (the app's standing assistants): id, name, the projects each works in and whether its chat is busy. |
| `mono_open` | `navigate` | idempotent | no | Open a Mono's tab in the window and show its chat. |
| `mono_send` | `dispatch` | (none) | no | Send a message to a Mono's chat, as typing it in that chat does. |
| `app_launch` | (relay) | idempotent | no | Start MonoCode when it is not running, and wait up to 30 seconds until it can be controlled. |
<!-- tools:end -->

The table is checked against the catalog by
`crates/monocode-mcp/tests/docs.rs`.

## Resources and subscriptions

| URI | Reads | Fresh for |
| --- | --- | --- |
| `monocode://app` | `app_info` | 60 s |
| `monocode://workspace` | `workspace_read` | 1 s |
| `monocode://projects` | `project_list` (first 200) | 5 s |
| `monocode://projects/{project}/sessions` | `session_list` for a percent-encoded project path (first 200) | 2 s |
| `monocode://sessions/{id}` | `session_status` | 1 s |
| `monocode://sessions/{id}/transcript` | `session_read` (last 3 exchanges) | 1 s |

Resources need the `read` scope. Every resource except `monocode://app` can
be followed: protocol 2025-11-25 uses `resources/subscribe`, protocol
2026-07-28 uses `subscriptions/listen`. Both send
`notifications/resources/updated` when the resource changed, without the new
content, so the client reads it again. Over HTTP a subscription lasts as long
as its session (2025-11-25) or its `subscriptions/listen` request
(2026-07-28). When the app restarts, the stdio relay
reconnects with backoff (250 ms to 5 s) and reports every subscribed resource
as updated. When the app closes the connection because the client may no
longer follow (revoked, `read` taken away, control off) or refuses the
relay's new hello (`user_declined`, `scope_denied`, `control_disabled`), the
relay stops trying until the next tool call, which connects and follows the
same resources again. The tool and resource lists are fixed for a relay
build, so there are no `list_changed` notifications.

## Errors

A failed tool call is a result with `isError: true` and
`{code, message, retryable}`; a failed resource read is a protocol error with
the same data.

| Code | Meaning | Retryable |
| --- | --- | --- |
| `app_not_running` | No app is listening for this data folder (stdio relay) | yes |
| `control_disabled` | The MCP server is off in Settings (stdio relay) | no |
| `scope_denied` | The client does not hold the tool's scope, or is no longer approved | no |
| `user_declined` | The person refused an approval or confirmation, or did not answer within 2 minutes | no |
| `not_found` | The session, project, automation, Mono or message does not exist | no |
| `project_not_open` | The path is not a project in MonoCode | no |
| `session_busy` | The session or Mono is working or has queued messages | yes |
| `session_not_idle` | The session waits on the person, is changing settings, or already holds a draft | yes |
| `provider_unavailable` | The provider is not installed or not signed in | no |
| `model_unavailable` | The provider does not offer the model | no |
| `remote_unsupported` | Remote projects cannot be changed from a client | no |
| `invalid_input` | Malformed or out-of-range input, an automation with full access, or a git or service refusal | no |
| `request_conflict` | The `requestId` was used with other input, is still running, another confirmation is open, or the files, branch or commit changed before the person confirmed | no (yes while running or asking, and after a change) |
| `timeout` | The app did not answer in time | yes |
| `internal` | Anything else | no |

When the connection closes while a call that changes something runs, the
call fails with `internal`, not retryable: it may already have acted. Check
the state (for example with `session_list` or `session_status`) before
calling it again, or pass a `requestId` so a retry is safe. A read, or a call
with a `requestId`, is retried once on a fresh connection.

## Limits

| Limit | Value |
| --- | --- |
| HTTP request body | 4 MiB |
| HTTP 2025-11-25 session without requests | ends after 5 minutes |
| Control socket connections per app | 16 |
| Calls running per connection | 8 (more are refused, retryably) |
| Request frame | 1 MiB |
| Response frame | 4 MiB |
| Topics followed per connection | 64 |
| Prompt, Mono message, automation prompt | 240,000 bytes |
| `session_wait` | 30 s by default, 60 s at most |
| App answer deadline | 90 s; 215 s for a confirmed call |
| Relay answer deadline | 100 s; 230 s for a confirmed call |
| Approval and confirmation prompts | 2 minutes |
| A denied client is not asked again for | 10 minutes |
| Kept `requestId` results per client | 200 (the oldest go first) |
| Activity log | 200 entries |
| `app_launch` wait | 30 s |
| Changed files in a git result | 100 |
| Git error text | 2,000 characters |
| Automation prompt in `automation_read` | 4,000 characters |
| Automation schedules | 20 |
| Paths per git call | 200 |
| Commit message | 20,000 characters |

## Platform notes

| Platform | Endpoint |
| --- | --- |
| Every platform | MCP over HTTP on TCP `127.0.0.1:<port>` while the MCP server is on |
| macOS, Linux | Control socket `<data folder>/control/monocode-control.sock`; folder `0700`, socket `0600` |
| Windows | Named pipe `monocode-control-<16 hex digits of the folder's SHA-256>` |

A socket path longer than 103 bytes on macOS (107 on Linux) cannot be bound,
and every call returns that error; choose a shorter data folder. The Windows
named pipe is covered by CI only and has not been checked against a real
Windows install.

Known Windows gaps, for a follow-up on a machine that builds for Windows:

| Gap | Effect | Follow-up |
| --- | --- | --- |
| A connection that never says hello is not closed | Named pipes have no read timeout and `Connection::shutdown` does nothing there, so 16 such connections take every slot and lock all clients out until they hang up (on macOS and Linux the hello deadline of 10 s closes them) | Close a silent pipe from a watchdog with `CancelIoEx`, which needs one reviewed `unsafe` module in `transport/windows.rs` (`monocode-control` forbids `unsafe` today) |
| The relay does not check who owns the pipe | Another user who creates the pipe name before the app could answer the relay; the pipe's owner-only access list keeps others out only once the app holds the name | Check the server process's token owner (`GetNamedPipeServerProcessId`, `OpenProcessToken`) in that same module, or have the app write a token to the data folder that the welcome must carry |

## Limitations

Not built, by design or for now:

| Feature | Why |
| --- | --- |
| Starting an orchestration | It is only reachable from a plan card an agent produced in the chat |
| App screenshots | The app has no window capture path |
| Commit-message generation | It spends model tokens on the person's account; `git_commit` takes the message |
| Answering tool-permission approvals | Approvals stay with the person at the app; questions can be answered |
| Remote projects | Changes, and reads that run in the project folder, fail with `remote_unsupported` |
| Mono memory, soul, habits and artifacts | They stay inside the Mono; Monos can be listed, opened and messaged |
| Automation event triggers | Set them in the app; `automation_upsert` edits time schedules and keeps event triggers |

Never exposed, whatever the scopes:

| Area | Why |
| --- | --- |
| Accounts and credentials | secrets |
| Provider CLI paths | equal to code execution |
| Claude hooks, the default access mode, skill source paths | widen what agents run |
| MCP server configuration | adds commands agents will run |
| Mobile pairing and router port mapping | network exposure |
| Remote connections and SSH | network access and credentials |
| App and harness updates | installers |
| Inbox connections, pull request merge and close | tokens, irreversible remote actions |
| Automations with full access | unattended code execution |
| Terminal input | arbitrary execution |
| The MCP server switch and its port | self-escalation |

Return to the [native README](README.md).

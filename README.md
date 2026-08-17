# Ariadne

A docker-style orchestrator for AI coding agents. A daemon (`ariadned`) breaks
goals into tasks with a **planner** agent, hands each task to an **engineer**
agent that owns it until merge, and gates merges behind one or more
**reviewer** agents — all running autonomously in tmux sessions you can attach
to at any time. Supports **Claude Code**, **OpenAI Codex CLI** and
**OpenCode**.

```
┌─────────┐   REST (unix socket / TCP)   ┌──────────────────────────────┐
│ ariadne │ ───────────────────────────► │           ariadned           │
│  (CLI)  │                              │  scheduler · tmux · git · db │
└─────────┘                              └──────┬───────────────────────┘
     ▲                                          │ spawns (tmux, worktree per task)
     │ MCP (stdio)                              ▼
     │        ┌─────────┐   ┌──────────┐   ┌──────────┐
     └─────── │ planner │   │ engineer │   │ reviewer │  · hooks report events
              └─────────┘   └──────────┘   └──────────┘  · tools via `ariadne mcp serve`
```

## How it works

1. `ariadne goal create` — you describe a goal, name the project repo(s), how
   many reviewer approvals a task needs (default 1) and optionally a max task
   count. The daemon spawns the **planner** in tmux; `ariadne goal attach`
   drops you into the conversation.
2. The planner discusses the breakdown with you, creates tasks through the
   Ariadne MCP tools (assigning an engineer profile and reviewer profiles per
   task, with optional `depends_on` ordering), then calls `finalize_plan`.
3. The scheduler takes over: when a task's dependencies are merged it becomes
   `ready`, an **engineer** is spawned in a dedicated git worktree on branch
   `ariadne/task-<id>`, implements, commits and calls `request_review`.
4. **Reviewers** spawn in read-only detached worktrees, inspect the diff and
   `approve` or `request_changes`. Change requests resume the engineer with
   the feedback; enough approvals trigger the merge instruction.
5. The engineer merges into the base branch and calls `mark_merged` — which
   the daemon only accepts after verifying the merge with
   `git merge-base --is-ancestor`. Worktrees are cleaned up, dependent tasks
   wake up, and the goal completes when everything is merged.

Task lifecycle: `pending → ready → in_progress → under_review →
(changes_requested → in_progress …) → approved → merging → merged`, with
`cancelled`/`failed` (retryable) escapes. Every transition is validated
against a typed state machine and recorded in an audit table.

Agents run with permissions bypassed (`--dangerously-skip-permissions`,
`--yolo`, `permission: allow`) — hooks installed at spawn time report every
session/tool event back to the daemon, and each agent's internal session id
is tracked so sessions can be resumed and attached.

## Install

```sh
scripts/install.sh             # builds, installs to ~/.local/bin, registers the
                               # daemon service (launchd / systemd --user) and
                               # bash+zsh completions. Idempotent — re-run to upgrade.
scripts/install.sh --prefix /usr/local/bin   # custom location
scripts/uninstall.sh           # removes everything, keeps ~/.ariadne data
scripts/uninstall.sh --purge   # ...and deletes the data too
```

Codex needs one manual step, which the installer runs last: it opens a codex
session so you can accept its "Hooks need review" prompt. Ariadne's codex hooks
travel with every session as `-c` overrides — nothing is written to `~/.codex` —
but codex only runs hooks it has been trusted with, and trust is a decision only
you can make. It is asked once: codex keys command-line hook trust on a
synthetic path, so the approval covers every later session in every worktree.
Without it, codex sessions run but never report their id and can be neither
resumed nor revived. Re-run it any time with `ariadne setup codex-hooks`, or
skip it during install with `--no-codex-hooks`.

The daemon then runs as a user service with restart-on-failure
(`launchctl bootout gui/$(id -u)/dev.ariadne.daemon` /
`systemctl --user stop ariadned` to stop it). Completions are **dynamic**: on
TAB the shell asks the daemon, so task/goal/session ids and profile names
complete with live values (annotated with status and title). Other shells:
`source <(COMPLETE=fish ariadne)` for dynamic, or `ariadne completions
<shell>` for a static script.

## Quick start

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# Built-in profiles Planner / Engineer / Reviewer are seeded automatically
# with no agent kind or model: at spawn time the first installed CLI is used,
# in order claude_code -> codex -> opencode. So this already works:
ariadne goal create --title "Add rate limiting" --repo ~/projects/api
ariadne goal attach <goal-id>

# --repo takes the base branch after '@' (or after ':' when it has no '/'):
ariadne goal create --title "..." --repo ~/projects/api@release/2.0

# Custom profiles pin a role to a specific agent/model/prompt:
ariadne profile create --name rev-strict --role reviewer --agent codex \
  --prompt "You are a demanding reviewer. Reject anything without tests."

# Every prompt a profile briefs its agents with is editable, and every one of
# them can go back to the default of its role:
ariadne profile prompts rev-strict                       # system + its briefings
ariadne profile prompt get rev-strict reviewer_briefing > brief.md
ariadne profile prompt set rev-strict reviewer_briefing --file brief.md
ariadne profile prompt reset rev-strict --all
ariadne goal create --title "..." --repo ~/projects/api \
  --planner Planner --approvals 2

# or write the plan yourself, instead of leaving it to the planner:
ariadne task create <goal-id> --title "Rate-limit middleware" \
  --engineer Engineer --reviewer Reviewer
ariadne task create <goal-id> --title "Wire it into the router" \
  --depends-on <first-task-id>
ariadne task update <task-id> --title "..." --reviewer rev-strict
ariadne goal finalize <goal-id>        # planning ends, the tasks start running

# watch it run
ariadne task ls --goal <goal-id>
ariadne task attach <task-id>          # engineer terminal (or --role reviewer)
ariadne task diff <task-id>            # current branch diff
ariadne task msg <task-id> "hold on, use the middleware crate instead"
ariadne session ls                     # every agent session + internal ids
ariadne attach <id>                    # session, task or goal id
```

## Commands

| Command | Purpose |
|---|---|
| `ariadne daemon start\|stop\|status\|logs` | manage `ariadned` |
| `ariadne profile create\|ls\|inspect\|update\|rm` | agent profiles |
| `ariadne profile prompts` / `profile prompt get\|set\|reset` | the prompts a profile briefs its agents with |
| `ariadne goal create\|ls\|inspect\|attach\|messages\|finalize\|cancel` | goals |
| `ariadne task create\|update\|ls\|inspect\|diff\|attach\|logs\|messages\|msg\|reviews\|history\|cancel\|retry` | tasks |
| `ariadne session ls\|inspect\|logs\|kill` | agent sessions |
| `ariadne attach <id>` | attach to a session, task or goal id |

Every command that prints data takes `--format json` (the ones that hand the
terminal to another program — `attach`, `daemon logs`, `completions`, `setup` —
do not); the daemon exposes its full API as OpenAPI at
`/api-docs/openapi.json` with Swagger UI at `/docs`.

Table output is for eyes and JSON is for scripts: tables cut long cells to the
column width with `…` (`--no-trunc` prints them whole) and show timestamps in
local time, while `--format json` is the daemon's own payload, RFC3339 and all.
Notes like "no tasks yet" go to stderr, so stdout stays pipeable. Irreversible
commands — `goal cancel`, `session kill`, `profile prompt reset` — ask first
when stdin is a terminal;
`-y` answers for you, and a script is never prompted.

A command that fails prints one line on stderr — `error: <what went wrong>`,
with a hint in parentheses when there is an obvious next step — and exits 1;
usage errors exit 2. Under `--format json` the failure comes back as the same
`{"error": {"code", "message"}}` envelope the daemon uses, so the status and
code the human line leaves out stay available to scripts.

`GET /v1/events/stream` is a server-sent-event stream of everything the daemon
changes — goals, tasks (including scheduler-driven transitions), messages,
reviews, sessions and profiles — each event carrying the full updated DTO, and
filterable with `?goal=` / `?task=`. There is no replay: on (re)connect,
refetch the REST state you care about and then follow the stream. A client
that falls behind is never left silently stale — it gets a final `resync`
event and the connection closes, which an `EventSource` turns into a
reconnect. CORS is wide open on the TCP listener so webview and browser
clients can use it.

`GET /v1/sessions/{id}/logs/stream` does the same for one agent's terminal: a
`snapshot` event with the current scrollback, a `delta` event per burst of new
output (tailed from the console log tmux pipes for every session), and a final
`end` event when the session is over, after which the stream closes. Chunks
travel as `{"chunk": "..."}`, so raw ANSI cannot break SSE framing — feed them
straight into a terminal emulator.

## Configuration

`~/.ariadne/config.toml` (all optional):

```toml
socket_path = "/Users/me/.ariadne/ariadne.sock"
db_path = "/Users/me/.ariadne/ariadne.db"
worktree_root = "/Users/me/.ariadne/worktrees"
tcp_listen = "127.0.0.1:7676"     # enables the TCP listener (for web/desktop UIs)
log_filter = "info,ariadne_daemon=debug"
cli_bin = "/usr/local/bin/ariadne" # hook/MCP entry point (default: sibling of ariadned)
delete_merged_worktrees = false    # keep task worktrees after merge for inspection (default)
delete_merged_branches = true      # only applies when worktrees are deleted too:
                                   # a kept engineer worktree pins the task branch
```

`ARIADNE_HOME` moves the whole home directory: daemon and CLI alike resolve
the socket from it (`--home` > `ARIADNE_HOME` > `~/.ariadne`, then that home's
`socket_path` > `<home>/ariadne.sock`), so every command addresses the daemon
of the home it runs in. `ARIADNE_SOCKET` (or `--endpoint`, whose old spelling
`--host` still works) overrides that with a socket path or `http://host:port`.

## Workspace layout

```
crates/
  ariadne-core     domain types + task state machine (pure, exhaustively tested)
  ariadne-api      REST DTOs / error shape (single source of truth for OpenAPI)
  ariadne-store    SQLite persistence (sqlx, embedded migrations)
  ariadne-client   REST client (unix socket / TCP), used by CLI + MCP
  ariadne-daemon   ariadned: axum API, scheduler, tmux/git managers, agent adapters
  ariadne-cli      ariadne: CLI, MCP server (`mcp serve`), hook sink (`agent-event`)
assets/opencode-plugin/  event-forwarding plugin installed for OpenCode
ui/              desktop UI (Tauri 2 + React): a REST/SSE client of the daemon's
                 TCP listener, outside the cargo workspace — see ui/README.md
```

## Development

```sh
cargo test                                  # unit + store integration tests
cargo test -p ariadne-daemon -- --ignored   # tmux/git integration tests
cargo clippy --all-targets
```

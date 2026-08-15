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

The daemon then runs as a user service with restart-on-failure
(`launchctl bootout gui/$(id -u)/dev.ariadne.daemon` /
`systemctl --user stop ariadned` to stop it). Completions for other shells:
`ariadne completions <shell>` (bash, zsh, fish, elvish, powershell).

## Quick start

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# Built-in profiles Planner / Engineer / Reviewer are seeded automatically
# with no agent kind or model: at spawn time the first installed CLI is used,
# in order claude_code -> codex -> opencode. So this already works:
ariadne goal create --title "Add rate limiting" --repo ~/projects/api
ariadne goal attach <goal-id>

# Custom profiles pin a role to a specific agent/model/prompt:
ariadne profile create --name rev-strict --role reviewer --agent codex \
  --prompt "You are a demanding reviewer. Reject anything without tests."
ariadne goal create --title "..." --repo ~/projects/api \
  --planner Planner --approvals 2

# watch it run
ariadne task ls --goal <goal-id>
ariadne task attach <task-id>          # engineer terminal (or --role reviewer)
ariadne task diff <task-id>            # current branch diff
ariadne task msg <task-id> "hold on, use the middleware crate instead"
ariadne session ls                     # every agent session + internal ids
```

## Commands

| Command | Purpose |
|---|---|
| `ariadne daemon start\|stop\|status\|logs` | manage `ariadned` |
| `ariadne profile create\|ls\|inspect\|update\|rm` | agent profiles |
| `ariadne goal create\|ls\|inspect\|attach\|messages\|cancel` | goals |
| `ariadne task ls\|inspect\|diff\|attach\|logs\|messages\|msg\|reviews\|history\|cancel\|retry` | tasks |
| `ariadne session ls\|inspect\|logs\|kill` | agent sessions |
| `ariadne attach <id>` | attach to a task's or goal's agent |

Every command takes `--format json`; the daemon exposes its full API as
OpenAPI at `/api-docs/openapi.json` with Swagger UI at `/docs`.

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

`ARIADNE_HOME` moves the whole home directory; `ARIADNE_SOCKET` points the
CLI at a socket path or `http://host:port`.

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
```

## Development

```sh
cargo test                                  # unit + store integration tests
cargo test -p ariadne-daemon -- --ignored   # tmux/git integration tests
cargo clippy --all-targets
```

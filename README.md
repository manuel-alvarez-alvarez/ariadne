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

1. `ariadne goal create` — you describe a goal, pick the registered
   repositories it works in (`ariadne repo add`), how many reviewer approvals
   a task needs (default 1) and optionally a max task count. The daemon spawns
   the **planner** in tmux; `ariadne goal attach` drops you into the
   conversation.
2. The planner discusses the breakdown with you, creates tasks through the
   Ariadne MCP tools (assigning an engineer profile and reviewer profiles per
   task, with optional `depends_on` ordering), then calls `finalize_plan`.
3. The scheduler takes over: when a task's dependencies are merged it becomes
   `ready` and an **engineer** is spawned in a dedicated git worktree, on a
   branch named after the task — its title slugged plus a short tail of its id,
   as in `fix-the-landing-briefing-real-fetch-r9jr7c`. It implements, commits
   and calls `request_review`.
4. **Reviewers** spawn in read-only detached worktrees, inspect the diff and
   `submit_verdict`, approving or requesting changes. Change requests resume
   the engineer with the feedback; enough approvals move the task to
   `approved`.
5. The task never leaves the engineer that wrote it: it keeps its session and
   its worktree, and is briefed to land the change the way the repository's
   **merge strategy** says (`ariadne repo add --merge-strategy`, default
   `direct`).
   - **`direct`** — rebase onto the base, squash into one commit with a
     conventional subject, fast-forward the base branch in the primary
     checkout, push it where there is a remote, then `mark_merged`. The daemon
     only accepts the sha after verifying the merge with
     `git merge-base --is-ancestor`.
   - **`pull_request`** — rebase once, push the branch, and open a request with
     `gh pr create` or `glab mr create` (whichever the `origin` remote calls
     for), following the repository's own templates; `record_pull_request`
     tells you where it is. The engineer then waits on it in its own session,
     polling the forge and sleeping between polls: it answers every comment,
     and a change somebody asks for is made on the same branch and sent through
     the Ariadne reviewers before it is pushed — a published branch is merged
     into and added to, never rewritten. Once the request is approved and green
     it merges it with `--squash`, fast-forwards the base branch and reports
     the sha.
6. Worktrees are cleaned up, dependent tasks wake up, and the goal completes
   when everything is merged.

Task lifecycle: `pending → ready → in_progress → under_review →
(changes_requested → in_progress …) → approved → merged`, with
`cancelled`/`failed` (retryable) escapes. From `approved` the engineer can also
`request_review` again, which is how a revision of a published request is
reviewed. Every transition is validated against a typed state machine and
recorded in an audit table.

Agents run with permissions bypassed — `--dangerously-skip-permissions` for
Claude Code, `--dangerously-bypass-approvals-and-sandbox` for Codex, `--auto`
plus an allow-everything permission block for OpenCode (`ariadne agent list`
prints the current flags). Hooks installed at spawn time report every
session/tool event back to the daemon, and each agent's internal session id is
tracked so sessions can be resumed and attached.

## Install

```sh
scripts/install.sh             # the latest release into ~/.local/bin, the daemon
                               # service (launchd / systemd --user), bash+zsh
                               # completions and the Ariadne Desktop app.
                               # Idempotent — re-run to upgrade.
scripts/install.sh --version v0.2.0          # a specific release
scripts/install.sh --build-from-source       # compile this checkout instead
scripts/install.sh --prefix /usr/local/bin   # custom location
scripts/install.sh --no-ui     # CLI and daemon only, no desktop app
scripts/uninstall.sh           # removes everything, keeps ~/.ariadne data
scripts/uninstall.sh --purge   # ...and deletes the data too
```

`scripts/install.sh --help` lists every flag; below is what the flags do not
say.

By default the binaries and the desktop app come from the GitHub release named
by `--version` (the latest one when it is not given), for the target triple
this machine runs — `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`; anything else has
to be built. The assets are unsigned, but they carry a build provenance
attestation, and **every downloaded file is checked with `gh attestation
verify` before anything is installed** — so the [GitHub
CLI](https://cli.github.com) has to be installed and logged in
(`gh auth login`), and a failed check aborts the install with nothing touched.
The release and its attestations are read from the `origin` remote of the
checkout you run the script from. Since nothing is signed, macOS would
quarantine what was downloaded, so the installer clears
`com.apple.quarantine` from what it installs. `--build-from-source` is the
other way in: it compiles this checkout with
`cargo build --release` and builds the desktop app with `npm run tauri build`,
and needs neither `gh` nor a published release.

The desktop app lands in `/Applications/Ariadne Desktop.app` on macOS
(`~/Applications` when `/Applications` is not writable) and as
`$PREFIX/ariadne-desktop` on Linux. Built rather than downloaded it needs
`npm`, and without it that one step is skipped instead of failing the install.
Where everything went is recorded in `~/.ariadne/install.env`, which is what
the uninstaller reads.

Codex needs one manual step, which the installer runs last: it opens a codex
session so you can accept its "Hooks need review" prompt. Ariadne's codex hooks
travel with every session as `-c` overrides — nothing is written to `~/.codex` —
but codex only runs hooks it has been trusted with, and trust is a decision only
you can make. It is asked once: codex keys command-line hook trust on a
synthetic path, so the approval covers every later session in every worktree.
Without it, codex stops each session on that prompt before its first turn —
bypass flags and all — so nothing runs and the session can be neither resumed
nor revived. Re-run it any time with `ariadne setup codex-hooks`, or skip it
during install with `--no-codex-hooks` (or `--yes`).

Codex grants that trust per event, so an Ariadne that declares a new hook event
keeps the verdicts you already gave and takes every session down to the prompt
over the one that is new — quietly, since the prompt is at the start of a
session nobody is watching. **After upgrading, re-run `ariadne setup
codex-hooks`.** `ariadne doctor` reads the verdicts back out of codex's config
and names any declared event that has none. Which events are declared, and why
each one, is in `crates/ariadne-core/src/codex_hooks.rs`.

The daemon then runs as a user service with restart-on-failure (stop it with
`launchctl bootout gui/$(id -u)/dev.ariadne.daemon` or `systemctl --user stop
ariadned`). Completions are **dynamic**: on TAB the shell asks the daemon, so
task/goal/session ids and profile names complete with live values. Other
shells: `source <(COMPLETE=fish ariadne)`, or `ariadne completions <shell>`
for a static script.

## Quick start

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# Planner / Engineer / Reviewer profiles are seeded automatically with no agent
# kind or model: at spawn time the first installed CLI is used, in order
# claude_code -> codex -> opencode. A repository is registered once and
# referenced by every goal that works in it (--branch defaults to the
# checked-out branch), so this already works:
ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api
ariadne goal attach <goal-id>

# watch it run
ariadne attention                      # what is waiting for you, across every goal
ariadne task ls --goal <goal-id>
ariadne task attach <task-id>          # engineer terminal (or --role reviewer)
ariadne task msg <task-id> "hold on, use the middleware crate instead"
ariadne attach <id>                    # session, task or goal id
```

`ariadne --help` lists every command and `ariadne <command> --help` every flag:
that is the CLI reference, and it is the one this binary actually implements.
`ariadne doctor` is what to run when something is not working — it reports what
your shell sees *and* what the daemon sees, because a daemon started by launchd
or systemd carries the PATH its service file was written with. Every command
that prints data takes `--format json` (the ones that hand the terminal to
another program — `attach`, `daemon logs`, `completions`, `setup` — do not).
The daemon serves its full API as OpenAPI at `/api-docs/openapi.json`, Swagger
UI at `/docs`, and a live event stream at `/v1/events/stream`.

## Configuration

`~/.ariadne/config.toml` (all optional):

```toml
socket_path = "/Users/me/.ariadne/ariadne.sock"
db_path = "/Users/me/.ariadne/ariadne.db"
worktree_root = "/Users/me/.ariadne/worktrees"
tcp_listen = "127.0.0.1:7676"     # enables the TCP listener (for web/desktop UIs)
log_filter = "info,ariadne_daemon=debug"
cli_bin = "/usr/local/bin/ariadne" # what starts every agent session (`ariadne _spawn`),
                                   # and their hook and MCP entry point
                                   # (default: sibling of ariadned)
delete_merged_worktrees = true     # remove task worktrees after merge (default);
                                   # false keeps them for inspecting merged work
delete_merged_branches = true      # only applies when worktrees are deleted too:
                                   # a kept engineer worktree pins the task branch
prevent_sleep = true               # hold a system sleep inhibition while any agent
                                   # session is live, so the box does not idle-sleep
                                   # out from under a working agent (default)
```

`db_path` has to be deleted before this version is started for the first time:
the schema's 29 migrations are squashed into one, so a database written by an
earlier release records migrations this one no longer ships and cannot be
opened. There is no upgrade from it — Ariadne is pre-1.0, and a database is
recreated rather than migrated. Delete it (with its `-wal` and `-shm` files)
and the daemon writes a fresh one on its next start; `ariadned` and `ariadne
doctor` both say so, by name, if it is still there.

The file is read strictly — an unknown key stops the daemon rather than being
ignored — so a `config.toml` naming `running_quiet_flag_secs` or
`running_quiet_resume_secs` has to drop them: the one watchdog that reads how
long a session has reported nothing keeps its own timeline (a nudge at five
minutes, the flag at fifteen, a relaunch at forty-five) and takes neither key.

`ARIADNE_HOME` moves the whole home directory: daemon and CLI alike resolve
the socket from it (`--home` > `ARIADNE_HOME` > `~/.ariadne`, then that home's
`socket_path` > `<home>/ariadne.sock`), so every command addresses the daemon
of the home it runs in. `ARIADNE_SOCKET` (or `--endpoint`, whose old spelling
`--host` still works) overrides that with a socket path or `http://host:port`.

## Workspace layout

```
crates/
  ariadne-core     domain types, task state machine, the shared binary/path probe
  ariadne-api      REST DTOs / error shape (single source of truth for OpenAPI)
  ariadne-store    SQLite persistence (sqlx, one embedded init migration)
  ariadne-client   REST client (unix socket / TCP), used by CLI + MCP
  ariadne-daemon   ariadned: axum API, scheduler, tmux/git managers, agent adapters
  ariadne-cli      ariadne: CLI, MCP server (`mcp serve`), hook sink (`agent-event`)
assets/opencode-plugin/  event-forwarding plugin installed for OpenCode
scripts/         install.sh / uninstall.sh + lib.sh, their shared step output
ui/              Ariadne Desktop (Tauri 2 + React): a REST/SSE client of the daemon's
                 TCP listener, outside the cargo workspace — see ui/README.md
```

## Development

```sh
cargo test                                  # unit + store integration tests
cargo clippy --all-targets
cargo build -p ariadne-cli                  # the tmux/git integration tests spawn
cargo test -p ariadne-daemon -- --ignored   # target/debug/ariadne, so build it first
```

[`AGENTS.md`](AGENTS.md) holds the conventions for changing this repository,
the commit types included — they are written down there and nowhere else. The
release loop is in [`.github/RELEASING.md`](.github/RELEASING.md); the desktop
app has its own [`ui/README.md`](ui/README.md).

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
   `ready`, an **engineer** is spawned in a dedicated git worktree on a branch
   named after the task — its title slugged, plus a short tail of its id, as in
   `fix-the-landing-briefing-real-fetch-r9jr7c` — implements, commits and
   calls `request_review`.
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
   - **`pull_request`** — rebase once, push the branch, and open a request
     with `gh pr create` or `glab mr create` (whichever the `origin` remote
     calls for), following the repository's own templates and conventions;
     `record_pull_request` tells you where it is. The engineer then waits on it
     in its own session, polling the forge and sleeping between polls: it
     answers every comment on the request, and a change somebody asks for is
     made on the same branch and sent through the Ariadne reviewers before it
     is pushed — a published branch is merged into and added to, never
     rewritten. Once the request is approved and green it merges it with
     `--squash`, fast-forwards the base branch and reports the sha.
6. Worktrees are cleaned up, dependent tasks wake up, and the goal completes
   when everything is merged.

Task lifecycle: `pending → ready → in_progress → under_review →
(changes_requested → in_progress …) → approved → merged`, with
`cancelled`/`failed` (retryable) escapes. From `approved` the engineer can also
`request_review` again, which is how a revision of a published request is
reviewed. Every transition is validated against a typed state machine and
recorded in an audit table.

Agents run with permissions bypassed (`--dangerously-skip-permissions`,
`--yolo`, `permission: allow`) — hooks installed at spawn time report every
session/tool event back to the daemon, and each agent's internal session id
is tracked so sessions can be resumed and attached.

## Install

```sh
scripts/install.sh             # downloads the latest release, installs to
                               # ~/.local/bin, registers the daemon service
                               # (launchd / systemd --user), bash+zsh
                               # completions and the Ariadne Desktop app.
                               # Idempotent — re-run to upgrade.
scripts/install.sh --version v0.2.0          # a specific release
scripts/install.sh --build-from-source       # compile this checkout instead
scripts/install.sh --prefix /usr/local/bin   # custom location
scripts/install.sh --no-ui     # CLI and daemon only, no desktop app
scripts/uninstall.sh           # removes everything, keeps ~/.ariadne data
scripts/uninstall.sh --purge   # ...and deletes the data too
```

By default the binaries and the desktop app come from the GitHub release named
by `--version` (the latest one when it is not given), for the target triple
this machine runs — `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`; anything else has
to be built. The assets are unsigned, but they carry a build provenance
attestation, and **every downloaded file is checked with `gh attestation
verify` before anything is installed** — so the [GitHub
CLI](https://cli.github.com) has to be installed and logged in
(`gh auth login`), and a failed check aborts the install with nothing touched.
The repository the release and its attestations are read from is the `origin`
remote of the checkout you run the script from. Since nothing is signed,
macOS would quarantine what was downloaded, so the installer clears
`com.apple.quarantine` from the binaries and the app it installs.

`--build-from-source` is the other way in: it compiles this checkout with
`cargo build --release` and builds the desktop app with `npm run tauri build`,
exactly as the installer always did, and needs neither `gh` nor a published
release.

Both scripts run as a numbered list of steps, each marked ✓ done, ↷ skipped or
✗ failed, and both end with a summary of what went where. The noise (cargo,
npm, launchctl, systemctl) is captured to `~/.ariadne/install.log` — or
`$TMPDIR/ariadne-uninstall.log`, which `--purge` cannot delete — and printed
only when a step fails. Colors and symbols are used only on a terminal, and
never when `NO_COLOR` is set.

| flag | |
| --- | --- |
| `--build-from-source`, `--build` | compile this checkout instead of downloading a release (install) |
| `--version vX.Y.Z` | the release to install, default the latest one (install, download mode only) |
| `--prefix DIR` | install binaries into `DIR` (default `~/.local/bin`); on the uninstaller, where to look when there is no manifest |
| `--no-service` | skip the daemon service registration (install) |
| `--no-completions` | skip the shell completions (install) |
| `--no-codex-hooks` | skip the Codex hook trust step (install) |
| `--no-ui` | skip installing the Ariadne Desktop app (install) |
| `--purge` | also delete `~/.ariadne` (uninstall) |
| `--verbose` | stream the subcommand output instead of capturing it |
| `--quiet` | errors and the final summary only |
| `--dry-run` | print the steps that would run and change nothing |
| `--yes`, `-y` | non-interactive: skip the Codex prompt (install; the uninstaller never asks) |
| `--help`, `-h` | the same list, from the script |

The desktop app (`ui/`, a Tauri shell around the web UI) is installed as one of
those steps: downloaded, it is one more verified asset; built, it is
best-effort, since it needs `npm` — the Tauri CLI itself comes from `ui/`'s
devDependencies — and without it the step is skipped (↷) rather than failing
the install, which is what `--no-ui` does too. It lands in
`/Applications/Ariadne Desktop.app` on macOS (`~/Applications` when
`/Applications` is not writable) and as `$PREFIX/ariadne-desktop` on Linux (the
AppImage, or the plain binary when no AppImage was produced). Wherever it went
is recorded in the manifest, and that is the exact path the uninstaller removes.

Adding a step to either script is a `plan_add` where the plan is built and a
`step_begin`/`step_ok` pair where the work happens; the shared output
framework is `scripts/lib.sh`.

Codex needs one manual step, which the installer runs last: it opens a codex
session so you can accept its "Hooks need review" prompt. Ariadne's codex hooks
travel with every session as `-c` overrides — nothing is written to `~/.codex` —
but codex only runs hooks it has been trusted with, and trust is a decision only
you can make. It is asked once: codex keys command-line hook trust on a
synthetic path, so the approval covers every later session in every worktree.
Without it, codex stops each session on that prompt before its first turn —
bypass flags and all — so nothing runs, no id is reported, and the session can
be neither resumed nor revived. Re-run it any time with `ariadne setup
codex-hooks`, or skip it during install with `--no-codex-hooks` (or `--yes`).

Codex grants that trust per event, so an Ariadne that declares a hook event
your last setup did not keeps the verdicts you already gave and takes every
session down to the prompt over the one that is new — quietly, since the
prompt is at the start of a session nobody is watching. **After upgrading,
re-run `ariadne setup codex-hooks`.** `ariadne doctor` reads the verdicts back
out of codex's config and names any declared event that has none; `ariadne
setup codex-hooks` says which are missing before it opens codex, and checks
codex's own config again afterwards rather than trusting an exit status. Which
events are declared, and why each one, is in
`crates/ariadne-core/src/codex_hooks.rs`.

The daemon then runs as a user service with restart-on-failure
(`launchctl bootout gui/$(id -u)/dev.ariadne.daemon` /
`systemctl --user stop ariadned` to stop it). The install ends with a checkup —
`ariadne doctor`, below — whose findings are reported and never fail the
install. Completions are **dynamic**: on
TAB the shell asks the daemon, so task/goal/session ids and profile names
complete with live values (annotated with status and title). Other shells:
`source <(COMPLETE=fish ariadne)` for dynamic, or `ariadne completions
<shell>` for a static script.

## Quick start

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# Built-in profiles Planner / Engineer / Reviewer are seeded
# automatically with no agent kind or model: at spawn time the first installed
# CLI is used, in order claude_code -> codex -> opencode. A repository is
# registered once and referenced by every goal that works in it (--branch
# defaults to the checked-out branch), so this already works:
ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api
ariadne goal attach <goal-id>

# The same checkout on another base branch is a repository of its own, and
# --repo then takes the id `repo ls` prints rather than the path:
ariadne repo add ~/projects/api --branch release/2.0
ariadne repo ls
ariadne goal create --title "..." --repo <repository-id>

# How a task lands here is the repository's to say: squashed straight onto the
# base branch (the default), or published for a human to merge — in which case
# the engineer opens the request with `gh`/`glab` and waits on it itself.
ariadne repo add ~/projects/api --merge-strategy pull_request
ariadne repo edit <repository-id> --merge-strategy direct

# Custom profiles pin a role to a specific agent/model/prompt:
ariadne profile create --name rev-strict --role reviewer --agent codex \
  --prompt system="You are a demanding reviewer. Reject anything without tests."

# A profile runs on the built-in prompts of its role until one is set on it, and
# a reset drops what was set rather than writing the default over it:
ariadne profile prompts rev-strict            # system + briefings, default or not
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
ariadne session ls                     # live agents + the work each one is on
ariadne session ls --status failed     # or any one status, --all not needed
ariadne attention                      # what is waiting for you, across every goal
ariadne attach <id>                    # session, task or goal id
```

## Commands

| Command | Purpose |
|---|---|
| `ariadne daemon start\|stop\|status\|logs` | manage `ariadned` |
| `ariadne doctor` | check the whole installation and say what to fix |
| `ariadne profile create\|ls\|inspect\|update\|rm` | agent profiles |
| `ariadne profile prompts` / `profile prompt get\|set\|reset` | the prompts a profile briefs its agents with |
| `ariadne agent list\|update` | the flags each coding-agent CLI is launched with |
| `ariadne goal create\|ls\|inspect\|attach\|messages\|msg\|finalize\|cancel\|rm` | goals |
| `ariadne task create\|update\|ls\|inspect\|diff\|attach\|logs\|messages\|msg\|reviews\|history\|cancel\|retry` | tasks |
| `ariadne session ls\|inspect\|logs\|kill` | agent sessions |
| `ariadne attention` | everything that needs a human, grouped by goal |
| `ariadne attach <id>` | attach to a session, task or goal id |

`ariadne attention` answers the question the day starts with: what is waiting
for me? It is the web UI's "Needs attention" strip in the terminal, and it
lists, per goal, the tasks that failed or stalled, and the agent sessions the
daemon raised a reason for. Work that is waiting on an agent rather than on
you stays off it: a task whose review asked for changes is one the daemon
resumes the engineer on, and a session that exited with nothing owed to it is
finished, not stuck. A session is listed with the reason the daemon raised for
it:

| Reason | What it means |
|---|---|
| `waiting for permission` | blocked on a permission or approval prompt |
| `waiting for input` | it asked a question and is idle until it is answered |
| `agent error` | the agent reported an error |
| `disconnected` | its terminal is gone while its work is still active |
| `stalled` | no activity for too long |

The same reason rides along with the session in `ariadne session ls` and
`ariadne session inspect`, and on its badge in the UI. Both surfaces apply the
same rules, so they always show the same list.

`ariadne doctor` is the one to run when something is not working: it reports
the CLI and daemon versions, the home and its `config.toml`, the service
registration, tmux and git, each coding-agent CLI with its version and flags,
and every profile whose agent cannot actually be launched. It reports twice
over — what your shell sees and what the daemon sees — because a daemon
started by launchd or systemd carries the PATH its service file was written
with, so an agent installed afterwards can be on your PATH and invisible to
the process that spawns sessions. It changes nothing, and exits 1 when
anything is at failure level (warnings alone exit 0).

Every command that prints data takes `--format json` (the ones that hand the
terminal to another program — `attach`, `daemon logs`, `completions`, `setup` —
do not); the daemon exposes its full API as OpenAPI at
`/api-docs/openapi.json` with Swagger UI at `/docs`.

Table output is for eyes and JSON is for scripts: tables cut long cells to the
column width with `…` (`--no-trunc` prints them whole) and show timestamps in
local time, while `--format json` is the daemon's own payload, RFC3339 and all.
Notes like "no tasks yet" go to stderr, so stdout stays pipeable. Irreversible
commands — `goal cancel`, `goal rm`, `session kill`, `profile prompt reset` — ask
first when stdin is a terminal; `-y` answers for you, and a script is never
prompted.

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
running_quiet_flag_secs = 1200     # how long an agent may be mid-turn without
                                   # reporting anything before it is raised for you
                                   # (default); what it measures is silence, not how
                                   # long the turn takes
running_quiet_resume_secs = 2700   # and how long before its pane is killed and the
                                   # same session put back on the conversation it was
                                   # having (default)
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
scripts/         install.sh / uninstall.sh + lib.sh, their shared step output
ui/              Ariadne Desktop (Tauri 2 + React): a REST/SSE client of the daemon's
                 TCP listener, outside the cargo workspace — see ui/README.md
```

## Development

```sh
cargo test                                  # unit + store integration tests
cargo test -p ariadne-daemon -- --ignored   # tmux/git integration tests
cargo clippy --all-targets
```

## Commit conventions

Commits follow [Conventional Commits](https://www.conventionalcommits.org):
`type(scope): subject`, with a lowercase type and an imperative subject
("add", not "added"/"adds"). Release notes and version bumps are generated
from these messages by release-please, so a subject is written to be read by a
user in a changelog — the full release loop is in
[`.github/RELEASING.md`](.github/RELEASING.md).

| Type | Effect on a release |
| --- | --- |
| `feat` | minor bump while pre-1.0 (0.1.0 → 0.2.0) |
| `fix`, `perf`, `revert` | patch bump (0.1.0 → 0.1.1) |
| `docs`, `refactor`, `test`, `chore`, `style`, `build`, `ci` | hidden: they ride along in a release triggered by something else, never trigger one on their own |

A `!` before the colon (`feat!: …`) or a `BREAKING CHANGE:` footer marks a
breaking change; while the project is pre-1.0 that is still a minor bump.

The scope is optional but encouraged, and it is one of the repository's area
names: `ui`, `daemon`, `cli`, `store`, `api`, `core`, `mcp`, `prompts`,
`doctor`, `codex`, `opencode`, `install`, `scripts`.

History is linear — **no merge commits, ever**. A task branch lands on its base
by squash or fast-forward, and the commit that lands carries a conventional
subject of its own.

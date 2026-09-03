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
   the **planner** in tmux; `ariadne goal attach` drops you into its terminal.
   `--model` runs that planner on something other than what its
   profile is on, and it is the whole choice: a model is spelled
   `<agent>[:<model>]` — the agent CLI that runs it (`claude_code`, `codex`,
   `opencode`) and, after a colon, one model of that CLI (`--model
   codex:gpt-5.6-sol`). The agent CLI on its own (`--model codex`) runs it on
   that CLI's own default model, and a model naming no CLI is a usage error,
   since nothing says which CLI would run it. `--effort` goes beside it and
   says how deeply that model reasons — one of the efforts `ariadne models ls`
   lists for it (`--effort xhigh`); left out, the model runs at whatever its
   agent CLI runs it at.
2. The planner first drafts a spec from the goal text, then asks its
   questions in its terminal and waits — `ariadne goal attach` drops you into
   that terminal to answer them. A planner waiting on you shows up wherever
   Ariadne lists what needs attention. It writes the spec the way the
   repository's existing specs are written; where the repository keeps none,
   the path they go in and the format they take are two more things it agrees
   with you first. It asks for an explicit yes on the finished spec before it
   creates any task.
3. Once the spec is approved, the planner lands it itself, the way the
   repository takes any other change: on a `direct` repository it commits the
   spec on the base branch of the primary checkout and pushes it; on a
   `pull_request` one it publishes a spec branch from a throwaway worktree,
   opens the request with `gh` or `glab`, answers the comments on it and
   merges it with `--squash`. That procedure is Ariadne's own text, one per
   merge strategy, and it reaches the planner in its briefing. No task is
   written until the spec is on the base branch, so every engineer branches
   off a base that already carries it and every ticket names its merged
   path.
4. Then the planner creates the tasks through the Ariadne MCP tools
   (assigning an engineer profile and reviewer profiles per task, with
   optional `depends_on` ordering, which is for real dependencies alone).
   Nothing runs while the goal is in `planning` — read the
   tasks and edit what they still need with `ariadne task update` — and it is
   `finalize_plan` that starts the work. The planner also picks what each of
   those agents runs on,
   sizing the model and the effort to the task it wrote; the last word is
   yours, with
   `ariadne task update <task-id> --model claude_code:claude-opus-5 --effort
   xhigh --reviewer Reviewer=codex:gpt-5.6-luna@high`, which a task takes while
   it is still pending or ready (`--model default` hands it back to the
   profile's, `--effort default` back to the CLI's own). A reviewer slot is
   spelled `<profile>[=<model>][@<effort>]` — `Reviewer` on its profile's own,
   `Reviewer=codex` on codex's default model, `Reviewer@high` on its profile's
   model reasoned harder — and `ariadne task create` takes the same flags. A
   pin also carries the **effort** its model is reasoned at, one of the ones
   `ariadne models ls` lists for that model, and it belongs to the model it
   runs at: a pin that names a model and no effort runs at the CLI's own
   default, and only a pin left on the profile's own model keeps the profile's
   effort. `list_models` describes what the planner sizes a task from: each
   model's tier, a cost and a speed band, what task shapes it is and is not a
   fit for, and what each of its efforts buys — `ariadne models show
   <model>` prints the same card (or, until that lands, `ariadne models ls
   --format json` carries the same fields).
5. The scheduler takes over: when a task's dependencies are merged it becomes
   `ready` and an **engineer** is spawned in a dedicated git worktree, on a
   branch named after the task — its title slugged plus a short tail of its
   id, as in `fix-the-landing-briefing-real-fetch-r9jr7c`. It implements,
   commits and calls `request_review` under a summary of what it did, which is
   what the reviewers read first.
6. **Reviewers** spawn in read-only detached worktrees, inspect the diff and
   `submit_verdict`, approving or requesting changes. Change requests resume
   the engineer with the feedback; enough approvals move the task to
   `approved`.
7. The task never leaves the engineer that wrote it: it keeps its session and
   its worktree, and is briefed to land the change with the repository's
   **landing briefing** — the whole procedure, which the engineer then runs.
   Profiles own only their system prompts. Ariadne supplies the built-in
   lifecycle prompts for planning, engineering and review sessions.
   The landing briefing is a repository field: it is prefilled from the
   repository's selected **merge strategy** (`ariadne repo add
   --merge-strategy`, default `direct`), can be replaced with custom text, and
   can be edited after registration (`--landing-prompt`,
   `--landing-prompt-file`, and `ariadne repo prompt get|set|reset`). It is put
   back on the strategy's default by clearing it (`repo update
   --reset-landing-prompt`, or `repo prompt reset`). It is what the engineers
   of that repository run: the spec its planner lands in step 3 goes by the
   merge strategy alone, whatever this text was rewritten to. What the two
   strategies prefill is:
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
8. Worktrees are cleaned up, dependent tasks wake up, and the goal completes
   when everything is merged.

Task lifecycle: `pending → ready → in_progress → under_review →
(changes_requested → in_progress …) → approved → merged`, with
`cancelled`/`failed` (retryable) escapes. From `approved` the engineer can also
`request_review` again, which is how a revision of a published request is
reviewed. A task that cannot be done as written is the engineer's own
`fail_task`, whose reason is what `ariadne task inspect` shows you. Every
transition is validated against a typed state machine and recorded in an audit
table.

Every text Ariadne hands an agent is written in ASD-STE100 **Simplified
Technical English**: the three system prompts, the briefings that start and
resume a session, and the description of every MCP tool. One instruction to a
sentence, the imperative for an instruction, the active voice, short sentences
(20 words in a procedure, 25 in a description), one meaning per word, a list
for a sequence of steps — what an agent misreads least, in the fewest tokens.
The agents write it too: the session rules every agent receives before its
first prompt, whatever its role and whatever its profile's prompts say, hold
everything it writes to the same English — its turn text and visible
reasoning, task titles and descriptions, review summaries, verdicts, failure
reasons, commit subjects and bodies, and pull request text.

Agents run with permissions bypassed — `--dangerously-skip-permissions` for
Claude Code, `--dangerously-bypass-approvals-and-sandbox` for Codex, `--auto`
plus an allow-everything permission block for OpenCode (`ariadne agent ls`
prints the current flags). Hooks installed at spawn time report every
session/tool event back to the daemon, and each agent's internal session id is
tracked so sessions can be resumed and attached.

Sessions are long-lived — one engineer per task, one reviewer per task across
its rounds, one planner per goal — and every resume replays the whole
transcript as its first prompt. So the daemon compacts each session at every
hand-off: after the planner finalizes its plan, when the engineer requests a
review, and after each verdict a reviewer gives. It types the CLI's own
`/compact` into the pane once the agent is at its prompt — with a per-role
focus for Claude Code, which takes one, saying what to keep — and leaves the
pane alone until the CLI reports the compaction done (Claude Code's
`SessionStart` from `compact`, Codex's `PostCompact` hook, OpenCode's
`session.compacted` event), or for three minutes at most. Nothing is typed into
a compacting session and nothing kills its pane: a review's feedback, a landing
briefing or a nudge that becomes due meanwhile goes out after it. Each
compaction shows in the session's events as `compaction`, and one that ended
any other way as `compaction_failed` naming why. A session is never held for
its compaction beyond that wait, and one whose CLI cannot be told to compact
from outside simply is not.

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
codex-hooks`** — the `PostCompact` hook, which tells the daemon a compaction it
asked for is over, is the latest addition. `ariadne doctor` reads the verdicts
back out of codex's config and names any declared event that has none. Which
events are declared, and why each one, is in
`crates/ariadne-core/src/codex_hooks.rs`.

The daemon then runs as a user service with restart-on-failure, and
`ariadne daemon start|stop|restart` drives that service rather than the bare
process wherever `~/.ariadne/install.env` records one — `launchctl kickstart
-k` / `launchctl bootout`, `systemctl --user` — saying which command it used;
`ariadne daemon status` says which manager is holding the daemon up.

## Shell completion

Completions are **dynamic**: what the shell sources is a few lines that call
`ariadne` back on every TAB, so the candidates are the ones the daemon has
right now — task, goal and session ids with their status and title beside
them, profile names with their role and model, and the models an agent can be
pinned to. They are verb-aware, too: `task retry` offers the failed tasks,
`session kill` the live sessions, `session resume` the ended ones, `goal rm`
the goals it will actually delete.

`scripts/install.sh` wires this up for bash and zsh. To do it yourself, or for
a shell it skipped:

```sh
ariadne completions install                 # $SHELL, or --shell bash|zsh|fish
```

or write the line by hand — it is the same one:

```sh
echo 'source <(COMPLETE=bash ariadne)' >> ~/.bashrc
echo 'source <(COMPLETE=zsh ariadne)' >> ~/.zshrc
ariadne completions fish > ~/.config/fish/completions/ariadne.fish
```

`ariadne completions <shell>` prints that registration, so `source <(ariadne
completions zsh)` works in a shell you have open now. A daemon that is down or
slow leaves TAB with nothing rather than an error, and `--model` and
`--effort` complete from a catalog cached under the ariadne home — `--model`
candidates carry the tier, cost and speed beside the description, and
`--effort` candidates carry what that effort buys. For somewhere a completion has
to be a file on disk, `ariadne completions <shell> --static` prints the old
snapshot script, which has the command tree but none of the live candidates.

## Quick start

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# Planner / Engineer / Reviewer profiles are seeded automatically with no model
# of their own: at spawn time the first installed CLI is used, in order
# claude_code -> codex -> opencode. A repository is registered once and
# referenced by every goal that works in it (--branch defaults to the
# checked-out branch), so this already works:
ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api
ariadne goal attach <goal-id>

# only a profile's system prompt is editable; lifecycle prompts come from Ariadne
ariadne profile prompt get Engineer system > system.md
ariadne profile prompt set Engineer system --file system.md
ariadne profile prompt reset Engineer system

# the repository landing briefing is prefilled from --merge-strategy, or can
# use custom text; edit it after registration with repo prompt
ariadne repo add ~/projects/ui --merge-strategy direct \
    --landing-prompt "Rebase, squash and fast-forward the base branch."
ariadne repo add ~/projects/web --merge-strategy pull-request \
    --landing-prompt-file landing.md
ariadne repo prompt get <repo-id> > landing.md   # pipe it out, edit, pipe it back
ariadne repo prompt set <repo-id> --file landing.md
ariadne repo prompt reset <repo-id>              # back to the strategy's default

# an agent CLI of your own, a model of it where you want one, and how deeply
# it reasons there
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex:gpt-5.6-sol --effort xhigh
ariadne task update <task-id> --model claude_code:claude-opus-5 --effort xhigh \
    --reviewer Reviewer=codex:gpt-5.6-luna@high
ariadne task update <task-id> --model default   # back to the profile's own
ariadne task update <task-id> --effort default  # at whatever the CLI reasons it at

# watch it run
ariadne attention                      # what is waiting for you, across every goal
ariadne task ls --goal <goal-id>       # what is going on; -a adds the finished work
ariadne task attach <task-id>          # engineer terminal (or --role reviewer)
ariadne attach <id>                    # session, task or goal id

# without attaching to a terminal
ariadne models ls --agent codex        # what --model pins, its tier/cost/speed, and the efforts each takes
ariadne models show codex:gpt-5.6-luna # one model's card: what it's for, and what each effort buys
ariadne session ls --attention         # the agents waiting on a human
ariadne session send <session-id> y    # type into a live agent, as the UI does

# lists are for reading and for piping
ariadne task ls -o wide                # every column, however narrow the terminal
ariadne task ls --columns id,title,age # or exactly the ones you name
ariadne task ls -q | xargs -n1 ariadne task inspect
ariadne task diff <task-id>            # coloured, and through $PAGER on a terminal
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

Tables are laid out for the terminal they are printed in: the least important
columns are dropped until the row fits, and `-o wide` puts them all back.
`--columns a,b,c` prints exactly the ones you name, `--no-trunc` prints the
cells whole, and `-q` prints one id per line and nothing else — the flag to
pipe a listing into whatever acts on it. `ariadne goal ls`, `task ls` and
`session ls` show what is going on rather than everything there has ever been;
`-a/--all` includes the finished work, and `--status` names the statuses
precisely. A pipe or a file gets every column, since there is no screen to fit.

Statuses are coloured and carry a glyph — `●` running, `○` pending, `✓` done
or ok, `✗` failed or cancelled, `?` waiting on you, `!` a warning worth a look
— so a table reads the same without colour. `--color auto|always|never`
decides, `NO_COLOR` is honoured, and a pipe is plain unless you ask otherwise
(`--color always`). `task diff` and the `logs` snapshots go through `$PAGER`
(`less -R`) on a terminal; `--no-pager` streams them instead.

## Following what happens

Nothing here polls: the daemon streams, and every follow mode below reads that
stream. Ctrl-C ends any of them and leaves the terminal as it found it.

```sh
ariadne events                         # what the daemon has done, one line each
ariadne events -f                      # ...and keep printing as it happens
ariadne events -f --goal <goal-id>     # one goal's; also --task, --session, --kind
ariadne events -f --format json        # one JSON object per line, for a pipe

ariadne session logs <session-id> -f   # an agent's terminal, until its session ends
ariadne task logs <task-id> -f         # the same, found by task (--role reviewer)
ariadne daemon logs -f                 # the daemon's own log, over the API

ariadne attention --watch              # redrawn whenever something needs you
ariadne task ls --watch --goal <id>    # redrawn whenever a task moves
```

`-f` prints as it goes; `--watch` redraws the whole table, `watch(1)`-style,
when an event says it has changed. `ariadne daemon logs` reads the daemon's own
ring buffer, so it works under launchd and systemd — where nothing writes
`~/.ariadne/ariadned.log` — and honours `--endpoint`; the file is the fallback
for a daemon that is not answering.

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

`ariadned --check-config` reads that file and exits: a key the daemon would
refuse is named where it stands, without starting anything or touching the
daemon that is already running. `ariadned --help` lists every key above and
the two environment variables (`ARIADNE_HOME`, `RUST_LOG`) with a line each.

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
long a session has reported nothing keeps its own timeline (a nudge at three
minutes, the flag at ten, a relaunch at thirty) and takes neither key.

`ARIADNE_HOME` moves the whole home directory: daemon and CLI alike resolve
the socket from it (`--home` > `ARIADNE_HOME` > `~/.ariadne`, then that home's
`socket_path` > `<home>/ariadne.sock`), so every command addresses the daemon
of the home it runs in. `--endpoint` (whose old spelling `--host` still works)
overrides that with a socket path or `http://host:port`, and so does
`ARIADNE_ENDPOINT` — read after the flag and before the home, with the older
`ARIADNE_SOCKET`, which every agent session is still spawned with, honoured
after it.

## Workspace layout

```
crates/          the Rust workspace: ariadned, the ariadne CLI and the libraries
                 they share — crate by crate in crates/AGENTS.md
assets/opencode-plugin/  event-forwarding plugin installed for OpenCode
scripts/         install.sh / uninstall.sh + lib.sh, their shared step output
ui/              Ariadne Desktop (Tauri 2 + React): a REST/SSE client of the daemon's
                 TCP listener, outside the cargo workspace — see ui/AGENTS.md and
                 ui/README.md
```

## Development

[`AGENTS.md`](AGENTS.md) holds the conventions for changing this repository,
the commit types included — they are written down there and nowhere else — and
points at the file each area keeps: [`crates/AGENTS.md`](crates/AGENTS.md) for
the Rust workspace and its cargo commands,
[`ui/AGENTS.md`](ui/AGENTS.md) for the desktop app. The release loop is in
[`.github/RELEASING.md`](.github/RELEASING.md); running the desktop app is
[`ui/README.md`](ui/README.md).

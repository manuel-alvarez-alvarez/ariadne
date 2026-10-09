<p align="center">
  <img src="assets/branding/banner.svg" alt="Ariadne" width="480">
</p>

<p align="center">
  <a href="https://github.com/manuel-alvarez-alvarez/ariadne/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/manuel-alvarez-alvarez/ariadne/ci.yml?branch=main&style=flat-square&label=CI&logo=githubactions&logoColor=white" alt="CI"></a>
  <a href="https://github.com/manuel-alvarez-alvarez/ariadne/releases/latest"><img src="https://img.shields.io/github/v/release/manuel-alvarez-alvarez/ariadne?style=flat-square&label=release&color=295984" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-295984?style=flat-square" alt="Apache-2.0 license"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/built%20with-Rust-295984?style=flat-square&logo=rust&logoColor=white" alt="Built with Rust"></a>
</p>

<p align="center">
  <a href="docs/README.md"><b>Manual</b></a>
  &nbsp;·&nbsp;
  <a href="#quick-start"><b>Quick start</b></a>
  &nbsp;·&nbsp;
  <a href="docs/how-it-works.md"><b>How it works</b></a>
  &nbsp;·&nbsp;
  <a href="docs/install.md"><b>Install</b></a>
  &nbsp;·&nbsp;
  <a href="ui/README.md"><b>Desktop app</b></a>
</p>

<h3 align="center">A docker-style orchestrator for AI coding agents.</h3>

<p align="center">
  You describe a goal. A daemon (<code>ariadned</code>) breaks it into tasks with an <b>orchestrator</b> agent,<br>
  and steps each task through a <b>workflow</b>: a kanban of columns, one agent per column,<br>
  that builds, reviews and finishes the change in order.
</p>

<p align="center">
  Every agent runs as an ACP session, and every task gets its own git worktree. Connect to an
  agent console at any moment to read events, send a prompt, or answer a permission question.
  Ariadne ships the ACP registry and runs whichever of its 41 agents — <b>Claude Code ACP</b>,
  <b>Codex ACP</b>, <b>OpenCode</b>, <b>goose</b> and the rest — you have installed; you can add
  any compatible agent. A goal picks each agent's model and effort.
</p>

<p align="center">
  <img src="assets/demo.gif" alt="Creating a goal and watching its tasks run from the terminal">
</p>

<p align="center">
  <sub>One goal, its tasks, and the agents running them — from the terminal.</sub>
</p>

## Features

<table>
<tr>
<td width="50%" valign="top">
<h4>🧩 ACP agents, one interface</h4>
Every agent speaks ACP. Built-in entries run Claude Code ACP, Codex ACP, and OpenCode ACP; add
another command in the registry when it meets the protocol contract. A model is spelled
<code>&lt;agent-id&gt;:&lt;model&gt;</code>, and <code>--effort</code> beside it says how deeply it
reasons.
</td>
<td width="50%" valign="top">
<h4>📝 A plan you agree to</h4>
The orchestrator asks until nothing about the goal is open, then writes the tasks. Nothing runs
until the plan is finalized, and <code>ariadne task update</code> is yours until it does.
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h4>✅ A workflow for every goal</h4>
A task steps through its workflow's columns in order — build, review, finish — one agent per
column, failing a step back for changes rather than passing broken work on.
</td>
<td width="50%" valign="top">
<h4>🚢 Two shipped workflows, and your own</h4>
<code>develop-review-merge</code> squashes and fast-forwards onto the base branch;
<code>develop-review-pr</code> opens a request through the forge and keeps it until a human
merges it. Write your own in a plain-text document, column by column.
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h4>🌱 A worktree and a branch per task</h4>
Tasks run in parallel without touching each other's checkout, and the daemon verifies every merge
before it accepts the sha.
</td>
<td width="50%" valign="top">
<h4>📚 Skills, not personas</h4>
An agent is its skills, its model and its brief — one document per kind of work, from the catalog
Ariadne ships and the ones you write.
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h4>🖥️ Connect whenever you want</h4>
Every session has a console. <code>ariadne attach</code> is an inline pane in your terminal:
markdown as it streams, a picker for permissions, Escape to cancel a turn.
<code>ariadne attention</code> says who is waiting for you.
</td>
<td width="50%" valign="top">
<h4>⚡ Nothing polls</h4>
The daemon streams: events, agent consoles and its own log, over a REST API with OpenAPI at
<code>/api-docs/openapi.json</code> and SSE at <code>/v1/events/stream</code>. A session's
console is also served as terminal bytes over a WebSocket, for a terminal emulator. A signed
forge webhook wakes the same way, and a fallback timer is the one thing that still polls.
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h4>🔄 Switch agents or models live</h4>
<code>ariadne session switch</code> moves to another agent or model mid-conversation: stop, start fresh with
what the old session knew, and the seat follows. When a model exhausts its quota, the daemon switches
automatically.
</td>
<td width="50%" valign="top">
<h4>🔀 GitHub and GitLab, hands off</h4>
Enable a registered repository's forge and Ariadne reads <code>gh</code> or <code>glab</code> for
you: the agent of a task's <code>pr</code> column keeps the request it opened with
<code>pr-babysit</code>, answering every comment and clearing every check until you merge it, a
<code>pr-reviewer</code> session posts findings by priority on the ones you are asked to review,
and none ever approves or merges in your name.
</td>
</tr>
</table>

## Ariadne Desktop

The same daemon, in a window: goals, tasks, diffs, agent consoles and the live
event stream. It is a pure REST/SSE client of the daemon's TCP listener — the
same code runs in a browser tab and in the packaged [Tauri
2](https://v2.tauri.app) app — and `scripts/install.sh` installs it beside the
CLI.

<p align="center">
  <img src="assets/demo-ui.gif" alt="Ariadne Desktop showing a goal, its tasks and an agent console">
</p>

> [!NOTE]
> The daemon's TCP listener is off by default. Set `tcp_listen` in
> `~/.ariadne/config.toml` before the app can reach it — see
> [Configuration](docs/configuration.md).

## Quick start

```sh
scripts/install.sh                     # CLI, daemon service, completions, desktop app
ariadne daemon start                   # unix socket at ~/.ariadne/ariadne.sock

ariadne models ls                      # choose an available <agent-id>:<model-id>

ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex-acp:<model-id>
ariadne goal attach <goal-id>          # answer the orchestrator's questions

ariadne attention                      # what is waiting for you, across every goal
ariadne task ls --goal <goal-id>       # what is going on
```

> [!TIP]
> Every command takes `--help`, and [`ariadne doctor`](docs/cli.md) is what to
> run when something is not working: it reports what your shell sees *and* what
> the daemon sees.

The [manual](docs/README.md) has the rest.

## Overview

```
┌─────────┐   REST (unix socket / TCP)   ┌──────────────────────────────┐
│ ariadne │ ───────────────────────────► │           ariadned           │
│  (CLI)  │                              │  scheduler · ACP · git · db  │
└─────────┘                              └──────┬───────────────────────┘
     ▲                                          │ spawns (ACP agents, worktree per task)
     │ MCP (stdio)                              ▼
     │        ┌──────────────┐   ┌───────────────────────────┐
     └─────── │ orchestrator │   │ one agent per workflow step │  · ACP events report progress
              └──────────────┘   └───────────────────────────┘  · tools via `ariadne mcp serve`
```

[How Ariadne works](docs/how-it-works.md) follows one goal from the question
the orchestrator asks to the last column of its workflow.

<details>
<summary><b>The top-level tree</b></summary>

```
bench/           reproducible experiments that select production defaults — see
                 bench/README.md
crates/          the Rust workspace: ariadned, the ariadne CLI and the libraries
                 they share — crate by crate in crates/AGENTS.md
docs/            the manual: install, the CLI, events, configuration
scripts/         install.sh / uninstall.sh + lib.sh, their shared step output
ui/              Ariadne Desktop (Tauri 2 + React): a REST/SSE client of the daemon's
                 TCP listener, outside the cargo workspace — see ui/AGENTS.md and
                 ui/README.md
```

</details>

## Documentation

| Page | What it covers |
| --- | --- |
| [Installing Ariadne](docs/install.md) | the installer, ACP agents, the release assets, the desktop app, and the daemon service |
| [Shell completion](docs/shell-completion.md) | dynamic completions for bash, zsh and fish, and the static fallback |
| [Using the CLI](docs/cli.md) | the command tour, the reference, and how tables and colour are printed |
| [Workflows](docs/workflows.md) | the document syntax, the two shipped workflows, every gate, and a repository's default and a goal's override |
| [Following what happens](docs/following-events.md) | events, the log streams and the `--watch` tables |
| [Configuration](docs/configuration.md) | every key of `~/.ariadne/config.toml`, and the environment that addresses a daemon |
| [Permission modes](docs/permissions.md) | automatic, prompted, and remembered ACP permission answers |
| [Resuming a session](docs/resuming-sessions.md) | listing every session and continuing one, live, ended, or started outside Ariadne |
| [How Ariadne works](docs/how-it-works.md) | planning, a task's workflow columns, and ACP sessions |
| [The forge integration](docs/forge.md) | enabling GitHub or GitLab, the pull request and review sessions, webhooks and the tunnel, issues |
| [Ariadne Desktop](ui/README.md) | running the desktop app |

## Development

[`AGENTS.md`](AGENTS.md) holds the conventions for changing this repository,
the commit types included — they are written down there and nowhere else — and
points at the file each area keeps: [`crates/AGENTS.md`](crates/AGENTS.md) for
the Rust workspace and its cargo commands,
[`ui/AGENTS.md`](ui/AGENTS.md) for the desktop app. [`specs/`](specs/README.md)
describes what each subsystem does, as it stands. [`bench/`](bench/README.md)
holds reproducible experiments that select production defaults. The release loop is in
[`.github/RELEASING.md`](.github/RELEASING.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE).

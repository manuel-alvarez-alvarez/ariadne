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
  hands each task to an <b>author</b> agent that owns it until it lands,<br>
  and gates that landing behind one or more <b>reviewer</b> agents.
</p>

<p align="center">
  Every agent runs in its own tmux session and its own git worktree, so you can attach to any of
  them at any moment and take over. <b>Claude Code</b>, <b>OpenAI Codex CLI</b> and
  <b>OpenCode</b> all drive the work, and a goal picks the model and the effort each agent runs
  at.
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
<h4>🧩 Three agent CLIs, one interface</h4>
Claude Code, OpenAI Codex CLI and OpenCode. A model is spelled
<code>&lt;agent_kind&gt;:&lt;model&gt;</code>, and <code>--effort</code> beside it says how deeply
it reasons.
</td>
<td width="50%" valign="top">
<h4>📝 A plan you agree to</h4>
The orchestrator asks until nothing about the goal is open, then writes the tasks. Nothing runs
until the plan is finalized, and <code>ariadne task update</code> is yours until it does.
</td>
</tr>
<tr>
<td width="50%" valign="top">
<h4>✅ Review gates every landing</h4>
Reviewers read the diff in detached read-only worktrees and approve or request changes. A change
request resumes the author with the feedback.
</td>
<td width="50%" valign="top">
<h4>🚢 Three ways a task can end</h4>
<code>merge</code> squashes onto the base branch, <code>pull_request</code> opens a request and
sees it through the forge, and <code>none</code> lands nothing at all.
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
<h4>🖥️ Attach whenever you want</h4>
Every session is a live tmux pane. <code>ariadne attach</code> drops you into it, and
<code>ariadne attention</code> says who is waiting for you.
</td>
<td width="50%" valign="top">
<h4>⚡ Nothing polls</h4>
The daemon streams: events, agent terminals and its own log, over a REST API with OpenAPI at
<code>/api-docs/openapi.json</code> and SSE at <code>/v1/events/stream</code>.
</td>
</tr>
</table>

## Ariadne Desktop

The same daemon, in a window: goals, tasks, diffs, agent terminals and the live
event stream. It is a pure REST/SSE client of the daemon's TCP listener — the
same code runs in a browser tab and in the packaged [Tauri
2](https://v2.tauri.app) app — and `scripts/install.sh` installs it beside the
CLI.

<p align="center">
  <img src="assets/demo-ui.gif" alt="Ariadne Desktop showing a goal, its tasks and an agent terminal">
</p>

> [!NOTE]
> The daemon's TCP listener is off by default. Set `tcp_listen` in
> `~/.ariadne/config.toml` before the app can reach it — see
> [Configuration](docs/configuration.md).

## Quick start

```sh
scripts/install.sh                     # CLI, daemon service, completions, desktop app
ariadne daemon start                   # unix socket at ~/.ariadne/ariadne.sock

ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model claude_code:claude-sonnet-5
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
│  (CLI)  │                              │  scheduler · tmux · git · db │
└─────────┘                              └──────┬───────────────────────┘
     ▲                                          │ spawns (tmux, worktree per task)
     │ MCP (stdio)                              ▼
     │        ┌──────────────┐   ┌────────┐   ┌──────────┐
     └─────── │ orchestrator │   │ author │   │ reviewer │  · hooks report events
              └──────────────┘   └────────┘   └──────────┘  · tools via `ariadne mcp serve`
```

[How Ariadne works](docs/how-it-works.md) follows one goal from the question
the orchestrator asks to the commit on the base branch.

<details>
<summary><b>The top-level tree</b></summary>

```
crates/          the Rust workspace: ariadned, the ariadne CLI and the libraries
                 they share — crate by crate in crates/AGENTS.md
assets/opencode-plugin/  event-forwarding plugin installed for OpenCode
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
| [Installing Ariadne](docs/install.md) | the installer, the release assets, the desktop app, the Codex hook trust, the daemon service |
| [Shell completion](docs/shell-completion.md) | dynamic completions for bash, zsh and fish, and the static fallback |
| [Using the CLI](docs/cli.md) | the command tour, the reference, and how tables and colour are printed |
| [Following what happens](docs/following-events.md) | events, the log streams and the `--watch` tables |
| [Configuration](docs/configuration.md) | every key of `~/.ariadne/config.toml`, and the environment that addresses a daemon |
| [How Ariadne works](docs/how-it-works.md) | planning, authoring, review, landing, and the sessions behind them |
| [Ariadne Desktop](ui/README.md) | running the desktop app |

## Development

[`AGENTS.md`](AGENTS.md) holds the conventions for changing this repository,
the commit types included — they are written down there and nowhere else — and
points at the file each area keeps: [`crates/AGENTS.md`](crates/AGENTS.md) for
the Rust workspace and its cargo commands,
[`ui/AGENTS.md`](ui/AGENTS.md) for the desktop app. [`specs/`](specs/README.md)
describes what each subsystem does, as it stands. The release loop is in
[`.github/RELEASING.md`](.github/RELEASING.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE).

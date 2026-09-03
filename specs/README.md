# Specs

What Ariadne does, one subsystem at a time. These describe the system **as it
stands**, not the order it was built in: a design that was later reversed is
gone from here and lives only in the git history the `commits:` field points
at.

They are written to be referenced: a planner names the spec a task works from,
and an engineer reads it in its own worktree. So each one is short, states its
rules as numbered sentences, and ties every acceptance criterion to the test
that proves it.

| # | Spec | What it settles |
| --- | --- | --- |
| 001 | [Goal and task lifecycle](001-goal-and-task-lifecycle.md) | statuses, the transition table, dependencies, failure and retry |
| 002 | [Repositories, branches and worktrees](002-repositories-branches-and-worktrees.md) | checkouts, base branches, branch naming, the worktree per role |
| 003 | [Spec-driven planning](003-spec-driven-planning.md) | the spec conversation, landing the spec, writing the plan |
| 004 | [Engineering and review rounds](004-engineering-and-review-rounds.md) | the engineer, the reviewers, verdicts and rounds |
| 005 | [Landing strategies](005-landing-strategies.md) | `direct` and `pull_request`, the landing briefing, merge verification |
| 006 | [Prompts and Simplified Technical English](006-prompts-and-simplified-technical-english.md) | the layers of text, who owns each, the English all of it is in |
| 007 | [Agent CLI adapters](007-agent-cli-adapters.md) | Claude Code, Codex, OpenCode: argv, env, hooks, resume |
| 008 | [Sessions, terminals and logs](008-sessions-terminals-and-logs.md) | tmux panes, log streams, typing, resizing, confirmed delivery |
| 009 | [Scheduler, attention and watchdogs](009-scheduler-attention-and-watchdogs.md) | the reconciliation loop, the quiet clock, what needs a human |
| 010 | [Session compaction](010-session-compaction.md) | the hand-offs that owe one, and what a compacting pane is spared |
| 011 | [Models, effort and pins](011-models-effort-and-pins.md) | the catalog, `<agent>:<model>`, effort, and where a pin is set |
| 012 | [HTTP API, event stream and usage](012-http-api-events-and-usage.md) | transports, the envelope, SSE, token accounting |
| 013 | [MCP tool surface](013-mcp-tool-surface.md) | the tools each role sees, and the rules every session is given |
| 014 | [Command-line interface](014-command-line-interface.md) | the command tree, flags, failures, completions, `doctor` |
| 015 | [Desktop app](015-desktop-app.md) | Ariadne Desktop, and its parity with the CLI |
| 016 | [Install, service and release](016-install-service-and-release.md) | the installer, the service, release-please, the migration policy |

## Writing one

- One file per subsystem, `NNN-kebab-slug.md`, numbered in the order they were
  written.
- YAML frontmatter: `id`, `status`, `updated`, `areas`, `commits`, `tests`.
- Sections: **Scope** (in and out), **Behavior** (numbered rules), **Acceptance
  criteria** (each citing the test that proves it), **Sources**. Add **Known
  gap** where something is deliberately unbuilt.
- A rule belongs to exactly one spec. Where another needs it, reference the
  number rather than restating it.
- Update the spec in the same change as the code. A spec that disagrees with
  its tests is a bug in one of the two.

# crates/AGENTS.md

Conventions for changing the Rust workspace under `crates/`. Read this before
editing anything here; commit-message and history rules live in the root
[`AGENTS.md`](../AGENTS.md).

## The crates

```
ariadne-core     domain types, task state machine, the shared binary/path probe
ariadne-api      REST DTOs / error shape (single source of truth for OpenAPI)
ariadne-store    SQLite persistence (sqlx, one embedded init migration)
ariadne-client   REST client (unix socket / TCP), used by CLI + MCP
ariadne-daemon   ariadned: axum API, scheduler, tmux/git managers, agent adapters
ariadne-cli      ariadne: CLI, MCP server (`mcp serve`), hook sink (`agent-event`)
```

The desktop app under `ui/` is not part of this workspace; it has its own
[`ui/AGENTS.md`](../ui/AGENTS.md).

## Checks

The test runner is [cargo-nextest](https://nexte.st), which is what CI runs:
`cargo install cargo-nextest --locked`, or `cargo binstall cargo-nextest`.

```sh
cargo build -p ariadne-cli    # the spawn-plan test launches the built CLI
cargo nextest run
cargo clippy --all-targets
```

The build comes first because one test spawns the `ariadne` binary from the
target directory beside the test binary, and `cargo nextest run` does not
build it.

Nothing is `#[ignore]`d: the suite drives real `tmux` panes and real `git`
worktrees, both of which Ariadne requires anyway, and the keystroke tests
script a TUI in `python3`. A machine missing one of the three gets failures
rather than a quiet pass.

Run `cargo nextest run` and `cargo clippy --all-targets` before you commit.

`cargo test` still works and does not need nextest installed, but it runs the test
binaries one at a time where nextest pools tests across all of them, so a full
run takes around three times as long. It is also the only way to run doctests,
which nextest does not support; there are none today, and CI keeps it that way
with `cargo test --doc --workspace`.

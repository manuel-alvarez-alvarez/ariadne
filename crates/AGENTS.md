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
ariadne-daemon   ariadned: axum API, scheduler, ACP runtime, agent registry, git managers
ariadne-cli      ariadne: CLI, MCP server (`mcp serve`), ACP session console
```

The desktop app under `ui/` is not part of this workspace; it has its own
[`ui/AGENTS.md`](../ui/AGENTS.md).

## Checks

The test runner is [cargo-nextest](https://nexte.st), which is what CI runs:
`cargo install cargo-nextest --locked`, or `cargo binstall cargo-nextest`.

```sh
cargo nextest run
cargo clippy --all-targets
```

Nothing is `#[ignore]`d: the suite drives real `git` worktrees, which Ariadne
requires anyway, and a stub ACP agent that `tests/common/acp.rs` writes in
`python3` and registers as the agent `stub`. Every seat a test starts runs on
that stub, so the suite needs no coding-agent CLI installed. A machine missing
`git` or `python3` gets failures rather than a quiet pass.

Run `cargo nextest run` and `cargo clippy --all-targets` before you commit.

`cargo test` still works and does not need nextest installed, but it runs the test
binaries one at a time where nextest pools tests across all of them, so a full
run takes around three times as long. It is also the only way to run doctests,
which nextest does not support; there are none today, and CI keeps it that way
with `cargo test --doc --workspace`.

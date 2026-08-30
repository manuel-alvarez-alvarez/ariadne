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

```sh
cargo test                                  # unit + store integration tests
cargo clippy --all-targets
cargo build -p ariadne-cli                  # the tmux/git integration tests spawn
cargo test -p ariadne-daemon -- --ignored   # target/debug/ariadne, so build it first
```

Run `cargo test` and `cargo clippy --all-targets` before you commit.

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
ariadne-console  the session console: transcript model, markdown, the inline pane and its loop
ariadne-knowledge the knowledge base: language registry, tree-sitter parser, the SQLite symbol store
ariadne-daemon   ariadned: axum API, scheduler, ACP runtime, agent registry, git managers
ariadne-cli      ariadne: CLI, MCP server (`mcp serve`), `ariadne attach` over ariadne-console
```

The desktop app under `ui/` is not part of this workspace; it has its own
[`ui/AGENTS.md`](../ui/AGENTS.md).

## Checks

The test runner is [cargo-nextest](https://nexte.st), which is what CI runs:
`cargo install cargo-nextest --locked`, or `cargo binstall cargo-nextest`.

On a task branch, run the checks of the crates you changed, and only those:

```sh
cargo nextest run -p <crate>              # the crate
cargo nextest run -p <crate> -E 'test(/^<module>::/)' # one test file of it
cargo clippy -p <crate> --all-targets
cargo fmt
scripts/check-unused-rust                 # a library pub item no other file names
```

`unreachable_pub` is a workspace lint, so a `pub` item that its crate does not
export is a warning. It cannot see a `pub` item of a library crate that no
other crate uses: to rustc, that is the library's interface.
`scripts/check-unused-rust` lists each such item, and fails on one. Remove the
item, or make it private. An item that a derive, serde, utoipa or a public
signature reaches goes in the script's `IGNORED`, with its reason.

The daemon's integration tests are one test binary, `it`: every file under
`crates/ariadne-daemon/tests/it/` is a module that `tests/it/main.rs`
declares, and a new file runs only once it is declared there. Keep it one
binary. A binary per file compiles `common` again and links the whole daemon
again, and a change to the daemon then rebuilds all of them.

A daemon test does not wait on a clock. Wait for the thing itself:

- For something that must happen, poll with `eventually(TIMEOUT, …)`. It
  returns as soon as the check holds, so the long `TIMEOUT` costs nothing.
- For something that must not happen, listen for `QUIET`. Where events come
  in a fixed order, the next expected event proves that nothing came between.
- For a daemon timeout that the test is about, shorten it with
  `harness().timeouts(Timeouts { …: RUNS_OUT, ..Timeouts::default() })`.
  Put every such timeout in `ariadne_daemon::timeouts::Timeouts`, never in a
  constant.
- For a stub that must hold still, make it wait for a file that the test
  writes (`wait_for`, `updates_when`), not for a number of seconds.
- Do not create a new executable for each test. On macOS each new executable
  is checked before it first starts, one at a time, so the tests wait for each
  other. Share one file and link it, as the stub launcher does.

Before a commit on `main`, run the whole workspace:

```sh
cargo nextest run
cargo clippy --all-targets
cargo fmt --all -- --check
scripts/check-unused-rust
```

Nothing is `#[ignore]`d: the suite drives real `git` worktrees, which Ariadne
requires anyway, and a stub ACP agent that `tests/it/common/acp.rs` writes in
`python3` and registers as the agent `stub`. Every seat a test starts runs on
that stub, so the suite needs no coding-agent CLI installed. A machine missing
`git` or `python3` gets failures rather than a quiet pass.

The dev profile carries line tables rather than full debug info, and none
for dependencies (`[profile.dev]` in the root `Cargo.toml`). A panic still
names its file and line and a backtrace still reads; what goes is stepping
through a dependency in a debugger. It is there because linking is what the
edit-test loop waits on and it is single-threaded per binary: rebuilding the
daemon's tests after one edit went from 7.3s to 2.4s. Switching it back
rebuilds the world once, so do that in a branch of its own.

`cargo test` still works and does not need nextest installed, but it runs the test
binaries one at a time where nextest pools tests across all of them, so a full
run takes around three times as long. It is also the only way to run doctests,
which nextest does not support; there are none today, and CI keeps it that way
with `cargo test --doc --workspace`.

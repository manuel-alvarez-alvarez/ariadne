# crates/AGENTS.md

Conventions for changing the Rust workspace under `crates/`. Read this before
editing anything here; commit-message and history rules live in the root
[`AGENTS.md`](../AGENTS.md).

## The crates

```
ariadne-core     id, models, state_machine, workflow, ACP and the binary/path probe
ariadne-api      REST DTOs and errors for agents, goals, tasks, sessions, skills and workflows
ariadne-store    SQLite repositories, migrations, and defaults that seed shipped skills and workflows
ariadne-client   REST and SSE client plus home, socket and TCP endpoint resolution for CLI and daemon
ariadne-console  terminal console: ANSI, markdown, transcript, theme and inline TUI; daemon hosts its terminal WebSocket
ariadne-daemon   ariadned: HTTP API, scheduler, ACP, agents, git, forge, webhooks, tunnel, AI permissions, stats and failure diagnosis
ariadne-cli      ariadne commands, output, completion, MCP server, attach console and store-backed doctor
```

The desktop app under `ui/` is not part of this workspace; it has its own
[`ui/AGENTS.md`](../ui/AGENTS.md).

## Checks

The test runner is [cargo-nextest](https://nexte.st), which CI runs with the
`ci` profile in [`../.config/nextest.toml`](../.config/nextest.toml). It caps
the daemon integration test group at two threads and writes JUnit output.
Install it with `cargo install cargo-nextest --locked` or
`cargo binstall cargo-nextest`. [`../.cargo/config.toml`](../.cargo/config.toml)
sets the Git environment for every Cargo process.

On a task branch, run the checks of the crates you changed, and only those:

```sh
cargo nextest run -p <crate>              # the crate
cargo nextest run -p ariadne-daemon -E 'test(/^<module>::/)' # one `it` module
cargo nextest run -p <crate> -E 'binary(<name>)' # one crate test binary
cargo clippy -p <crate> --all-targets -- -D warnings
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
declares, and a new file runs only once it is declared there. Keep one binary
to avoid rebuilding `common` and relinking the daemon for every file.

A daemon test does not wait on a clock. Wait for the thing itself:

- For something that must happen, poll with `eventually(TIMEOUT, …)`. It
  returns as soon as the check holds, so the long `TIMEOUT` costs nothing.
- For something that must not happen, listen for `QUIET`. Where events come
  in a fixed order, the next expected event proves that nothing came between.
- For a daemon timeout that the test is about, shorten it with
  `harness().timeouts(Timeouts { …: RUNS_OUT, ..Timeouts::default() })`.
  Put every such timeout in `ariadne_daemon::timeouts::Timeouts`, never in a
  constant.
- For a stub that must hold still, use the `wait_for` and `updates_when` keys
  in its JSON turn script in `tests/it/common/acp.rs`.
- Do not create a new executable for each test: macOS checks each executable
  before it starts, so share one file and link it, as the stub launcher does.

nextest marks a test `SLOW` if it runs past the `slow-timeout` of 10 seconds.
A test in that marker waits on a clock or a scheduler tick. Find the wait and
rework the test to poll the thing itself, with `eventually(TIMEOUT, …)` or
listen to events, so the test returns as soon as its assertion passes. See
the rules above.

## What a reviewer flags

A reviewer fails a Rust change for any of these:

- A `sleep` outside a `QUIET` listen. Ask for `eventually(TIMEOUT, …)` or a `QUIET` listen instead.
- A `Duration` constant in a test, not a `Timeouts` field. Ask for the timeout in `ariadne_daemon::timeouts::Timeouts`.
- A test nextest marks `SLOW` in the reviewer's run. Ask the author to poll the thing itself, per the `SLOW` rule above.
- A test that passed only on a second run. Ask the author to prove it: run 100 times under the bounded busy loop in the root [`AGENTS.md`](../AGENTS.md).
- A wall clock read into an assertion, in any crate. Ask the author to wait for the thing itself, never the clock.
- A stub or child process that outlives its test. Ask the author to end it when the test ends, even on failure, per [Processes you leave behind](../AGENTS.md#processes-you-leave-behind).
- A test that depends on another test, or on thread order. Ask for a test that holds under the `ci` profile's two-thread cap.

Before a commit on `main`, run the whole workspace:

```sh
cargo machete .
cargo build --workspace --all-targets
cargo nextest run --workspace --profile ci
cargo test --doc --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
scripts/check-unused-rust
(cd ui/src-tauri && cargo fmt --all -- --check)
(cd ui/src-tauri && cargo check)
```

`cargo machete` is [cargo-machete](https://github.com/bnjbvr/cargo-machete)
(`cargo binstall cargo-machete@0.9.2`, the version CI pins). It fails on a
dependency that no source file names, in the workspace and in `ui/src-tauri`.
Remove the dependency. Where a crate is used only through a macro or a
feature, keep it and list it under `[package.metadata.cargo-machete]` with a
comment that says why.

Nothing is `#[ignore]`d: the suite drives real `git` worktrees, which Ariadne
requires anyway, and a stub ACP agent that `tests/it/common/acp.rs` writes in
`python3` and registers as the agent `stub`. Every seat a test starts runs on
that stub, so the suite needs no coding-agent CLI installed. A machine missing
`git` or `python3` gets failures rather than a quiet pass.

`[profile.dev]` in the root `Cargo.toml` keeps line tables for workspace code
and no debug information for dependencies; change it only in a separate branch.

Use `cargo test` only for doctests, because nextest runs normal test binaries
faster; CI runs `cargo test --doc --workspace`.

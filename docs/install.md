# Installing Ariadne

`scripts/install.sh` installs the CLI, the daemon service, shell completions
and the desktop app. It is idempotent — re-run it to upgrade.

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

## Agent CLIs

Ariadne supports Claude Code, OpenAI Codex CLI, OpenCode, and
[ACP-compatible agents](https://agentclientprotocol.com). Install each CLI
separately and make its executable available to the daemon on `PATH`.

For ACP, expose the selected agent as an executable named `acp`. A small
wrapper can add the command or mode that starts its ACP server. Ariadne speaks
ACP version 1 over standard input and output. The agent must offer a `model`
session option. It must also offer a `thought_level` option when the pin has an
effort. Ariadne's ACP flags are empty by default because agents use different
permission flags. Configure them with `ariadne agent update acp`.

## Where the binaries come from

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

## The desktop app

The desktop app lands in `/Applications/Ariadne Desktop.app` on macOS
(`~/Applications` when `/Applications` is not writable) and as
`$PREFIX/ariadne-desktop` on Linux. Built rather than downloaded it needs
`npm`, and without it that one step is skipped instead of failing the install.
Where everything went is recorded in `~/.ariadne/install.env`, which is what
the uninstaller reads.

## Trusting the Codex hooks

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
codex-hooks`** — the `PostCompact` hook, which tells the daemon the agent is
back at its prompt after a compaction, is the latest addition. `ariadne doctor`
reads the verdicts back out of codex's config and names any declared event that
has none. Which events are declared, and why each one, is in
[`crates/ariadne-core/src/codex_hooks.rs`](../crates/ariadne-core/src/codex_hooks.rs).

## The daemon service

The daemon then runs as a user service with restart-on-failure, and
`ariadne daemon start|stop|restart` drives that service rather than the bare
process wherever `~/.ariadne/install.env` records one — `launchctl kickstart
-k` / `launchctl bootout`, `systemctl --user` — saying which command it used;
`ariadne daemon status` says which manager is holding the daemon up.

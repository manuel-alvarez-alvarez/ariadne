# Installing Ariadne

Install Ariadne, then make at least one ACP agent available. The installer
adds the CLI, daemon service, shell completions, and desktop app. Re-run it to
upgrade; it is idempotent.

```sh
scripts/install.sh                         # latest release
scripts/install.sh --version v0.2.0        # one release
scripts/install.sh --build-from-source     # build this checkout
scripts/install.sh --prefix /usr/local/bin # choose the binary directory
scripts/install.sh --no-ui                 # CLI and daemon, without the app
scripts/uninstall.sh                       # keep ~/.ariadne data
scripts/uninstall.sh --purge               # remove the data too
```

Run `scripts/install.sh --help` for the complete flag list.

## Connect an agent

Every agent runs through the [Agent Client Protocol][ACP] (ACP). Ariadne
includes these registry entries. Install the command you want to use and make
it available on the daemon's `PATH`.

| Agent id | Command Ariadne starts |
| --- | --- |
| `claude-code-acp` | `claude-code-acp` |
| `codex-acp` | `codex acp` |
| `opencode-acp` | `opencode acp` |

Start the daemon, then check which agents and models it found:

```sh
ariadne daemon start
ariadne doctor
ariadne models ls
```

`ariadne doctor` reports an unavailable command or an agent that does not meet
the ACP contract. `ariadne models ls` lists only models that a ready agent
offered. Use the printed `<agent-id>:<model-id>` value in `--model`; there is
no implicit agent or model default.

### Add another ACP agent

Add a `[[acp_agents]]` entry to `~/.ariadne/config.toml`, then restart the
daemon so it discovers the command:

```toml
[[acp_agents]]
id = "my-agent"
command = ["my-agent", "acp"]
```

```sh
ariadne daemon restart
ariadne doctor
ariadne models ls --agent my-agent
```

The id must be non-empty, unique, and contain no `:`. It becomes the model
prefix, for example `my-agent:my-model`. Ariadne starts the command exactly as
the list gives it. To pass flags to every session of a registered agent, use
`ariadne agent update my-agent --flag --my-agent-flag`; `ariadne agent ls`
shows the active flags. The next launch uses the new flags.

### ACP capability contract

An agent must communicate over standard input and output, negotiate ACP
version 1, support `session/new` and `session/prompt`, and offer at least one
`model` session configuration option. Ariadne rejects an agent that lacks any
of those requirements.

An agent may omit `thought_level`; Ariadne can run it, but the model has no
selectable effort. `session/list` enables session discovery and adoption.
`loadSession` enables restoring a conversation after the daemon restarts.
`ariadne doctor` names a missing required capability as an error and an absent
optional capability as a limitation.

## Where the binaries come from

By default the binaries and desktop app come from the GitHub release named by
`--version` (the latest release when it is omitted), for the target triple the
machine runs. The downloaded files carry a build provenance attestation. The
installer verifies each file with `gh attestation verify` before installing it,
so it requires the [GitHub CLI](https://cli.github.com) and `gh auth login`.
A failed verification changes nothing. `--build-from-source` compiles this
checkout instead and needs neither `gh` nor a published release.

## The desktop app

The app installs to `/Applications/Ariadne Desktop.app` on macOS
(`~/Applications` when needed), and to `$PREFIX/ariadne-desktop` on Linux.
Its TCP connection is off by default; set `tcp_listen` in
`~/.ariadne/config.toml` before connecting the app. See
[Configuration](configuration.md).

## The daemon service

The installer registers a user service with restart-on-failure. Use
`ariadne daemon start`, `ariadne daemon stop`, or `ariadne daemon restart` to
control it, and `ariadne daemon status` to see its manager. The daemon starts
the ACP agents as child processes and keeps their conversations available
through the console.

[ACP]: https://agentclientprotocol.com

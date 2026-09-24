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
ships a snapshot of the ACP registry — the index of every agent that speaks
the protocol, and of the command each one is run by — and installs none of
them: the daemon registers the agents whose command is on its `PATH`. Install
the one you want to use, and it is there at the daemon's next start.

| Agent id | Command Ariadne starts | Where it comes from |
| --- | --- | --- |
| `claude-acp` | `claude-agent-acp` | `npm install -g @agentclientprotocol/claude-agent-acp` |
| `codex-acp` | `codex-acp` | `npm install -g @agentclientprotocol/codex-acp` |
| `opencode` | `opencode acp` | [OpenCode](https://opencode.ai) itself |

The registry names 41 agents, and those three are only the ones this page
walks through. An agent takes the id the registry gives it, which is the half
of a `<agent-id>:<model-id>` pin before the `:`.

Start the daemon, then check which agents and models it found:

```sh
ariadne daemon start
ariadne doctor
ariadne models ls
```

`ariadne doctor` reports an agent that does not meet the ACP contract, and
says so where no agent at all is ready. An agent it does not name is an agent
whose command is not on the daemon's `PATH` — a service carries the `PATH` of
the service file it was installed with, which is not always the shell's.
`ariadne models ls` lists only models that a ready agent offered. Use the
printed `<agent-id>:<model-id>` value in `--model`; there is no implicit agent
or model default.

### Add an agent the registry does not name

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
prefix, for example `my-agent:my-model`. An entry whose id is one the registry
found replaces that agent: the configured command is the one Ariadne starts,
exactly as the list gives it. To pass flags to every session of an agent, use
`ariadne agent update my-agent --flag --my-agent-flag`; `ariadne agent ls`
shows the active flags. The next launch uses the new flags.

### ACP capability contract

An agent must communicate over standard input and output, negotiate ACP
version 1, support `session/new`, and offer at least one `model` session
configuration option. Ariadne rejects an agent that lacks any of those
requirements. Discovery sends no prompt, so probing an agent costs no model
turn.

Every daemon start asks each agent to `initialize`. The models and efforts
come off a `session/new`, which Ariadne opens only when an agent reports a
version it has not read yet, and closes again. The database keeps the result
under that version, so later starts open no session at all. An agent that
reports no version is read on every start. After you configure a new model
without upgrading the agent, for example by pulling a local one, re-read
every catalog with `POST /v1/acp-agents/refresh`.

An agent may omit `thought_level`; Ariadne can run it, but the model has no
selectable effort. `session/list` enables listing that agent's outside
sessions in `ariadne session ls`. `loadSession` enables resuming one of them,
and reviving an ended Ariadne session, including after the daemon restarts.
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

On Linux, install the WebKitGTK 4.1 runtime before running the installer:

```sh
sudo apt install libwebkit2gtk-4.1-0       # Debian / Ubuntu
sudo dnf install webkit2gtk4.1            # Fedora
sudo pacman -S webkit2gtk-4.1             # Arch Linux
```

The app uses the host's WebKitGTK libraries. If `libwebkit2gtk-4.1.so.0` is
missing, the installer skips the app and GNOME registration. It still
installs the CLI, daemon service, and completions. Install the runtime, then
re-run the installer. This check also applies to `--build-from-source`;
source builds still need the development dependencies.

The app installs to `/Applications/Ariadne Desktop.app` on macOS
(`~/Applications` when needed), and to `$PREFIX/ariadne-desktop` on Linux.
Linux releases supply the binary and GNOME icon in
`ariadne-desktop-<target>.tar.gz`. Source builds install the plain binary
without a bundle and take the icon from `ui/src-tauri/icons/`.
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

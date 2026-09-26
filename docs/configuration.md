# Configuration

`~/.ariadne/config.toml` (all optional):

```toml
socket_path = "/Users/me/.ariadne/ariadne.sock"
db_path = "/Users/me/.ariadne/ariadne.db"
worktree_root = "/Users/me/.ariadne/worktrees"
run_dir = "/Users/me/.ariadne/run"
tcp_listen = "127.0.0.1:7676"     # enables the TCP listener (for web/desktop UIs)
log_filter = "info,ariadne_daemon=debug"
cli_bin = "/usr/local/bin/ariadne" # runs the Ariadne MCP server for ACP sessions
                                   # (default: sibling of ariadned)
delete_merged_worktrees = true     # remove task worktrees after merge (default);
                                   # false keeps them for inspecting finished work
delete_merged_branches = true      # only applies when worktrees are deleted too:
                                   # a kept author worktree pins the task branch
prevent_sleep = true               # hold a system sleep inhibition while any agent
                                   # session is live, so the box does not idle-sleep
                                   # out from under a working agent (default)
acp_registry_url = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json"
                                   # download the index only on an explicit refresh
python_bin = "/opt/python3.12/bin/python3"
                                   # the Python that the AI permission model
                                   # installs into; it has to be Python 3.12 or 3.13
                                   # (default: python3.13, python3.12, then python3 on PATH).
                                   # See Permission modes.

[[acp_agents]]                     # an agent of your own, or one the registry
id = "my-agent"                    # names under another command
command = ["my-agent", "acp"]      # program followed by its arguments
```

The agents of a daemon are the agents of the ACP registry whose command is on
its `PATH`: Ariadne ships a snapshot of that index — `claude-acp`,
`codex-acp`, `goose`, `opencode` and 37 more — and installs none of them. An
`[[acp_agents]]` entry adds an agent the registry does not name, and one whose
id the registry does name replaces it, command and all. The daemon probes
every entry at startup. Where an agent is found under the name of its package
rather than its own — `claude-acp` is installed as `claude-agent-acp` — the
command has to be the file that package installed, and it has to answer as an
ACP agent: a program that only shares the name, as Visual Studio Code's `code`
shares MiniMax Code's, is not started and is not listed. See [Installing Ariadne](install.md) to add an agent,
and [Permission modes](permissions.md) to choose, per repository, how it
handles tool requests.

`POST /v1/acp-agents/refresh` downloads the index from `acp_registry_url`,
then searches `PATH` again and probes every agent. The download has a
30-second timeout. The database keeps the last accepted document, its URL,
and its fetch time in one row. A failed download or refused document logs a
warning and keeps that row. Refresh still searches `PATH`, probes agents,
and returns the agent list.

Startup downloads nothing. It uses the kept index only when its fetch time
is after the snapshot date at midnight UTC. Otherwise, it uses the shipped
snapshot. Changing the URL does not discard the last good copy.

`python_bin` belongs to the AI permission model, the model the `ai` permission
mode answers with. It is the interpreter the model's virtual environment is
built from — set it where the
first supported Python on the daemon's own `PATH` is not 3.12 or 3.13, or where
you want the model on a different interpreter. Nothing is installed into that interpreter: the
package and PyTorch go into `~/.ariadne/ai-permissions/venv`.
The daemon installs fixed model pins. `python_bin` does not turn the model on —
[Permission modes](permissions.md) does that.

`ariadned --check-config` reads that file and exits: a key the daemon would
refuse is named where it stands, without starting anything or touching the
daemon that is already running. `ariadned --help` lists every key above and
the two environment variables (`ARIADNE_HOME`, `RUST_LOG`) with a line each.

## The database of an earlier release

`db_path` has to be deleted before this version is started for the first time:
the schema's 29 migrations are squashed into one, so a database written by an
earlier release records migrations this one no longer ships and cannot be
opened. There is no upgrade from it — Ariadne is pre-1.0, and a database is
recreated rather than migrated. Delete it (with its `-wal` and `-shm` files)
and the daemon writes a fresh one on its next start; `ariadned` and `ariadne
doctor` both say so, by name, if it is still there.

## Keys the daemon refuses

The file is read strictly — an unknown key stops the daemon rather than being
ignored — so a `config.toml` naming `running_quiet_flag_secs` or
`running_quiet_resume_secs` has to drop them: the one watchdog that reads how
long a session has reported nothing keeps its own timeline (a nudge at three
minutes, the flag at ten, a relaunch at thirty) and takes neither key.

## Addressing another daemon

`ARIADNE_HOME` moves the whole home directory: daemon and CLI alike resolve
the socket from it (`--home` > `ARIADNE_HOME` > `~/.ariadne`, then that home's
`socket_path` > `<home>/ariadne.sock`), so every command addresses the daemon
of the home it runs in. `--endpoint` (whose old spelling `--host` still works)
overrides that with a socket path or `http://host:port`, and so does
`ARIADNE_ENDPOINT` — read after the flag and before the home, with the older
`ARIADNE_SOCKET`, which every agent session is still spawned with, honoured
after it.

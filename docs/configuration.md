# Configuration

`~/.ariadne/config.toml` (all optional):

```toml
socket_path = "/Users/me/.ariadne/ariadne.sock"
db_path = "/Users/me/.ariadne/ariadne.db"
worktree_root = "/Users/me/.ariadne/worktrees"
tcp_listen = "127.0.0.1:7676"     # enables the TCP listener (for web/desktop UIs)
log_filter = "info,ariadne_daemon=debug"
cli_bin = "/usr/local/bin/ariadne" # what starts every agent session (`ariadne _spawn`),
                                   # and their hook and MCP entry point
                                   # (default: sibling of ariadned)
delete_merged_worktrees = true     # remove task worktrees after merge (default);
                                   # false keeps them for inspecting finished work
delete_merged_branches = true      # only applies when worktrees are deleted too:
                                   # a kept author worktree pins the task branch
prevent_sleep = true               # hold a system sleep inhibition while any agent
                                   # session is live, so the box does not idle-sleep
                                   # out from under a working agent (default)
```

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

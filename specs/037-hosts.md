---
id: 037
status: current
updated: 2026-11-28
areas: [api, daemon, client, cli, store, docs, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/far_tasks/source_references.rs
  - crates/ariadne-daemon/tests/it/remote_tasks.rs
  - crates/ariadne-store/tests/store.rs
  - ui/scripts/host-events.test.mjs
  - crates/ariadne-client/src/endpoint.rs
  - crates/ariadne-daemon/tests/it/hosts.rs
  - crates/ariadne-daemon/src/hosts/setup/repositories.rs
  - crates/ariadne-daemon/tests/it/ssh_forward.rs
  - crates/ariadne-daemon/tests/it/portable_config.rs
  - crates/ariadne-daemon/tests/it/host_sync.rs
  - crates/ariadne-daemon/tests/it/host_health.rs
  - crates/ariadne-daemon/tests/it/host_reconnect.rs
  - crates/ariadne-daemon/tests/it/learned_permissions.rs
  - crates/ariadne-daemon/tests/it/host_queries.rs
  - crates/ariadne-daemon/tests/it/far_tasks.rs
  - crates/ariadne-cli/tests/hosts.rs
  - crates/ariadne-cli/src/commands/events.rs
  - crates/ariadne-cli/src/commands/query.rs
  - crates/ariadne-daemon/src/hosts.rs
  - crates/ariadne-daemon/src/hosts/sync.rs
  - ui/src/features/hosts/host-setups.test.tsx
  - ui/src/features/hosts/add-machine.test.tsx
  - ui/src/features/hosts/setup-progress.test.tsx
---

# Hosts

## Scope

In: database host connections, their validation, the
`GET /v1/hosts` route, the `POST /v1/hosts/{name}/reconnect` route,
`HostDto`, the `host_updated` event, `ssh_bin`, the
SSH forward the daemon keeps for each `ssh://` host, the push of the local
catalogs and settings to each host, and the query routes under
`/v1/hosts/{host}`. Out: a Rust SSH library, a reverse forward from the far
daemon, a reconnect of every host in one request, and any write from a host back to this
daemon. CLI selection follows spec 014 (rules 43 to 50). The transports and the
event stream are spec 012's. The desktop app's reading of the hosts — the host
picker, the Hosts screen and one stream per host — is spec
015's (rules 51 to 60).

## Behavior

1. The settings daemon database stores each named host's `name`, `endpoint`,
   optional `remote`, and optional `color`. It also stores an optional colour for `local`.
   An additive migration preserves existing data. Stored hosts start when the daemon starts.
   Startup and `ariadned --check-config` refuse any `[hosts]` table with the same error.
   The error tells users to add each host again on the Hosts screen or with `ariadne host add`.
   The daemon never imports configuration hosts. `ssh_bin` remains in `config.toml`.
2. `endpoint` accepts `ssh://[user@]host[:port]`, `http://host:port`, or an absolute socket path.
   `remote` accepts an absolute socket path or loopback `host:port`, only with an SSH endpoint.
   Schemes must be lowercase. API validation errors name the invalid field.
3. Host names contain only letters, digits, `-`, and `_`, and cannot be empty.
   The names `local` and `all` are reserved. Hosts cannot be renamed.
4. `GET /v1/hosts` returns `HostDto[]`. The first row is `local`; the others
   follow in database insertion order.
   Reconnecting a host preserves its position before and after restart.
   `HostDto` has `name`; `kind` (`local`, `direct`,
   `ssh`); `endpoint` (the address a client on this machine uses: for `local`
   the `tcp_listen` URL when set, else the socket path; for `direct` the
   configured value; for `ssh` null until a forward is up); `status` (`ready`,
   `connecting`, `down`); `error`; `remote`; `sync` (rule 22); and nullable `color` (rule 56).
5. An `ssh` host is `down`, with a null `endpoint` and the error
   `ssh forward not started`, until its forward starts. A `direct` host is
   `connecting` until its first health check (rule 30).
6. `host_updated` carries the full `HostDto` after each add or change.
   Runtime changes of status, endpoint, error, or sync also publish `host_updated`.
   Removal publishes `host_deleted` with the full removed `HostDto`.
7. OpenAPI carries the routes, DTOs, colour enum, and both host events.
8. `ariadned --help` lists `ssh_bin` and does not list the retired hosts table.
9. `ssh_bin` names the `ssh` program (default: `ssh` on the daemon's PATH).
   `ariadned --check-config` prints `ssh_bin = <value>`.
10. At startup, for each `ssh` host, the daemon picks a free loopback port and
    runs `<ssh_bin> -N -L 127.0.0.1:<port>:<remote> -o ServerAliveInterval=15
    -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes -o BatchMode=yes
    [-p <port>] [user@]host`. A socket path forwards to a unix socket; a
    `host:port` forwards to TCP. A host with no `remote` forwards to
    `<far $HOME>/.ariadne/ariadne.sock`. Before each forward starts, one call
    `<ssh_bin> -o BatchMode=yes [-p <port>] [user@]host 'printf %s "$HOME"'`
    gets the far user's `$HOME`. The stored `remote` stays empty and
    `HostDto.remote` stays `null`, so a host saved without a `remote` needs no
    edit. A call that fails, prints an empty or a non-absolute path, or runs
    past `ssh_home` makes the host `down` with the error `the far home lookup
    failed: <reason>`, plus SSH's last stderr line if any. The restart of rule
    12 follows. The call's process ends with the call, also on a reconnect and on
    an idle restart of the daemon, as a forward's does. A
    host with a `remote` makes no such call.
11. The host is `connecting` while the process starts. The daemon probes
    `GET /v1/health` through the port every `ssh_probe_interval`, each probe
    bounded by `ssh_probe`. On the first good answer the host is `ready` and
    its `endpoint` is `http://127.0.0.1:<port>`. When no good answer comes in
    `host_unanswered`, the daemon kills the process and the host is `down`
    with the error `the far daemon does not answer through the forward:
    <reason>`, followed by `: <line>` when SSH wrote a last stderr line. The
    restart of rule 12 follows.
    Forward cleanup waits until every process in its group is gone, including zombies,
    before publishing `down` or starting a replacement. This includes reconnect requests.
    Each cleanup attempt has a five-second deadline (`Timeouts::ssh_forward_end`).
    A zombie-only group can return `EPERM` on macOS; continue polling without an error until this deadline.
    Group disappearance is polled every ten milliseconds (`ssh_forward_poll`).
    Failure keeps the host `connecting` with an explicit cleanup error and attempt number.
    The daemon retains the old child and group, retries cleanup, and blocks replacement until the group disappears.
12. When the process exits, the host is `down`, `endpoint` is null and `error`
    is the last non-empty stderr line of the process (else its exit status).
    Guard and lifetime-watcher diagnostics never enter the SSH stderr pipe.
    The daemon starts it again after `ssh_retry`, doubled after each exit up
    to 60 seconds and reset by a `ready`; the host goes back to `connecting`.
13. Daemon shutdown ends every ssh child process and every health check
    (rule 30).
14. One forward carries requests, event streams and answers both ways. The
    forward is the user's own `ssh`: their keys, agent and `~/.ssh/config`
    apply.
15. CLI host selection follows spec 014.

### Health

30. The daemon checks `GET /v1/health` of each `ready` host every
    `host_check`, `ssh` and `direct` alike, each check bounded by
    `ssh_probe`. A good answer is a success with `status` `ok`. A `ready`
    host with no good answer for `host_unanswered` is `down`, and `error` is
    `<endpoint> has not answered for <host_unanswered>: <reason>`. An `ssh`
    host then loses its forward: the daemon kills the process, and rule 12
    starts a new one with its backoff. A `direct` host keeps its configured
    `endpoint`.
31. A `direct` host starts `connecting`. The daemon checks it every
    `ssh_probe_interval`. Its first good answer makes it `ready`. With no
    good answer in `host_unanswered` it is `down`, and `error` is
    `<endpoint> does not answer: <reason>`.
32. The daemon goes on to check a `direct` host that is `down`, every
    `host_check`. Its first good answer makes it `ready` again.
33. A `direct` host with a socket path is checked through that socket.
34. Each change of status publishes one `host_updated` (rule 6). The sync of
    the host stops when it leaves `ready`, and starts again on its next
    `ready` (rule 25).

### Reconnect

35. `POST /v1/hosts/{name}/reconnect` takes no body. It answers `200` with
    the `HostDto` of the host once the host has acted on the reconnect. The
    name `local` answers `400`, and an unknown name answers `404`, both in
    the error envelope.
36. A `down` `ssh` host starts a new forward at once. Its backoff goes back
    to `ssh_retry`, so a forward that then exits waits `ssh_retry` (rule 12).
37. A `connecting` `ssh` host loses its current try: the daemon kills the
    process and starts a new forward at once, with the backoff of rule 36.
38. A `ready` `ssh` host takes one health check at once. A good answer
    changes nothing: the forward and every open stream through it stay up.
39. A `ready` `ssh` host whose check fails is `down`, with the error
    `<endpoint> does not answer: <reason>`. The daemon kills the process
    and starts a new forward at once, with the backoff of rule 36.
40. A `direct` host takes one health check at once, and its status follows
    the answer: a good answer makes it `ready`, and a failed one makes it
    `down` with the error `<endpoint> does not answer: <reason>`.
41. Each change of the host publishes one `host_updated` (rule 6).
42. The OpenAPI document carries the route.

### Queries on a host

16. Every Prometheus and Loki route of spec 034 is also served under
    `/v1/hosts/{host}/<same path>`, with the same methods and parameters.
    `{host}` is a host name or `all`.
17. A host name sends the request, with its method, query string, form body
    and content type, to that host's endpoint (an `http://` URL or a socket
    path) and returns its status, content type and body. `local` is answered
    by this daemon's own query routes. An unknown host answers `404`, and a
    host whose status is not `ready` answers `503`, both in the error
    envelope; the `503` message names the status. A host that cannot be
    reached answers `502`. A path that is not a query route answers `404`,
    and a method it does not take answers `405`.
18. `all` runs the request on every ready host at once and answers
    `{"status": "success", "data": …}` in the shape of one daemon:
    - a vector, matrix or stream result is the concatenation of every
      host's, each series or stream with a `host` label (an attribute already
      named `host` becomes `exported_host`, with another `exported_` added
      while that key is taken);
    - a scalar or string result becomes a vector of one sample per host;
    - `series` answers every label set with `host`; `labels` adds `host`;
      the values of the label `host` are the host names; other label values
      are the union;
    - `metadata` is the union by metric, and `index/stats` keeps its flat
      fields (no envelope) and sums them;
    - `-/healthy` and `parse_query` answer as the first host does, and
      `parse_query` and `index/stats` carry `warnings` like any other.
19. A host that is not ready, cannot be reached, or answers a failure is
    skipped, and `warnings` lists it as `<host>: <reason>`. `warnings` is
    absent when every host answered. When no host answered: a `4xx` answer
    of a host is returned as it came, so a bad query is a `400`; otherwise
    the answer is `502`.
20. `ariadne query --host NAME|all` sends its route under
    `/v1/hosts/{host}` to the local daemon, and prints each warning on
    stderr as `warning: <host>: <reason>`. With `all` the table has `HOST`
    as its first column, from the label. A host name that is unknown or not
    `ready` ends the command with the usage exit code, before the query; the
    message names the configured hosts, or gives the status. Without
    `--host` the route is unchanged.
21. The OpenAPI document carries the routes of rule 16.

### Sync

22. `sync` is `state` (`in_sync`, `pending`, `pending_restart`, `failed`), `error` (string or
    null) and `missing_repositories` (`MissingRepositoryDto[]`: the local
    repository's `id`, and `forge`, its `owner/repo`, or null for a
    repository with no forge) and `skipped_permissions` (rule 27). `local`
    is always `in_sync` with an empty list and no skipped permission. A
    configured host starts `pending`.
23. When a host becomes `ready`, the daemon pushes, in order: every skill
    (`PUT /v1/skills/{name}`, else `POST /v1/skills` on a 404), every workflow
    (the same with the workflow routes), the repositories (rule 26), every
    learned permission (rule 27) and the AI permission settings (rule 28).
    Each item is a push of its own (rule 25). A host item with the same name
    or key is overwritten. An item that only the host has stays as it is.
    The host is `in_sync` when every push has landed.
24. After that, each local domain event of those items is pushed as it
    happens: a created or updated skill or workflow as in rule 23, a deleted
    one with `DELETE` (a 404 counts as done), and any repository event
    with rule 26. The host is `pending` while a push runs. A bus that lags
    is answered with the whole push of rule 23 again.
25. A failed push makes the host `failed`, with the reason of the last
    failure in `error`, while any push stays failed. A failed push holds back
    no other one: the later pushes run, and the failed ones are tried again,
    each on its own, after `host_sync_retry`. The wait is doubled after each
    round that leaves one failed, up to 60 seconds, and reset once none is.
    A later push of the same item ends its failed push, both when it is
    queued and when it runs, so no retry writes an older version over a
    newer one. When the host leaves `ready`, the tries stop and the host is
    `pending` with a null `error`. Its next `ready` starts the push of rule
    23 again. Queued or interrupted deletions remain pending across recovery.
    A later change of the same item supersedes its pending deletion.
26. A local repository matches a host repository on the same forge
    repository (kind, host, owner, name). One on the same base branch is
    taken first, else the first one left; each host repository matches once.
    For each match the daemon sends `PUT /v1/repositories/{id}` with the
    local `default_workflow`, `description` (empty for none),
    `permission_mode`, and `base_branch` and `forge.enabled` where they
    differ. The host keeps its own `path`. A local repository with no match
    is in `missing_repositories`. Nothing is registered on the host, and a
    missing checkout leaves `state` as it is.
27. A learned permission is pushed with `POST /v1/permissions/learned` on
    the host repository its repository matches (rule 26). The route writes
    the whole row; a row with the same repository, tool name, level and key
    keeps its id and `created_at`. An unknown repository is a 404. A deleted
    row deletes the host row with the same repository, tool name, level and
    key. A row whose repository has no match is not pushed: it is counted in
    `skipped_permissions`, recounted on each push of a repository or a
    learned permission, and leaves `state` as it is.
28. The AI permission settings are pushed with `PUT /v1/permissions/ai`:
    `enabled`, the thresholds where they are set by hand (else
    `default_thresholds`), and `flavour`. A sync never sends `device`. A
    status event that changes none of these pushes nothing. A flavour equal
    to the one the host stores keeps the host's device (spec 022, rule 19), so
    a device set on one host with `permissions ai set --device --host NAME`
    (spec 014, rule 32) persists through every push, including the full push
    when the host becomes `ready` again. It lasts until `local` changes the
    flavour: a different flavour makes the host pick its best device.
29. Writes go to the local daemon. Nothing is read back from a host into
    this daemon.

43. Skill pushes include the registered document and a complete supporting-file snapshot.
    Paths stay within the destination skill directory. Nested paths and executable bits survive transfer.
    Matching names take source contents; worker-only skills remain unchanged.
    The daemon checks supporting files at each `host_check`, including file removals.
    A transfer rechecks its source before it reports success.
    Scans skip failed skills; only the backoff of rule 25 retries them.
    A remote rejection of the `files` field reports an incompatible skill API,
    names the host and skill, and instructs the user to upgrade the remote daemon.
    Complete transfers remain mandatory and retries recover when the daemon is upgraded.
44. Escaping symbolic links fail that skill push and name the offending path.
    Other skills continue. Failed staging or replacement preserves recoverable previous worker contents.
    Explicit Markdown local-file links outside the skill directory produce warnings in `sync.error`.
    Such warnings leave successful transfers `in_sync`; web links produce no warning.
    External files are not transferred. Prose and shell examples are not dependency detection inputs.

### SSH worker setup

45. `POST /v1/hosts/{name}/setup` starts or retries setup and returns `202` with `SetupDto`.
    `GET` on the same path returns progress, or `404` for an unknown operation.
    Reserved or invalid names and invalid requests return `400`.
46. A request contains `ssh`, optional absolute socket `remote`, and optional `source`.
    An omitted source defaults to `{ "kind": "daemon" }`, the version of the settings daemon.
    A release daemon resolves this kind to its recorded repository and its own version tag.
    A source daemon builds its recorded commit on the worker, as rule 46a states.
    Inspection includes `binaries: {method, version, tag, commit, uncommitted}` for the inspected machine.
    The method is `release` or `source_build`; tag and commit can be null.
    Review has one Ariadne programs item that names the method and version.
    SSH-only request files work with both `ariadne host review` and `ariadne host setup`.
    SSH uses `ssh://[user@]host[:port]`, BatchMode, and the daemon user's existing SSH authentication.
    Passwords in URLs are refused. No credential values are accepted or returned.
    A local source contains `kind: local`, `cli`, and `daemon`.
    Both paths refer to files on the source daemon machine that runs the settings daemon.
    A release source contains `kind: release`, `repository`, and an explicit `tag`.
    Optional `tools` selects shipped registry agent IDs and GitHub tooling.
    Optional `credentials` contains source references only. Repository installation remains outside this operation.
46a. A source build packs its build commit with `git archive` from the recorded checkout on this machine.
    It sends the archive through SSH stdin to `~/.ariadne/worker-source` on the worker.
    Uncommitted edits and untracked files are never sent. No Git remote is cloned on the worker.
    Setup and update refuse in `validate`, before any transfer, when the checkout or the commit is missing.
    The reason names the missing checkout or commit. Its manual action carries that reason.
    When the daemon's build had uncommitted edits, review says that the worker gets the commit only.
    The steps after `validate` are `prerequisite:compiler`, `prerequisite:curl`, `source`, `rust`, and `build`.
    They replace `transfer`. Each step has a title.
    `prerequisite:compiler` compiles and links a small C program with `cc` and the standard C headers.
    The program uses a temporary directory, which the check removes. On macOS it also needs the Xcode developer directory.
    A missing compiler, linker, C library headers, or startup files make this check fail.
    `prerequisite:curl` needs `curl`, a `rustup`, or a Cargo that runs.
    A missing prerequisite is manual work. Setup records it, continues its independent selections, and stays `incomplete`.
    Resume checks the prerequisite again and continues. Setup runs no administrator command.
    The manual command is `xcode-select --install` on macOS.
    On Linux it uses the first of `apt-get`, `dnf`, `yum`, `zypper`, `pacman`, and `apk` on the worker, else null.
    The Linux compiler packages include the C library headers: `build-essential`, `gcc glibc-devel`, `base-devel`, or `build-base`.
    Inspection, the prerequisite check, and `rust` treat Cargo as present only when `cargo --version` succeeds.
    They use the build PATH, with `~/.cargo/bin` first.
    `rust` reuses a Cargo that runs. A `rustup` without a toolchain installs and selects the stable toolchain.
    Otherwise, `rust` runs the rustup installer with `-y --no-modify-path --profile minimal` in the user's home.
    `build` runs `cargo build --release --locked --bin ariadne --bin ariadned` in the source.
    `CARGO_TARGET_DIR` is `~/.ariadne/worker-build`, and `ARIADNE_SOURCE_COMMIT` names the commit.
    The build writes its output to `build.log` in that directory.
    A completed build of the same commit is reused on retry, and is not built again.
    `install` installs the built programs as it installs transferred programs.
    `Timeouts::worker_source_build` bounds the source transfer, the Rust installation, and the build.
    The worker needs network access to crates.io, and to rustup when Rust is missing.
47. Setup probes the remote OS, architecture, and home before transfer.
    Setup finds tools that the user installed by hand on the PATH of rule 48.
    A runtime is present when its command runs: `node --version`, `npm --version`, `uv --version`.
    A registry agent with an `npx` or `uvx` distribution is present when its registry command runs on that PATH (`<command> --version` with stdin from `/dev/null`), such as `claude-agent-acp`.
    The registry command is the executable that the agent's package installs, from the `bin` of its npm package or the console script of its Python package.
    `plan.rs` lists it for each agent, since the registry does not name it and it is not the package name: `@google/gemini-cli` installs `gemini`.
    A unit test requires an entry for each npm or uv agent of the shipped registry.
    An npm package of the registry version in `~/.local`, or a `uv tool` of the registry version, also counts.
    A provider CLI is present when its command runs on that PATH, as above: `claude` for `claude-acp`, `codex` for `codex-acp`.
    An npm package in `~/.local` also counts.
    A command that exists but does not run is missing.
    A present item is reused, is not installed again, and shows in `already_present`; a missing item is installed.
    An update upgrades the npm and uv packages that setup installed, and leaves an agent or provider CLI that the user installed by hand.
    An update upgrades a runtime, Git or GitHub CLI with user-owned Homebrew only where Homebrew installed it.
    An update reuses one that runs and that Homebrew does not own, with or without Homebrew, and installs none beside it.
    After an update upgrades or installs it, the command must run on the PATH, whatever Homebrew reported; if it does not, the step names the missing runtime.
    A failed agent step names what is missing: Node.js and npm, uv, the ACP package, or the provider CLI.
    The runtime presence check is one shell function, `runtime`, in `setup/populate.rs`.
    Setup installs a missing Node.js or uv in this order, for an agent step that needs it.
    First, a user-owned Homebrew installs it, as before.
    Without one, setup installs it into the worker user's home, with no administrator command.
    For Node.js, setup downloads the current LTS archive for the worker's OS and architecture from `https://nodejs.org/dist`.
    It checks the archive against the `SHASUMS256.txt` of that release, and extracts it under `~/.local`, so `node`, `npm` and `npx` are in `~/.local/bin`.
    A failed download, a missing checksum line or a checksum mismatch installs nothing, and the step error names the cause.
    For uv, setup downloads the standalone installer from `https://astral.sh/uv/install.sh` and runs it with `UV_UNMANAGED_INSTALL=~/.local/bin`.
    `Timeouts::worker_runtime_install` bounds each runtime download and install. The bound ends the SSH command. The install script runs each download in a process group of its own and ends it when the SSH connection ends, so no download outlives its step. The script also ends it 120 seconds after that bound, in case it never noticed.
    The runtime supervisor uses `/bin/bash` for noninteractive job control, including when the worker login shell is `sh`.
    A worker without executable `/bin/bash` gets a Bash prerequisite error before any download starts.
    The error tells the user to install Bash at that path and resume setup.
    Completed runtime work stops and reaps its watchdogs before the step returns.
    The worker needs `/bin/bash`, `curl` and, for Node.js, `tar`, and network access to nodejs.org for Node.js and to astral.sh for uv.
    A runtime that setup installed this way shows as installed by Ariadne; a runtime that was present shows as already present.
    Setup records it in `~/.local/share/ariadne/installed-<command>`. An update upgrades a runtime with that record, and leaves one without it.
    A failed runtime install shows a manual action that names the network access and the manual install command.
    macOS and Linux support ARM64 and x86_64.
    Both local binaries must have matching executable headers before either transfers.
    macOS accepts thin binaries and universal binaries with a compatible executable slice.
    Release assets use `ariadne-<target>.tar.gz`.
    GitHub CLI 2.102.0 or later verifies the selected repository's release workflow and exact tag before extraction.
48. Setup installs binaries under the remote user's `~/.local/bin`.
    It registers `dev.ariadne.daemon` with launchd or `ariadned.service` with systemd user services.
    New Linux service definitions preserve relay processes on daemon stop (016, rule 12).
    New service definitions use the PATH of the remote user's terminal, with `~/.local/bin` first.
    Setup reads it from `"$SHELL" -ilc`, an interactive login shell that reads stdin from `/dev/null`.
    Setup uses only PATH text between unique probe markers, ignoring login-profile banners, job-control warnings and other output.
    If the shell fails, markers are missing, or the marked PATH is empty, setup reads the login-shell PATH (`-lc`) in the same way.
    If that also fails, setup uses the SSH session's PATH.
    Inspection, the setup steps and the service definition all use that one PATH.
    Setup rejects PATH values with control characters before transfer, including newline and NUL.
    Systemd values escape backslashes, quotes, and percent specifiers; dollar signs stay literal in `Environment=`.
    Launchd stores the XML-escaped PATH in its `EnvironmentVariables` dictionary.
    Existing service definitions, including PATH and machine-specific settings, remain unchanged through retries.
    Configuration and worker data remain intact.
    Inspection reports missing Git, tar, user services, and Linux lingering without administrator commands.
    An existing custom socket must be supplied through `remote`.
49. Progress states are `pending`, `running`, `incomplete`, `failed`, and `complete`.
    Base steps are probe, validate, transfer, install, service, health, and register.
    A source build replaces transfer with the steps of rule 46a.
    Selected tools, credential transfers, and authentication checks have individual results.
    Failure retains completed steps and records the failing step's error.
    No successful step means `failed`; otherwise failure means `incomplete`.
    Completion requires a successful daemon status check and local connection registration.
    Before the service step restarts the user service, it stops a daemon that answers on the worker socket
    but is not the service's own process: SIGTERM, then SIGKILL after 30 seconds.
    The service's process is systemd's `MainPID` or the pid in `launchctl print`.
    The health step passes only when the daemon that answers reports the service's own pid (016, rule 21).
    Another pid, a service with no process, or a daemon that reports no pid fails the step and names the pids.
    The user can skip a failed external-software step of a stopped setup or update.
    `POST /v1/hosts/{name}/setup/steps/{step}/skip` marks the step `skipped` and returns the full `SetupDto`.
    The route acts on an update of the machine that is not `complete`, else on its setup.
    These steps can be skipped: `tool:github`, `tool:<agent>`, `credential:<id>`, and `authentication:<id>`.
    Any other step, or a step that has not failed, returns `400`. An unknown setup or step returns `404`.
    A `pending` or `running` setup returns `409`. All refusals use the daemon's error envelope.
    A skipped step keeps its error and its manual action, and its `group` is `skipped`.
    Resume and update retries never run a skipped step again. This includes the authentication checks.
    Setup goes on to registration when its only failed steps are skipped, and then ends `complete`.
    No route undoes a skip. A new setup or a later update installs the item.
50. The settings daemon stores operations atomically in `worker-setup.json` under its home.
    Restart converts interrupted operations to `incomplete`. The register step writes the host into the database.
    Setup records hold operations only. Restart never restores connections from `worker-setup.json`.
    The same request resumes setup without transferring installed binaries again.
    Retry rechecks access and binaries, preserves completed package installations, restarts the service, and checks health.
    Concurrent repeats return the existing operation. Changed requests are refused while setup is incomplete.
    After completion, repository selections can change with the same SSH access and binary source.
    A complete request stays complete on repetition.
51. `ariadne host setup NAME --request FILE` starts or retries an operation.
    `ariadne host setup-status NAME` shows progress and per-step errors.
    Both use the local daemon; JSON output uses the operation DTO.
    A failed or incomplete result exits unsuccessfully.
52. The Desktop Hosts screen installs a worker through the install method of its "Add machine" dialog (spec 015, rule 52a).
    It sends no `source` unless the user fills local program paths or a verified release under "Advanced".
    Its Install step shows each step by its title and its failure, and polls active setup.
    "Check again" resumes a stopped setup with the same request (rule 78).

53. Tool selections install Git and optional `gh`, plus npm or uv distributions from the shipped ACP registry.
    User-configured commands override registry IDs and receive manual installation guidance.
    Unsupported IDs and distributions receive manual installation guidance without generated commands.
    npm uses the worker user's `.local` prefix. uv manages user tools and Python.
    Setup verifies npm packages with `npm list --global` in that prefix after installation and during retries.
    Missing Git and gh use existing user-owned Homebrew without administrator commands.
    Missing Node/npm or uv use it too, else the user-home install of rule 47.
    Without those installation paths, progress reports the prerequisite and remains incomplete.
54. Credential references accept `codex`, `github`, `claude`, `opencode`, and `ssh`.
    Unknown references are refused before persistence. Values are never request fields.
    Only selected `codex` and `github` transfers are supported.
    Codex uses the source native home's `.codex/auth.json`, transferred atomically with mode 0600.
    GitHub uses `gh auth token --hostname github.com` and remote `gh auth login --with-token` through SSH stdin.
    Command arguments, progress, persisted descriptions, and responses contain no transferred credential values.
    Other references report unsupported transfer and separate worker login.
    Successful transfers survive retries without another source read or write.
55. Selected GitHub tooling requires successful `gh auth status` and `gh auth setup-git` for github.com.
    Codex and Claude agents install their provider CLIs and require successful provider authentication status.
    Other agent authentication checks remain unsupported and leave setup incomplete with worker login guidance.
    Retries check required tools and authentication again. Daemon health alone does not complete selected tool setup.

56. `HostColor` has snake_case values `red`, `orange`, `amber`, `green`, `teal`, `blue`, `violet`, and `pink`.
    `HostDto.color` is the selected value or null. Null means no override; the daemon computes no default.
57. `POST /v1/hosts` accepts `{name, endpoint, remote?, color?}` and returns 201 with the full `HostDto`.
    The daemon persists and starts the host immediately. Invalid names or addresses return 400 with the field named.
    An existing name returns 409.
58. `PATCH /v1/hosts/{name}` accepts `{endpoint?, remote?, color?}` and returns 200 with the full `HostDto`.
    Present fields replace stored values. Null clears `remote` or `color`; omitted fields retain their values.
    Endpoint or remote changes stop the old connection and start its replacement.
    Colour changes retain the connection. For `local`, only `color` is accepted; other fields return 400.
59. `DELETE /v1/hosts/{name}` removes the database row and stops forwarding, health checks, and sync.
    It returns 204 with no body and leaves the remote daemon untouched. Deleting `local` returns 400.
60. PATCH and DELETE return 404 with code `unknown_host` for an unknown name.
61. The UI accepts host changes and deletions only from the `local` event stream.
    Deletion removes the cached host row, closes its stream through the host provider, and drops `qk.host(name)`.
    Host configuration and colours never enter catalog sync.
62. `ariadne host add`, `edit` and `rm` call rules 57 to 59 on the local daemon
    and take `--color` from the eight `HostColor` values (014, rules 52 to 57).


63. Repository population extends setup with an optional `repositories` map from source IDs to worker destinations.
    Destinations accept absolute paths or `~/` paths resolved against the probed worker home.
    Desktop proposes `~/workspace/<repository>` and permits each destination to change.
    Setup clones Git remote history without source edits or untracked files.
    Registered checkout reuse follows rule 26, including base branch preference and preservation of worker paths.
    Each selection records `clone:<id>` and `repository:<id>` progress.
    Their titles name the repository as Desktop does, by its forge name or its folder, not by its id.
    Review names it the same way.
    For an SSH remote, setup copies the `known_hosts` lines that this machine's `ssh` trusts for the Git host
    to the worker's `~/.ssh/known_hosts`, unless the worker already knows that host.
    Appending keys preserves existing entries, including a last line without a newline.
    Host-key errors do not assume that this machine has no trusted key.
    Missing remotes, worker authentication failures, and inaccessible destinations leave setup incomplete.
    A failed clone names its cause: an existing destination, missing Git, an untrusted host key with the command
    that checks it, refused authentication, or a missing branch. It names the remote's host, never its URL or Git's output.
    Setup refuses unrelated existing destination directories.
    Completed clone and registration checkpoints survive retries.
    Registration invokes the normal host synchronization for settings and learned permissions.
    Automatic enrollment of later repositories and movement of existing checkouts remain outside scope.

### Portable daemon configuration

64. Each ready host receives portable daemon settings through `PUT /v1/config/sync`.
    `GET` on that path returns `ConfigSyncDto`, with `state`, `error`, and `configuration`.
    The portable fields are `delete_merged_branches`, `delete_merged_worktrees`, `auto_switch`,
    `exhausted_patterns`, `log_filter`, the three `telemetry_*_days` values, `acp_registry_url`,
    `acp_agents`, and explicitly stored `agent_flags`.
    Source defaults participate. Worker-only agents and flag entries remain.
    Paths, listeners, origins, webhook addresses, hardware choices, sleep prevention,
    executable locations, hosts, and other settings remain the worker's.
65. Custom agents match by identifier. An existing worker command takes precedence over the source command.
    Without a mapping, the command must use an executable on the worker PATH.
    Explicit filesystem paths in commands or arguments require a worker mapping and otherwise fail synchronization.
    These are absolute paths, `~/`, `./`, `../`, and Windows paths.
    Other slash tokens remain portable, including provider/model identifiers and scoped package names.
    Path-bearing flags fail instead of silently installing unusable source paths.
    Matching portable flag lists take the source list. Worker-only flag lists remain.
66. The worker saves accepted settings in `portable-config.json` under its own home.
    Startup overlays these settings without rewriting `config.toml` or changing worker locations.
    Worker commands configured in `config.toml` retain precedence by identifier.
    Cleanup policies, model switching, logging, and retention update live.
    Existing retention jobs use the updated cutoffs on their next pass.
    `RUST_LOG` keeps precedence over the synchronized logging filter.
    Registry additions and index changes require an idle restart.
67. Maintenance snapshots admitted tasks under the same lock that guards admission.
    Admitted tasks continue through all remaining columns, including review and CI waits.
    New tasks remain queued. New review and babysit operations receive a maintenance conflict.
    Existing operations and their coordinating sessions can continue.
    The worker waits for admitted tasks and active turns before replacing its process.
    Process replacement supports directly launched macOS and Linux workers.
    It preserves the PID, inherited environment, and service supervision for installed workers.
    Open event streams do not delay replacement.
    SSH forward guards close their process groups when replacement closes the daemon lifetime pipe.
68. Host synchronization reports `pending_restart` while registry changes wait for maintenance.
    Successful replacement restores normal scheduling and reports `in_sync`.
    A replacement failure reports `failed` with its reason and restores admission.
    A later configuration transfer retries maintenance. CLI and desktop expose the state and error.
69. Host checks detect source configuration and explicit agent-flag changes.
    Configuration failures use the existing independent retry queue from rule 25.
    They do not block catalog, repository, learned permission, or AI permission synchronization.

### Explicit worker updates

70. POST /v1/hosts/{name}/update starts or retries an explicit update and returns 202 with SetupDto.
    GET returns its durable progress, or 404 for an unknown update.
    UpdateRequest accepts optional source and tools selections. At least one section is required.
    Binary-only, tools-only, and combined updates are supported.
    An omitted source updates no programs. An explicit `daemon` source selects this daemon's version.
    The worker name selects its saved SSH address and socket. Requests accept no SSH overrides.
    Credentials and repositories are refused. Setup records and update records remain separate.
    Active repeats return the existing operation. Changed selections are refused while running or pending.
    Failed or incomplete updates accept revised selections.
    Unchanged work retains its completed checkpoints. A changed binary source invalidates binary installation checkpoints.
    Removed tool steps disappear. New tool steps start incomplete.
    A new request after completion starts another update.
71. Updates reuse binary compatibility checks, release verification, installation, service restart, and health checks from setup.
    An explicit `daemon` source on a source daemon uses the build of rule 46a, before maintenance.
    Before installation, the worker closes admission and waits for admitted tasks and active sessions.
    New tasks stay queued. Existing tasks can finish every column.
    GET /v1/maintenance reports the explicit hold and drain status.
    POST /v1/maintenance with active true acquires this hold and reports ready after draining.
    The internal CLI daemon maintenance command reads status. Its --active true or false option changes the hold.
    Updates use the worker CLI for these requests, without curl.
    The hold survives worker restart in worker-maintenance.json.
    Automatic configuration synchronization cannot release the hold or upgrade programs.
72. Selected tools upgrade only during an explicit update.
    User-owned Homebrew upgrades selected Git, GitHub CLI, and required Node or uv runtimes.
    npm reinstalls selected registry packages and provider CLIs in the user prefix.
    uv explicitly upgrades installed selected tools within their registry version constraints and provides Python.
    Unsupported tools and unavailable user-owned prerequisites report per-step failures without administrator commands.
    Successful tool steps survive retries without another upgrade.
73. After successful service restart and health checks, the update releases maintenance and restores scheduling.
    Failures preserve completed steps and report the primary error.
    After failure, the controller checks worker health and releases maintenance when the worker is healthy.
    Release preserves any pending configuration drain and its admitted tasks.
    Failed health checks or release requests retain maintenance and report a separate resume-step error.
    Retries reacquire maintenance before changing installed programs.
    The current worker must answer maintenance status before any binary transfer.
    Staged replacement CLI binaries must support daemon maintenance.
    Staged replacement daemons must report protocol 1 with --maintenance-protocol.
    These capability checks finish before maintenance or installation.
    An unsupported current worker or replacement receives guidance while installed binaries remain unchanged.
74. CLI commands host update NAME --request FILE and host update-status NAME use the local daemon.
    Each host row of the Desktop Hosts screen with a saved setup offers "Update", with independent program and tool selections (spec 015, rule 52b).
    It uses saved SSH settings and permits revised selections after failure.
    Both interfaces show individual steps and retained failures.

### Inspection, review, and resume

75. `POST /v1/hosts/{name}/setup/inspect` accepts only an SSH address and changes no worker files.
    Inspection requires an account with configured noninteractive SSH authentication and host trust.
    It reports supported platform targets, command availability, credential-file presence, and user-service prerequisites.
    Provider authentication remains unverified until application because provider commands can create files.
    Each check includes an identifier, satisfaction state, missing capability, resolution, and recheck instructions.
    Supported transfers remain Codex file credentials and GitHub tokens.
76. `POST /v1/hosts/{name}/setup/review` accepts the existing setup request and starts no durable operation.
    Review uses the same inspection result as CLI and Desktop.
    It lists installations, selected transfers, repository destinations, preserved machine settings, and prerequisites.
    Local executable compatibility appears before application. Release verification retains rule 47.
    Credential values and provider command output never enter inspection or review responses.
77. Setup continues independent selected tool and credential actions after prerequisite failures.
    It also continues independent repository selections after clone or registration failures.
    A failed selection keeps its error and prevents setup completion and initial host registration.
    Requests without tool selections check worker Git without installing it.
    Missing or unusable Git prevents initial registration; resume rechecks it after manual installation.
    Successful installation and credential checkpoints prevent repeated work.
    Authentication checks run again on retry, except a skipped one (rule 49). Explicit worker-update policy remains unchanged.
78. `POST /v1/hosts/{name}/setup/resume` retries the exact durable request without new selections.
    Existing setup POST and GET routes retain their request and progress formats.
    CLI provides `host inspect`, `host review`, and `host resume` through the local settings daemon.
    These commands reject other host selections with a worker setup reason and `--host local` guidance.
    Desktop requires inspection and review before application, and reviews again after a selection changes.
    Desktop resumes a stopped setup from the "Check again" of its Install step.

### Saved setups

79. `GET /v1/host-setups` lists each saved setup and update as `{ name, operation, ssh, state, steps, tools }`.
    `operation` is `setup` or `update`; `tools` is the saved selection or null.
    The list holds no credential reference or value. Clients poll it while an operation is `pending` or `running`.
80. `DELETE /v1/hosts/{name}/setup` deletes the saved record of a setup in state `incomplete` or `failed`, and returns `204`.
    It contacts no worker and leaves every worker file untouched.
    A setup in state `pending`, `running` or `complete` returns `409` with its state.
    An unknown name returns `404`. Update records are never discarded.
    A new setup with the same name then starts from its first step.
81. `ariadne host setups` prints the list, and `ariadne host discard NAME` discards one setup.
    Both use the local daemon, and JSON output uses the route DTOs.

### Setup groups

82. `SetupGroup` is `ariadne_installs`, `already_present`, `manual`, or `skipped`. It says who installs an item on the worker.
    Only a progress step is `skipped` (rule 49). A plan item never is.
    `ManualAction` is `{reason, command}`. `reason` is plain words. `command` is the exact worker command, or null.
    No manual action contains a credential value or provider command output.
83. `InspectionDto.agents` lists each shipped registry agent, then each agent that only this daemon's configuration names.
    Each entry has `id`, display `name`, `on_this_machine`, `group`, `manual`, and `sign_in`.
    `on_this_machine` is true when this daemon's agent registry reports the agent ready.
    An npm agent is `ariadne_installs` when the worker has Node.js and npm, or user-owned Homebrew.
    A uv agent is `ariadne_installs` when the worker has uv, or user-owned Homebrew.
    Either agent is also `ariadne_installs` when the worker has `curl`, for the user-home install of rule 47.
    An agent is `already_present` when setup would reuse its packages without installing them, or when its registry command and its provider CLI command run on the worker PATH (rule 47).
    For npm, the `.local` prefix holds the exact registry version, and any Codex or Claude CLI that setup adds.
    For uv, `uv tool list` shows the exact registry version. The probe prints only fixed package markers.
    Configured commands, binary distributions, and unknown agents are `manual`.
    `sign_in` names the `sign_ins` entry of the agent: `codex`, `claude`, `opencode`, or null.
84. `InspectionDto.github` is `{group, manual}`. GitHub CLI on the worker is `already_present`.
    Without it, user-owned Homebrew makes it `ariadne_installs`; otherwise it is `manual`.
85. `InspectionDto.sign_ins` lists `codex`, `github`, `claude`, `opencode`, and `ssh` with `name`, `copy_supported`, and `command`.
    Only `codex` and `github` support copying (rule 54). `command` signs in on the worker, or is null for `ssh`.
86. `SetupReviewDto.plan` has one `{id, title, group, detail, manual}` item for each thing that the request causes.
    The order is the setup order: `install`, the source build items, Git, GitHub CLI, agents, credentials,
    authentication checks, repositories, `service`, and `register`. Each `id` is the name of the progress step that does the item.
    The source build items are `prerequisite:compiler`, `prerequisite:curl`, `source`, `rust`, and `build` (rule 46a).
    Rust is `already_present` when the worker has `cargo`, else `ariadne_installs`.
    A missing compiler, linker, or curl is `manual`, with the command of rule 46a.
    Git is `prerequisite:git` without tool selections and `tool:git` with them.
    `manual` is set exactly for items in the `manual` group.
87. `SetupStep.title` is a plain-words title for every step. `SetupStep.manual` is set when a failed step needs manual work.
    `SetupStep.skipped` is true for a step that the user skipped (rule 49). Its `group` is then `skipped`.
    Otherwise `SetupStep.group` is `manual` when `manual` is set. It is `already_present` for a complete step that installed nothing:
    a found Git, GitHub CLI, or agent package, and a sign-in check that no copy preceded. Otherwise it is `ariadne_installs`.
    Updates always install, so their steps are never `already_present`.
    It equals the `manual` of the plan item with the same id. `validate` uses the `install` item.
    A failed `validate` step of a release source has no manual action.
88. All new fields are additive. Existing request formats are unchanged.
    The CLI reads responses without the new fields. A step without `group` is `manual` when it has `manual`.
    Stored operations without the new fields gain titles and groups when read.
89. `host inspect`, `host review`, and `host setup-status` print items under "Ariadne installs", "Already present", and "Manual".
    Each manual item prints its reason and its command. `--format json` prints the DTO.

### Far tasks

95. `POST /v1/goals/{id}/tasks` with a `host` other than `local` (001 rule 17) checks the host
    before it creates the far task:
    - An unknown host answers `400`, and the message names it as unknown.
    - A host that is not `ready` answers `409`, and the message names the host status.
    - A host with no checkout of the task's repository by rule 26 answers `409`. The message
      names the repository by its forge name or folder, the host, and the setup step that
      adds the repository: `repository:<id>` of `ariadne host setup <host>` (rule 63).
    - A model that the host's `GET /v1/models` does not list answers `400`. The message
      names the model and the host. The local catalog does not decide the models of a far task.
    - A model that the host lists with `enabled` false answers `400` for the orchestrator,
      and the message names the model and the host. It stays the user's choice (011 rule 19):
      a request with no session header takes it.
    - A host that does not answer these reads answers `502`.
96. Existing legacy mirrors retain their local execution records and linked goals.
    The goal's daemon keeps each legacy far task in its own task list. The store keeps its host, the
    id of the linked goal on the host and the id of the task on the host. The goal's daemon
    never starts an agent for a far task.
97. A legacy far task is free to start when it is `pending`, it has not gone to its host, its goal is
    `active`, and every task in its `depends_on` has merged (001 rule 19). Then the goal's
    daemon sends it to its host. A send whose answer was lost makes no second goal or task
    when it is sent again (001 rules 14 and 15):
    - `GET /v1/tasks?origin_task_id=<id>` asks first what the host made of the far task, in
      whatever goal and status. A task found is recorded, with the goal it is in, and the
      send ends.
    - `POST /v1/goals` with `linked: { origin_goal_id }`, the goal's workflow, title,
      description and issue URL, and the host's id of each goal repository that the host has,
      the task's own first. The model is the one of the task's first agent. The host has one
      linked goal for the goal, and a repository set up on the host since the first send
      joins it.
    - `POST /v1/goals/{id}/tasks` in that linked goal, with no session header, the host's
      repository id, the same title, description and agents, no `depends_on`, and the far
      task's own id as `origin_task_id`. The goal's daemon records the task the host
      answers, with the goal it is in.
    - A refusal of the host fails the far task with the reason
      `host <name> refused the task: <message>`. A linked goal that ended on the host is
      such a refusal, also when it ends between the two creates (001 rules 14 and 15). A host that does not answer gets the task again after
      `far_retry`.
    - Each of these requests runs only while the host stays in the `ready` the send started
      in. A host that leaves it ends the send at once: the request is dropped, nothing that
      it answers is written, and the far task is not failed. The send starts again on the
      host's next `ready`.
98. For legacy mirrors, the goal's daemon follows `GET /v1/events/stream` of each `ready` host. Each
    `task_updated` of a far task writes its status, step, branch, request URL and merge
    commit on the far task. A changed status writes a transition by the daemon, with the
    host's reason. A far task that the host reports `finished` has merged. A far task whose
    state is final here is never written: one that is `finished` or `cancelled`, and one whose
    goal is over (001 rules 3 and 4).
99. When the host leaves `ready`, each far task on it keeps its last known state, and
    `host_unreachable` is true, whatever the status of its goal. A send, a read or the open
    of the event stream under way does not hold this back: each call to the host runs only
    while the host stays in its `ready` (rules 97 and 100). Each call waits `far_call` at most
    for the headers of its answer. A lost event stream of a
    `ready` host is the same, and the goal's daemon opens the stream again after `far_retry`.
100. On the next `ready`, the goal's daemon opens the event stream of the host. Then it reads
    again each far task that went to the host, whatever its state and its goal
    (`GET /v1/tasks/{id}` on the host), and sets `host_unreachable` to false on each far task
    of the host. A far task whose state is final keeps it (rule 98). A host that leaves
    `ready` and comes back at the same endpoint while the daemon still works on an earlier
    answer of it counts as a next `ready` too. An answer that arrives after the host left
    `ready` writes nothing, and clears no `host_unreachable`.

101. `GET /v1/remote-tasks/protocol` explicitly publishes `remote-tasks-v1` support and the worker's durable daemon ID.
     Connection aliases and tunnel addresses do not identify execution ownership.
     A database retains its daemon ID across restarts.
     New remote execution uses the worker draft protocol (012), independently of legacy linked goals.
     Authorized execution requires no source connection.

### Source references for new tasks

102. New remote creation uses the worker protocol from rule 101 during source planning.
     The source requires the source-dependency-controls-v1 capability before reserving a stable task ID.
     Incompatible workers receive an explicit upgrade instruction. No legacy fallback creates duplicate records.
     The source reserves its reference and creation request before the first worker write.
     A lost response replays the same ID and request. Acknowledgment removes the creation request.
     The worker owns the task and staffing. The source creates no task or staffing rows.
     The source retains a disposable worker snapshot and source dependency and control records.
103. Source authorization requires an active source goal and completed dependencies on every host.
     Dependency validation and cycle detection include local tasks and source references.
     A durable authorization claim prevents dependency changes after admission begins.
     Authorization names the worker's current draft revision. Repeated authorization never restarts execution.
104. A failed dependency queues a repeatable failure request for an unauthorized worker draft revision.
     The worker owns the failure, reason, and audit under the dependency control contract in 012.
     Source status remains the last acknowledged worker status until that operation resolves.
     Explicit source retry uses the worker dependency retry operation on the same task identity.
     Worker execution failures retain their existing retry path. Queued retries wait for completed dependencies.
105. Source task lists, details, and events include referenced worker tasks with their stable task IDs.
     Projections retain worker execution context, staffing IDs, usage, and acknowledged execution progress.
     They replace host aliases, repository IDs, and dependencies with source values.
     Session IDs use `far~TASK_ID~WORKER_SESSION_ID` at the source. Worker reads retain worker session IDs.
106. Source task controls, worker history, and diffs resolve the worker by its persistent daemon ID.
     Worker history includes authoritative dependency failure and recovery audits.
     Session aliases route session controls, console reads, streams, input, and terminal connections to the worker.
     A session must belong to its referenced task. Source sessions cannot control another source goal.
     Worker endpoints can change without changing task ownership. Another worker cannot replace the recorded owner.
107. Rules 99 and 100 also govern reference freshness. Stream loss marks cached source progress stale.
     Reconnection reads the worker before clearing `host_unreachable` in the current connection.
     Delayed responses from previous connections cannot refresh the source snapshot.
     Older worker snapshots cannot overwrite newer acknowledged progress.
108. Removed: source and worker agents store no message and relay none. Posts between them follow 039.
109. Source cancellation records intent until the worker acknowledges its authoritative outcome.
     A worker completion that wins cancellation remains finished at the source.
     Source goal cancellation requests worker cancellation. Goal completion requires terminal referenced tasks.
     Source deletion retains references and pending cancellation until worker actions resolve.
     Worker worktrees and branches receive no source cleanup operations.
110. Goal usage adds each referenced worker task's cached usage once.
     Worker sessions create no source session rows. Existing local and legacy usage remains unchanged.

111. `POST /v1/tasks` accepts a ready host for goal-less work, with or without a workflow.
     Existing host checks refuse unavailable hosts and name the cause.
     The source reserves a reference and authorizes its worker draft without goal planning.
     The worker saves no source goal. The protocol advertises `goal-less-tasks-v1`.
     Goal-less requests and execution contexts carry null `source_goal` and a separate `source_daemon_id`.
     An empty workflow name and empty columns represent no workflow in the remote protocol.
     The source reports worker status, step, and reason through the existing reference cache.
     Source cancel and retry use the existing worker controls. Goal tasks retain their rules.
     A goal-less reference wakes no goal reconciliation when its cache refreshes.

## Acceptance criteria

- Admission: `far_tasks::source_references::goal_less_admission_checks_the_host_and_saves_its_workflow`.
- Rule 111: `far_tasks::source_references::a_goal_less_far_task_has_no_source_goal_and_accepts_cancel_and_retry`.
  Worker columns and gates: `remote_tasks::goal_less_remote_columns_keep_their_gates_and_can_retry`.
  No goal wake: `far_tasks::source_references::a_goal_less_far_task_wakes_no_goal_reconciliation`.
  No workflow: `remote_tasks::a_remote_task_without_a_workflow_finishes_or_fails_at_its_agent_call`.

- Rules 102 and 105: `far_tasks::source_references::new_remote_tasks_are_worker_drafts_during_source_planning`.
- Rule 102 restart: `far_tasks::source_references::reopening_the_source_store_recovers_an_unacknowledged_creation_without_a_second_worker_task`.
- Rule 102 recovery: `far_tasks::source_references::a_lost_worker_creation_response_replays_one_task_identity`.
- Rule 102 capability negotiation: `far_tasks::source_references::a_worker_with_creation_only_v1_requires_a_dependency_control_upgrade`.
- Rule 102 compatibility: `far_tasks::source_references::incompatible_workers_require_an_upgrade_without_creating_records`.
- Rule 103: `far_tasks::source_references::source_dependencies_hold_worker_authorization_until_the_plan_and_dependencies_finish`.
- Rule 103 source edits: `far_tasks::source_references::source_dependency_edits_need_no_worker_connection_or_local_staffing`.
- Rule 103 revisions and cycles: `far_tasks::source_references::source_draft_edits_advance_worker_revision_and_reject_dependency_cycles`.
- Rule 104: `far_tasks::source_references::dependency_failure_and_retry_preserve_one_authoritative_worker_draft`.
- Rules 105, 106, and 110: `far_tasks::source_references::source_sessions_history_diffs_and_usage_resolve_the_worker_identity`.
- Rule 106 endpoint changes: `far_tasks::source_references::an_endpoint_change_preserves_the_worker_identity_and_task_reference`.
- Rule 106 ownership: `far_tasks::source_references::changing_the_host_endpoint_cannot_replace_the_recorded_execution_owner`.
- Rule 106 retired console routes: `far_tasks::source_references::the_retired_console_routes_of_a_worker_session_are_not_forwarded`.
- Rule 106 console routing: `far_tasks::source_references::source_trajectory_stream_and_terminal_connections_reach_the_worker_session`.
- Rule 107 session updates: `far_tasks::source_references::worker_session_creation_refreshes_source_progress_without_a_usage_report`.
- Rule 107: `far_tasks::source_references::stream_loss_marks_progress_stale_until_the_current_worker_is_reconciled`.
- Rule 109 completion: `far_tasks::source_references::goal_completion_includes_referenced_remote_work`.
- Rule 109 cleanup: `far_tasks::source_references::source_cleanup_preserves_local_branches_that_share_a_worker_task_name`.
- Rule 109: `far_tasks::source_references::source_cancellation_waits_for_worker_acknowledgment_after_reconnection`,
  `::a_finished_worker_outcome_wins_a_source_cancellation_race`, and
  `::deleting_a_source_goal_keeps_coordination_until_worker_cancellation_is_acknowledged`.


- Rule 101: `remote_tasks.rs::a_remote_draft_has_context_without_a_worker_goal` proves durable worker identity.
  `::authorization_starts_one_saved_workflow_without_a_source_connection` proves execution.


- Rule 95: `far_tasks.rs::a_far_task_is_refused_on_a_host_that_is_unknown_or_not_ready`,
  `::a_far_task_is_refused_on_a_host_that_lacks_its_repository`,
  `::a_far_task_is_refused_a_model_its_host_does_not_list` and
  `::a_model_its_host_turned_off_is_the_users_choice_alone`.
- Rules 96 and 97: `far_tasks.rs::one_linked_goal_on_a_host_takes_every_far_task_of_a_goal`
  proves that nothing goes to the host while the goal plans, and that two far tasks share one
  linked goal on the host's repository. `::a_far_task_waits_until_its_dependencies_merge_on_any_host`
  proves the wait for a far and a local dependency.
  `::a_lost_answer_of_the_host_makes_no_second_goal_or_task` proves the repeated send,
  `::a_task_the_host_made_is_found_again_after_its_linked_goal_ended` proves the question
  first, and `::a_repository_set_up_later_joins_the_same_linked_goal` proves the repository
  that joins. `::a_lost_goal_answer_after_its_goal_ended_fails_the_far_task` proves the refusal
  of an ended linked goal, and `::a_linked_goal_that_ends_between_the_two_creates_fails_the_far_task`
  proves the goal that ends between the creates. `::a_send_under_way_when_the_host_leaves_writes_nothing`
  proves the send that the host leaves, and the flag that does not wait for it.
  `::a_stream_that_opens_when_the_host_leaves_does_not_hold_the_flag` proves the same for the open
  of the event stream.
- Rule 98: `far_tasks.rs::a_far_task_mirrors_the_state_its_host_reports` proves the column,
  branch, request URL, merge commit and status.
  `::a_far_task_cancelled_here_stays_cancelled_whatever_its_host_reports` proves the final state,
  live and on the read after a return.
- Rules 99 and 100: `far_tasks.rs::a_far_task_keeps_its_state_while_its_host_is_away` proves the
  kept state, `host_unreachable`, and the read on the next `ready`.
  `::a_far_task_of_a_completed_goal_is_unreachable_while_its_host_is_away` proves the flag on a
  goal that is over, the read of its final task and the kept state.
  `::a_far_task_cancelled_here_stays_cancelled_whatever_its_host_reports` proves the read of a
  cancelled task. `::a_host_back_at_the_same_endpoint_is_read_again_while_its_worker_is_busy`
  proves the return at the same endpoint, and
  `::a_read_that_answers_after_the_host_left_writes_nothing` proves the late answer.

### Discovery by the orchestrator

90. The orchestrator seat has the MCP tool `list_hosts`. It takes no parameter and gives, for each host in
    the order of `GET /v1/hosts`, its `name`, `status`, `sync.state` and `sync.missing_repositories` (013).
    No other seat has it. The orchestrator seat's `orchestration` supplement (006 rule 21) tells the orchestrator
    to call it once for the goal, and to run every task on this host until `create_task` takes a `host`.
91. `GET /v1/hosts/{host}/models` answers the model catalog of that host, as `GET /v1/models` does.
    `local` answers this daemon's catalog. A `ready` host answers its own daemon's catalog.
    An unknown name answers `404`. A host that is not `ready` answers `503`, and the message names its status.
    The MCP tool `list_models` with `host` reads this route, and the `orchestration` supplement tells the
    orchestrator to call it with no `host` and to staff a task only on a model of this host.
92. [The manual](../docs/hosts.md) tells users how a goal sends tasks to other hosts.

## Acceptance criteria

- Rule 90: `crates/ariadne-cli/src/commands/mcp/tools.rs::the_hosts_and_the_models_of_a_host_reach_the_orchestrator`
  proves the fields and the order, and `mcp.rs::every_seat_has_the_tools_its_playbook_names_and_no_others` proves the seat.
  `crates/ariadne-store/src/skill_tests/operate.rs::the_orchestration_skill_picks_a_host_for_each_task_and_staffs_its_models` proves the supplement text.
- Rule 91: `host_queries.rs::a_host_serves_its_models_and_an_unknown_or_unready_one_is_refused` proves the three answers.
  `tools.rs::the_hosts_and_the_models_of_a_host_reach_the_orchestrator` proves the tool reads the route.
- Rule 92: `docs/hosts.md` is the page.

- Rule 46: `hosts.rs::daemon_version_installs_the_recorded_release_of_a_release_daemon` proves SSH-only review and setup.
  Release builds verify their own repository and version tag before installation.
  `hosts.rs::release_setup_requires_workflow_and_tag_verification_before_transfer` preserves explicit release checks.
  `hosts.rs::tools_only_updates_preserve_worker_binaries` proves that omitted update sources preserve programs.
- Rule 46a: `hosts.rs::a_source_build_daemon_builds_its_commit_on_the_new_machine` proves the packed commit,
  the plan groups, rustup in the home, the release build, the titled steps, and no administrator command.
  `hosts.rs::a_source_build_is_refused_before_transfer_without_its_checkout_or_commit` proves both refusals.
  `hosts.rs::a_missing_compiler_is_manual_and_a_retry_does_not_build_the_same_commit_again` proves
  the manual compiler, the `incomplete` state, the resume, and the reused build.
  `hosts.rs::an_update_from_a_source_build_daemon_builds_before_it_holds_maintenance` proves rule 71's build path.
  `hosts.rs::a_compiler_that_cannot_build_with_the_c_headers_is_a_manual_item` proves the compile and link check.
  `hosts.rs::an_unusable_cargo_without_curl_needs_curl_and_a_resume_installs_rust` proves the Cargo check,
  the manual curl item, and the resume.


- Rules 79 and 80: hosts.rs::saved_setups_are_listed_without_credentials_and_only_a_stopped_one_is_discarded proves the list, the refusals, the untouched worker and the fresh restart.
- Rule 81: crates/ariadne-cli/tests/hosts.rs::host_setups_lists_saved_operations_and_host_discard_deletes_one proves both commands.

- Rules 70 and 73: hosts.rs::unsupported_tools_can_be_removed_on_retry_without_reinstalling_binaries and
  hosts.rs::invalid_binary_sources_can_change_on_retry prove both review recovery cases.
- Rule 70: hosts.rs::revised_binary_sources_invalidate_the_install_checkpoint proves source-dependent checkpoint invalidation.
- Rule 72: hosts.rs::explicit_updates_upgrade_already_installed_uv_tools proves installed uv tools receive an explicit upgrade.
- Rule 70: hosts.rs::tools_only_updates_preserve_worker_binaries proves independent tool selection.
- Rules 70 and 74: hosts.rs::updates_use_saved_ssh_and_allow_binary_only_selection proves saved SSH and binary-only updates.
- Rule 73: hosts.rs::failed_updates_keep_maintenance_when_worker_health_fails proves failed health recovery.
- Rule 49: hosts.rs::an_update_stops_a_daemon_outside_the_service_before_it_restarts_the_service proves the stray stop.
- Rule 49: hosts.rs::an_update_fails_its_health_check_while_a_daemon_outside_the_service_answers proves the pid check.
- Rule 49: `ariadne-cli` hosts.rs::daemon_status_gives_the_pid_of_the_daemon_that_answered proves the status pid.
- Rule 73: hosts.rs::unsupported_current_workers_fail_before_binary_transfer and
  hosts.rs::unsupported_replacements_fail_before_installation_or_maintenance prove compatibility refusal.
- Rule 71: crates/ariadne-cli/tests/hosts.rs::maintenance_cli_reads_and_changes_the_selected_worker_hold proves maintenance without curl.
- Rule 74: host-setups.test.tsx::sends tools alone when the programs are not selected, and says why it waits when nothing is proves desktop independence.
- Rules 70–73: hosts.rs::updates_wait_for_idle_and_retry_without_replacing_installed_binaries proves idle waiting, selected upgrades, checkpoints, failure, and retry.
- Rules 71–73: portable_config.rs::explicit_updates_hold_new_tasks_until_maintenance_ends proves admission and continuing tasks.
- Rule 71: portable_config.rs::explicit_maintenance_survives_restart_and_restores_admission proves the durable hold.
- Rules 71 and 73: portable_config.rs::releasing_update_maintenance_preserves_a_pending_configuration_drain proves safe release during configuration changes.
- Rule 74: crates/ariadne-cli/tests/hosts.rs::worker_update_and_status_show_each_completed_step_and_failure proves command routing and progress.
- Rule 74: host-setups.test.tsx::updates a machine with no name or SSH input, the programs of this machine and the saved tools selected proves desktop selection.

- Rule 63: `hosts.rs::setup_accepts_selected_repository_destinations` and
  `hosts.rs::selected_repositories_clone_only_committed_files_and_retry_registration`;
  `hosts.rs::setup_rejects_unsafe_repository_destinations_before_starting` proves request validation.
  `hosts.rs::repository_setup_preserves_an_unrelated_existing_destination` and `::repository_setup_preserves_an_empty_existing_destination` prove preservation of unrelated directories, including empty directories.
  `hosts.rs::repository_setup_reports_a_missing_source_remote` proves the missing-remote step failure.
  `hosts.rs::repository_setup_reports_worker_git_authentication_failure` proves failure after a worker receives an HTTP authentication challenge.
  `hosts.rs::repository_setup_copies_this_machines_host_key_to_the_worker` proves the host key copy, the untrusted-key failure, the retry and the repository name in step titles.
  `hosts/setup/repositories.rs::tests` proves which remotes are SSH, the `known_hosts` lookup name, and that a failure never shows URL credentials.
  `add-machine.test.tsx` proves selection and destination editing.
  `hosts.rs::worker_setup_and_status_show_each_completed_step_and_failure` proves CLI selection and progress.

- Rules 1 and 56, persistence: `crates/ariadne-store/tests/store.rs::host_rows_survive_reopening_and_keep_local_color`.
- Rules 1–3, 6, and 56–60: `hosts.rs::host_routes_persist_nullable_overrides_and_publish_full_rows`,
  `::host_routes_refuse_invalid_fields_duplicates_and_unknown_names`,
  `::local_rejects_an_endpoint_even_when_null`.
- Rule 1, configuration: `hosts.rs::a_bad_host_is_refused_by_name_at_startup_and_by_check_config`,
  `::even_an_empty_hosts_table_is_refused`.
- Rules 4 and 5: `hosts.rs::a_daemon_without_hosts_lists_only_local`,
  `::stored_hosts_follow_local_in_insertion_order`,
  `::reconnecting_the_first_host_preserves_database_insertion_order`.
- Rules 58 and 59: `host_health.rs::editing_and_deleting_a_host_stop_its_old_health_checks`,
  `ssh_forward.rs::changing_a_remote_replaces_the_forward_and_keeps_its_colour`,
  `::deleting_a_host_ends_its_forward`.
- Rule 62: `crates/ariadne-cli/tests/hosts.rs::host_add_posts_the_host_and_prints_its_row`,
  `::host_edit_patches_only_the_given_fields_and_null_clears`,
  `::host_rm_deletes_the_host_and_prints_its_name`.
- Rule 61: `ui/scripts/host-events.test.mjs::closes a deleted host stream and removes its cache only from local events`.
- An incompatible skill API reports the upgrade and recovers: `host_sync.rs::incompatible_skill_api_reports_upgrade_and_recovers`.

- Hosts keep file order and an unknown key is refused by name
  (`endpoint.rs::hosts_keep_the_order_of_the_file`,
  `::an_unknown_key_in_a_host_is_refused_by_name`,
  `::a_host_without_an_endpoint_is_refused`).
- Rules 2 and 3 hold at startup and in `--check-config`
  (`hosts.rs::a_bad_host_is_refused_by_name_at_startup_and_by_check_config`,
  `::good_hosts_pass_check_config`).
- Rules 4 and 5
  (`hosts.rs::a_daemon_without_hosts_lists_only_local`,
  `::configured_hosts_follow_local_in_file_order`).
- Rule 11, no answer through the forward
  (`host_health.rs::a_forward_whose_far_daemon_does_not_answer_is_down_and_starts_again`).
- Rules 13 and 30, shutdown
  (`host_health.rs::shutdown_ends_the_checks_of_a_direct_host`,
  `ssh_forward.rs::shutdown_ends_every_ssh_child`).
- Rule 30
  (`host_health.rs::a_direct_host_is_ready_while_it_answers_and_down_while_it_does_not`,
  `::a_ready_forward_that_stops_answering_loses_its_forward_and_starts_again`).
- Rules 31 and 32
  (`host_health.rs::a_direct_host_that_does_not_answer_is_down_and_ready_once_it_answers`,
  `::a_direct_host_is_ready_while_it_answers_and_down_while_it_does_not`).
- Rule 33
  (`host_health.rs::a_direct_host_on_a_socket_path_is_checked_through_the_socket`).
- Rule 34
  (`host_health.rs::a_direct_host_is_ready_while_it_answers_and_down_while_it_does_not`,
  `host_sync.rs::each_change_of_the_sync_publishes_one_host_updated`,
  `host_sync.rs::sync_stops_blocked_and_queued_pushes_until_the_host_recovers`).
- Rule 6, CLI event display: `commands/events.rs::a_deleted_host_frame_names_the_removed_host`.
- Rule 7 (`hosts.rs::openapi_carries_the_route_the_dto_and_the_event`).
- Rules 8 and 9 (`ssh_forward.rs::help_and_check_config_list_ssh_bin`).
- Rule 10 (`ssh_forward.rs::a_host_with_no_remote_forwards_to_the_far_users_own_socket`,
  `::a_host_with_a_remote_forwards_to_it_and_asks_for_no_home`,
  `::a_home_call_that_fails_makes_the_host_down_with_its_last_stderr_line`,
  `::an_empty_home_makes_the_host_down`,
  `::a_relative_home_makes_the_host_down`,
  `::a_home_call_past_its_bound_is_down_and_its_process_ends`,
  `::a_reconnect_ends_a_home_call_under_way`,
  `::shutdown_ends_a_home_call_under_way`,
  `portable_config.rs::an_idle_worker_restarts_with_an_open_event_stream`,
  `::a_home_call_that_exits_keeps_a_long_last_stderr_line_whole`,
  `::a_home_call_past_its_bound_keeps_a_long_unended_stderr_line_whole`,
  `src/hosts.rs::a_forward_names_the_user_the_port_and_the_remote`,
  `::a_forward_without_user_or_port_names_the_host_alone`,
  `ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`,
  `::a_forward_to_a_tcp_port_comes_up_and_goes_down`).
- Rules 6, 11 and 12
  (`ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`,
  `::an_exited_forward_is_down_with_its_last_stderr_line_and_starts_again`,
  `::a_signalled_forward_reports_ssh_stderr_without_guard_diagnostics`,
  `::an_unanswered_forward_names_the_far_daemon_and_ssh_last_line`).
- Rule 13 (`ssh_forward.rs::shutdown_ends_every_ssh_child`).
- Rule 14 (`ssh_forward.rs::an_event_stream_and_a_post_both_cross_a_ready_forward`).
- Rules 4, 7 and 22 (`hosts.rs::a_daemon_without_hosts_lists_only_local`,
  `::stored_hosts_follow_local_in_insertion_order`,
  `::openapi_carries_the_route_the_dto_and_the_event`).
- Rules 23, 27 and 28
  (`host_sync.rs::a_ready_host_takes_every_item_and_keeps_its_own`).
- Rule 24
  (`host_sync.rs::each_later_change_reaches_the_host_and_a_delete_removes_it`).
- Rule 25
  (`host_sync.rs::a_queued_skill_deletion_reaches_the_host_after_recovery`,
  `::an_interrupted_skill_deletion_reaches_the_host_after_recovery`,
  `::a_recreated_skill_supersedes_its_queued_deletion_after_recovery`,
  `::a_refused_push_holds_back_no_later_change`,
  `::a_refused_repository_holds_back_no_other_repository`,
  `::a_newer_push_of_an_item_ends_its_older_failed_push`,
  `ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`).
- Rule 26
  (`host_sync.rs::a_repository_takes_the_local_settings_by_forge_and_a_missing_one_is_listed`,
  `src/hosts/sync.rs::a_repository_pairs_by_forge_and_prefers_its_own_base_branch`,
  `::a_forge_repository_on_another_branch_pairs_once`).
- Rule 27
  (`host_sync.rs::a_ready_host_takes_every_item_and_keeps_its_own`,
  `::each_later_change_reaches_the_host_and_a_delete_removes_it`,
  `::a_learned_permission_without_a_checkout_on_the_host_is_counted_as_skipped`;
  the route is spec 022, rule 21).
- Rules 6 and 22
  (`host_sync.rs::each_change_of_the_sync_publishes_one_host_updated`,
  `host_sync.rs::sync_stops_blocked_and_queued_pushes_until_the_host_recovers`).
- Rule 28, device: no sync sends `device`; a device set on the host stays
  until `local` changes the flavour, then the host picks its best device
  (`host_sync.rs::a_device_set_on_the_host_stays_until_local_changes_the_flavour`,
  `ssh_forward.rs::a_device_set_on_the_host_stays_when_the_host_becomes_ready_again`).

- Rule 17
  (`host_queries.rs::a_named_host_answers_from_its_own_daemon_and_local_from_this_one`,
  `::an_unknown_host_is_404_and_a_host_that_is_not_ready_is_503`).
- Rules 18 and 19
  (`host_queries.rs::all_merges_every_ready_host_with_a_host_label_and_lists_failures`,
  `::all_labels_streams_series_and_label_values_with_the_host`,
  `::a_query_every_host_refuses_answers_its_400`,
  `::all_keeps_the_flat_shape_of_the_index_stats`,
  `::all_keeps_the_warnings_on_a_parsed_query`).
- Rule 20 (`tests/hosts.rs::query_host_all_reads_the_merged_answer_and_prints_a_host_column`,
  `commands/query.rs::tests::a_host_moves_the_route_under_the_hosts_prefix`,
  `::host_all_puts_the_host_column_first`,
  `tests/hosts.rs::query_on_an_unknown_or_unready_host_is_a_usage_error`).
- Rule 21 (`host_queries.rs::openapi_carries_the_host_query_routes`).
- Rule 35
  (`host_reconnect.rs::local_is_400_and_an_unknown_name_is_404_in_the_error_envelope`,
  `::a_direct_host_takes_one_check_and_its_status_follows_the_answer`).
- Rules 36 and 41
  (`host_reconnect.rs::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after`,
  `hosts.rs::tests::a_reset_backoff_waits_the_first_value_again_and_the_cap_holds`,
  `hosts.rs::tests::a_reconnect_cuts_the_backoff_and_the_wait_after_it_is_the_first_again`).
- Rules 11 and 37
  (`host_reconnect.rs::a_connecting_forward_loses_its_try_and_starts_a_new_one_at_once`,
  `::each_reconnect_reaps_the_previous_forward_before_answering`,
  `host_health.rs::cleanup_retries_keep_a_zombie_group_owned_until_it_disappears`,
  `::a_zombie_reaped_before_the_deadline_reports_only_down`).
- Rule 38
  (`host_reconnect.rs::a_ready_forward_takes_one_check_and_a_good_answer_cuts_no_stream`).
- Rules 39 and 41
  (`host_reconnect.rs::a_ready_forward_whose_check_fails_is_down_and_starts_again_at_once`).
- Rules 40 and 41
  (`host_reconnect.rs::a_direct_host_takes_one_check_and_its_status_follows_the_answer`).
- Rule 42 (`host_reconnect.rs::openapi_carries_the_reconnect_route`).

- Rules 43 and 44: `hosts.rs::setup_rejects_reserved_names_and_keeps_unknown_operations_absent`.
- Rules 45 and 46: `hosts.rs::setup_checks_linux_x86_64_before_it_transfers_a_binary`,
  `hosts.rs::setup_checks_linux_arm64_before_it_transfers_a_binary`,
  `hosts.rs::setup_checks_macos_x86_64_before_it_transfers_a_binary`,
  `hosts.rs::setup_checks_macos_arm64_before_it_transfers_a_binary`.
- Rule 45: `hosts.rs::release_setup_requires_workflow_and_tag_verification_before_transfer`.
- Rules 46–48: `hosts.rs::setup_preserves_installation_after_service_failure_and_retries_safely`.
- Rule 46, relay persistence: `hosts.rs::setup_uses_the_ssh_path_when_the_login_shell_fails_or_returns_no_path`.
- Rule 46, banners: `hosts.rs::setup_ignores_login_banners_with_a_newline_in_service_paths`,
  `hosts.rs::setup_ignores_login_banners_without_a_newline_in_service_paths`.
- Rule 46, control characters: `hosts.rs::setup_rejects_control_characters_in_remote_path_before_transfer`.
- Rule 46, shell fallback: `hosts.rs::setup_uses_the_ssh_path_when_the_login_shell_fails_or_returns_no_path`.
- Rules 47 and 48, PATH order: `hosts.rs::setup_reads_the_interactive_shell_path_first_and_falls_back_in_order`.
- Rule 48, real bash, zsh and sh without a terminal: `setup.rs::tests::the_path_probe_reads_the_path_of_real_shells_without_a_terminal`.
- Rule 47, a command that differs from its package name: `hosts.rs::an_agent_whose_command_differs_from_its_package_name_is_found_by_its_command` and `plan.rs::tests::every_npm_or_uv_registry_agent_has_its_executable_named`.
- Rule 47, a command that exists but does not run: `hosts.rs::inspection_does_not_count_an_agent_or_provider_command_that_does_not_run` and the last two cases of `hosts.rs::a_failed_agent_step_names_what_is_missing`.
- Rule 48, control characters of the interactive PATH: `hosts.rs::setup_rejects_a_control_character_in_the_interactive_shell_path`.
- Rule 47, runtimes, agents and provider CLIs on the PATH: `hosts.rs::inspection_finds_runtimes_agents_and_provider_clis_on_the_interactive_shell_path`.
- Rule 47, reuse of items installed by hand: `hosts.rs::setup_reuses_agents_and_provider_clis_that_the_user_installed_by_hand`.
- Rule 47, what a failed agent step names: `hosts.rs::a_failed_agent_step_names_what_is_missing`.
- Rule 47, an update still upgrades what setup installed: `hosts.rs::explicit_updates_upgrade_already_installed_uv_tools`.
- Rule 47, an update reuses runtimes without Homebrew: `hosts.rs::an_update_reuses_hand_installed_node_and_npm_without_homebrew`, `hosts.rs::an_update_reuses_a_hand_installed_uv_without_homebrew` `hosts.rs::an_update_installs_no_second_runtime_beside_a_hand_installed_one` and `hosts.rs::an_update_names_the_runtime_that_homebrew_did_not_provide`.
- Rule 47, Node.js installs from the nodejs.org LTS archive into `~/.local` without Homebrew (criteria 2, 6): `hosts.rs::setup_installs_the_node_js_lts_archive_into_the_home_when_no_homebrew_exists`.
- Rule 47, a bad checksum leaves no file under `~/.local` (criterion 3): `hosts.rs::a_node_js_archive_that_fails_its_checksum_installs_nothing`.
- Rule 47, a failed download installs nothing and names the download (criterion 3): `hosts.rs::a_failed_node_js_download_installs_nothing_and_names_the_download` and `hosts.rs::a_failed_uv_download_names_astral_sh`.
- Rule 47, uv installs with the astral.sh installer into `~/.local/bin` (criterion 4): `hosts.rs::setup_installs_uv_with_the_standalone_installer_when_no_homebrew_exists`.
- Rule 47, a user-owned Homebrew comes first (criterion 1): `hosts.rs::setup_uses_a_user_owned_homebrew_before_it_downloads_a_runtime`.
- Rule 47, a download that runs past `Timeouts::worker_runtime_install` ends with its step (criterion 5): `hosts.rs::a_runtime_install_that_runs_past_its_timeout_stops_the_step`.
  `hosts.rs::a_disconnected_runtime_install_ends_the_download_process` verifies cleanup after observing an active download.
- Rule 47, a missing Bash supervisor names the prerequisite before downloading: `hosts.rs::a_worker_without_bash_names_the_missing_runtime_supervisor`.
- Rule 47, a present runtime shows as already present (criterion 6): `hosts.rs::a_runtime_that_was_present_shows_as_already_present`.
- Rule 47, an update upgrades what setup installed and leaves the user's runtime (criterion 7, Node.js and uv, owned and user-installed): `hosts.rs::an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed`.
- Rule 83, the manual action of a failed uv install names astral.sh and the command (criterion 8): `hosts.rs::inspection_names_who_installs_each_agent_github_and_each_sign_in`.
- Rule 83, the manual action of a failed Node.js install names nodejs.org and the command (criterion 8): `hosts.rs::the_manual_action_of_a_failed_node_js_install_names_nodejs_org_and_the_command`.
- Rule 49: `crates/ariadne-cli/tests/hosts.rs::worker_setup_and_status_show_each_completed_step_and_failure`.
- Rule 49, skip: `hosts.rs::skipped_failed_tool_and_sign_in_steps_let_setup_complete_and_connect_the_machine` proves
  the `409` while setup runs, each refusal, the kept error and manual action, no second sign-in check, and the connected machine.
  `hosts.rs::a_skipped_failed_step_of_an_update_lets_the_update_complete` proves the same rules for an update.
- Rule 50: `hosts.rs::setup_preserves_installation_after_service_failure_and_retries_safely`.
- Rule 50: `ui/src/features/hosts/setup-progress.test.tsx::shows each step by its title, and a manual action with its reason, its command and a copy control`.
- Rule 52: `ui/src/features/hosts/add-machine.test.tsx::sends a source only when the Advanced inputs name the programs, and says why Next waits`
  and `ui/src/features/hosts/setup-progress.test.tsx::resumes the setup from Check again and shows the progress it answers with`.

The tests run a stub `ssh` (`tests/it/common/ssh.rs`) that relays its `-L`
port to its remote, with a second harness daemon as the far machine. The sync
and query tests serve the second harness daemon on a loopback port, and give
the local one a `direct` host that points at it. The health tests serve the
second harness daemon behind a switch that turns its `GET /v1/health` off and
on, and shorten `host_unanswered` and `host_check` through `Timeouts`.

- Failed skill pushes wait for backoff despite supporting-file scans
  (`host_sync.rs::a_failed_skill_waits_for_backoff_despite_bundle_scans`).
- Skill bundles retain layout, executable bits, updates and removals
  (`host_sync.rs::supporting_files_reach_the_worker_and_later_changes_replace_them`).
- Unsafe paths preserve the worker document (`host_sync.rs::an_invalid_bundle_preserves_the_worker_skill`).
- Failed staging preserves previous binary files
  (`host_sync.rs::failed_staging_preserves_the_previous_worker_files`).
- Escaping links preserve the worker and allow unrelated skills
  (`host_sync.rs::an_escaping_link_preserves_the_worker_and_does_not_block_other_skills`).
- External Markdown links warn without copying dependencies
  (`host_sync.rs::external_markdown_links_warn_without_copying_external_files`).

- Rules 53–55: `hosts.rs::setup_reports_custom_tools_without_executing_install_commands`,
  `::selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy`,
  `::credential_values_are_rejected_before_an_operation_is_persisted`,
  `::selected_github_credentials_enable_git_without_entering_command_arguments`.
- Desktop selections: `ui/src/features/hosts/add-machine.test.tsx::asks one sign-in choice per needed sign-in, and copies only where inspection permits it`.

- Portable live settings preserve worker locations and survive reload
  (`portable_config.rs::portable_settings_apply_without_changing_worker_paths`).
- Worker commands and worker-only entries survive matching source updates
  (`portable_config.rs::worker_commands_and_worker_only_agents_survive_source_replacement`).
- Provider/model flags and scoped package arguments remain portable
  (`portable_config.rs::provider_model_flags_remain_portable_with_live_policy_updates`,
  `::scoped_npx_package_arguments_remain_portable`).
- Unmapped paths and invalid retention fail before installation
  (`portable_config.rs::an_unmapped_source_path_fails_without_applying_settings`,
  `::invalid_retention_fails_before_any_setting_changes`).
- Maintenance lets admitted tasks continue and holds new work
  (`portable_config.rs::maintenance_keeps_admitted_tasks_running_and_holds_new_tasks`).
- Process replacement keeps the worker PID and works with an open event stream
  (`portable_config.rs::an_idle_worker_restarts_with_an_open_event_stream`).
- A failed replacement retains service availability and permits retry
  (`portable_config.rs::a_failed_process_replacement_keeps_the_worker_available_and_can_retry`).
- Existing retention and cleanup consumers use live policies
  (`portable_config.rs::a_running_retention_job_uses_the_new_cutoff`,
  `::a_live_cleanup_policy_keeps_the_finished_worktree`,
  `::the_logging_filter_changes_without_a_restart`,
  `::an_existing_runtime_classifies_errors_with_the_new_patterns`).
- Source file and flag changes reach workers; configuration errors leave catalog transfers working
  (`host_sync.rs::portable_policies_and_flags_reach_the_worker_after_source_changes`,
  `::unmapped_configuration_fails_without_blocking_catalogs_and_can_retry`).
- CLI listings expose pending restart (`crates/ariadne-cli/tests/hosts.rs::named_hosts_and_guards_use_the_local_registry`).
  Desktop sync lines expose pending restart and configuration errors
  (`ui/src/components/host-sync.test.tsx`: `shows configuration status %s`).

Inspection and review proof: `hosts.rs::inspection_needs_only_ssh_and_does_not_install_or_register`
and `hosts.rs::review_lists_transfers_and_manual_actions_without_changing_the_worker`.
Git prerequisite proof: `hosts.rs::binary_only_setup_requires_git_and_resumes_after_manual_installation`.
CLI host guard proof: `onboarding_commands_refuse_other_hosts_with_a_setup_reason`.
Partial setup proof: `hosts.rs::setup_reports_custom_tools_without_executing_install_commands`.
Resume proof: `hosts.rs::setup_preserves_installation_after_service_failure_and_retries_safely`.
Desktop proof: `ui/src/features/hosts/add-machine.test.tsx::reviews the plan in three groups, keeps the inputs on Back, and reviews a changed selection again`.

- Rules 79–82: `hosts.rs::inspection_names_who_installs_each_agent_github_and_each_sign_in`.
- Rules 79 and 83: `hosts.rs::review_plans_each_requested_item_in_one_of_three_groups`.
- Rule 84: `hosts.rs::a_step_that_waits_for_manual_work_says_what_its_plan_item_says`.
- Rules 80, 83 and 84, present packages: `hosts.rs::an_agent_package_already_on_the_worker_is_already_present`
  and `hosts.rs::progress_puts_a_step_that_reused_a_present_item_in_already_present`.
- Rules 85 and 86: `crates/ariadne-cli/tests/hosts.rs::worker_inspection_review_and_status_print_three_groups_with_manual_commands`
  and `::worker_inspection_review_and_resume_use_the_local_setup_contract`, which reads responses without the new fields.

## Sources

`crates/ariadne-client/src/endpoint.rs`, `crates/ariadne-api/src/hosts.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/hosts/sync.rs`,
`crates/ariadne-daemon/src/timeouts.rs`,
`crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`crates/ariadne-daemon/src/http/query/fan.rs`, `docs/configuration.md`,
`docs/telemetry.md`.

SSH setup sources: `crates/ariadne-daemon/src/hosts/setup.rs`, `crates/ariadne-daemon/src/hosts/setup/plan.rs`, `ui/src/features/hosts/add-machine-dialog.tsx`, `ui/src/features/hosts/machine-setup.ts`,
`ui/src/features/hosts/setup-progress.tsx`, `ui/src/features/hosts/update-machine-dialog.tsx`.

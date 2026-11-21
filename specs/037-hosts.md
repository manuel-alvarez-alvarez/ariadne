---
id: 037
status: current
updated: 2026-11-21
areas: [api, daemon, client, cli, docs, ui]
commits: []
tests:
  - crates/ariadne-client/src/endpoint.rs
  - crates/ariadne-daemon/tests/it/hosts.rs
  - crates/ariadne-daemon/tests/it/ssh_forward.rs
  - crates/ariadne-daemon/tests/it/host_sync.rs
  - crates/ariadne-daemon/tests/it/host_health.rs
  - crates/ariadne-daemon/tests/it/host_reconnect.rs
  - crates/ariadne-daemon/tests/it/learned_permissions.rs
  - crates/ariadne-daemon/tests/it/host_queries.rs
  - crates/ariadne-cli/tests/hosts.rs
  - crates/ariadne-cli/src/commands/query.rs
  - crates/ariadne-daemon/src/hosts.rs
  - crates/ariadne-daemon/src/hosts/sync.rs
  - ui/src/features/hosts/worker-setup.test.tsx
---

# Hosts

## Scope

In: the `[hosts.<name>]` tables of `config.toml`, their validation, the
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

1. `FileConfig.hosts` reads `[hosts.<name>]` tables, in file order. Each has
   `endpoint` (required string) and `remote` (optional string). An unknown key
   in a table is refused by name.
2. `endpoint` is `ssh://[user@]host[:port]`, `http://host:port`, or an absolute
   socket path. `remote` is accepted only with `ssh://`, and is an absolute
   socket path or a loopback `host:port`. Any other value is refused by name
   (`hosts.<name>.endpoint`, `hosts.<name>.remote`) at startup and by
   `ariadned --check-config`, in the same words. The scheme is lowercase:
   `SSH://box` is refused, so every later check of the text agrees.
3. The names `local` and `all` are refused by name: `local` is this daemon,
   and `all` selects every host (spec 014, rule 46). A name has only letters,
   digits, `-` and `_`.
4. `GET /v1/hosts` returns `HostDto[]`. The first row is `local`; the others
   follow in file order. `HostDto` has `name`; `kind` (`local`, `direct`,
   `ssh`); `endpoint` (the address a client on this machine uses: for `local`
   the `tcp_listen` URL when set, else the socket path; for `direct` the
   configured value; for `ssh` null until a forward is up); `status` (`ready`,
   `connecting`, `down`); `error`; `remote`; `sync` (rule 22).
5. An `ssh` host is `down`, with a null `endpoint` and the error
   `ssh forward not started`, until its forward starts. A `direct` host is
   `connecting` until its first health check (rule 30).
6. `host_updated` carries one `HostDto`. The daemon publishes one event for
   each change of a host's `status`, `endpoint`, `error` or `sync`.
7. The OpenAPI document carries the route, the DTO and the event.
8. `ariadned --help` lists the `hosts` table and `ssh_bin`.
9. `ssh_bin` names the `ssh` program (default: `ssh` on the daemon's PATH).
   `ariadned --check-config` prints `ssh_bin = <value>`.
10. At startup, for each `ssh` host, the daemon picks a free loopback port and
    runs `<ssh_bin> -N -L 127.0.0.1:<port>:<remote> -o ServerAliveInterval=15
    -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes -o BatchMode=yes
    [-p <port>] [user@]host`. `remote` defaults to the local daemon's own
    `socket_path`. A socket path forwards to a unix socket; a `host:port`
    forwards to TCP.
11. The host is `connecting` while the process starts. The daemon probes
    `GET /v1/health` through the port every `ssh_probe_interval`, each probe
    bounded by `ssh_probe`. On the first good answer the host is `ready` and
    its `endpoint` is `http://127.0.0.1:<port>`. When no good answer comes in
    `host_unanswered`, the daemon kills the process and the host is `down`
    with the error `the far daemon does not answer through the forward:
    <reason>`. The restart of rule 12 follows.
12. When the process exits, the host is `down`, `endpoint` is null and `error`
    is the last non-empty stderr line of the process (else its exit status).
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

22. `sync` is `state` (`in_sync`, `pending`, `failed`), `error` (string or
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
    23 again.
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
44. Escaping symbolic links fail that skill push and name the offending path.
    Other skills continue. Failed staging or replacement preserves recoverable previous worker contents.
    Explicit Markdown local-file links outside the skill directory produce warnings in `sync.error`.
    Such warnings leave successful transfers `in_sync`; web links produce no warning.
    External files are not transferred. Prose and shell examples are not dependency detection inputs.

### SSH worker setup

45. `POST /v1/hosts/{name}/setup` starts or retries setup and returns `202` with `SetupDto`.
    `GET` on the same path returns progress, or `404` for an unknown operation.
    Reserved or invalid names and invalid requests return `400`.
46. A request contains `ssh`, optional absolute socket `remote`, and `source`.
    SSH uses `ssh://[user@]host[:port]`, BatchMode, and the daemon user's existing SSH authentication.
    Passwords in URLs are refused. No credential values are accepted or returned.
    A local source contains `kind: local`, `cli`, and `daemon`.
    Both paths refer to files on the source daemon machine that runs the settings daemon.
    A release source contains `kind: release`, `repository`, and an explicit `tag`.
    Optional `tools` selects shipped registry agent IDs and GitHub tooling.
    Optional `credentials` contains source references only. Repository installation remains outside this operation.
47. Setup probes the remote OS, architecture, and home before transfer.
    macOS and Linux support ARM64 and x86_64.
    Both local binaries must have matching executable headers before either transfers.
    macOS accepts thin binaries and universal binaries with a compatible executable slice.
    Release assets use `ariadne-<target>.tar.gz`.
    GitHub CLI 2.102.0 or later verifies the selected repository's release workflow and exact tag before extraction.
48. Setup installs binaries under the remote user's `~/.local/bin`.
    It registers `dev.ariadne.daemon` with launchd or `ariadned.service` with systemd user services.
    New service definitions use the remote user's PATH from a noninteractive login shell, with `~/.local/bin` first.
    Setup uses only PATH text between unique probe markers, ignoring login-profile banners and other output.
    If the shell fails, markers are missing, or the marked PATH is empty, setup uses the SSH session's PATH.
    Setup rejects PATH values with control characters before transfer, including newline and NUL.
    Systemd values escape backslashes, quotes, and percent specifiers; dollar signs stay literal in `Environment=`.
    Launchd stores the XML-escaped PATH in its `EnvironmentVariables` dictionary.
    Existing service definitions, including PATH and machine-specific settings, remain unchanged through retries.
    Configuration and worker data remain intact.
    Missing git, tar, user services, or Linux lingering produce prerequisite guidance without administrator commands.
    An existing custom socket must be supplied through `remote`.
49. Progress states are `pending`, `running`, `incomplete`, `failed`, and `complete`.
    Base steps are probe, validate, transfer, install, service, health, and register.
    Selected tools, credential transfers, and authentication checks have individual results.
    Failure retains completed steps and records the failing step's error.
    No successful step means `failed`; otherwise failure means `incomplete`.
    Completion requires a successful daemon status check and local connection registration.
50. The settings daemon stores operations atomically in `worker-setup.json` under its home.
    Restart converts interrupted operations to `incomplete` and restores registered connections.
    The same request resumes setup without transferring installed binaries again.
    Retry rechecks access and binaries, restarts the service, and checks health.
    Concurrent repeats return the existing operation. Changed requests are refused while setup is incomplete.
    After completion, repository selections can change with the same SSH access and binary source.
    A complete request stays complete on repetition.
51. `ariadne host setup NAME --request FILE` starts or retries an operation.
    `ariadne host setup-status NAME` shows progress and per-step errors.
    Both use the local daemon; JSON output uses the operation DTO.
    A failed or incomplete result exits unsuccessfully.
52. The Desktop Hosts screen has a "Set up worker" dialog. It includes SSH access, local binary paths, and verified release selection.
    Both binary path labels identify the source daemon machine that runs the settings daemon.
    It shows completed steps and failures, polls active setup, and permits retry with the same request.
    Entering a worker name and loading progress restores its durable operation view.

53. Tool selections install Git and optional `gh`, plus npm or uv distributions from the shipped ACP registry.
    User-configured commands override registry IDs and receive manual installation guidance.
    Unsupported IDs and distributions receive manual installation guidance without generated commands.
    npm uses the worker user's `.local` prefix. uv manages user tools and Python.
    Setup verifies npm packages with `npm list --global` in that prefix after installation and during retries.
    Missing Git, gh, Node/npm, or uv use existing user-owned Homebrew without administrator commands.
    Without that installation path, progress reports administrator prerequisites and remains incomplete.
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

56. Repository population extends setup with an optional `repositories` map from source IDs to worker destinations.
    Destinations accept absolute paths or `~/` paths resolved against the probed worker home.
    Desktop proposes `~/workspace/<repository>` and permits each destination to change.
    Setup clones Git remote history without source edits or untracked files.
    Registered checkout reuse follows rule 26, including base branch preference and preservation of worker paths.
    Each selection records `clone:<id>` and `repository:<id>` progress.
    Missing remotes, worker authentication failures, and inaccessible destinations leave setup incomplete.
    Setup refuses unrelated existing destination directories.
    Completed clone and registration checkpoints survive retries.
    Registration invokes the normal host synchronization for settings and learned permissions.
    Automatic enrollment of later repositories and movement of existing checkouts remain outside scope.

## Acceptance criteria

- Rule 56: `hosts.rs::setup_accepts_selected_repository_destinations` and
  `hosts.rs::selected_repositories_clone_only_committed_files_and_retry_registration`;
  `hosts.rs::setup_rejects_unsafe_repository_destinations_before_starting` proves request validation.
  `hosts.rs::repository_setup_preserves_unrelated_existing_destinations` proves preservation of unrelated directories, including empty directories.
  `hosts.rs::repository_setup_reports_a_missing_source_remote` proves the missing-remote step failure.
  `hosts.rs::repository_setup_reports_worker_git_authentication_failure` proves failure after a worker receives an HTTP authentication challenge.
  `worker-setup.test.tsx` proves selection and destination editing.
  `hosts.rs::worker_setup_and_status_show_each_completed_step_and_failure` proves CLI selection and progress.

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
- Rule 7 (`hosts.rs::openapi_carries_the_route_the_dto_and_the_event`).
- Rules 8 and 9 (`ssh_forward.rs::help_and_check_config_list_ssh_bin`).
- Rule 10 (`ssh_forward.rs::a_forward_runs_ssh_to_the_local_socket_path_by_default`,
  `src/hosts.rs::a_forward_names_the_user_the_port_and_the_remote`,
  `::a_forward_without_user_or_port_names_the_host_alone`,
  `ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`,
  `::a_forward_to_a_tcp_port_comes_up_and_goes_down`).
- Rules 6, 11 and 12
  (`ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`,
  `::an_exited_forward_is_down_with_its_last_stderr_line_and_starts_again`).
- Rule 13 (`ssh_forward.rs::shutdown_ends_every_ssh_child`).
- Rule 14 (`ssh_forward.rs::an_event_stream_and_a_post_both_cross_a_ready_forward`).
- Rules 4, 7 and 22 (`hosts.rs::a_daemon_without_hosts_lists_only_local`,
  `::configured_hosts_follow_local_in_file_order`,
  `::openapi_carries_the_route_the_dto_and_the_event`).
- Rules 23, 27 and 28
  (`host_sync.rs::a_ready_host_takes_every_item_and_keeps_its_own`).
- Rule 24
  (`host_sync.rs::each_later_change_reaches_the_host_and_a_delete_removes_it`).
- Rule 25
  (`host_sync.rs::a_refused_push_holds_back_no_later_change`,
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
  (`host_reconnect.rs::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after`).
- Rule 37
  (`host_reconnect.rs::a_connecting_forward_loses_its_try_and_starts_a_new_one_at_once`).
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
- Rule 46, banners: `hosts.rs::setup_ignores_login_banners_with_a_newline_in_service_paths`,
  `hosts.rs::setup_ignores_login_banners_without_a_newline_in_service_paths`.
- Rule 46, control characters: `hosts.rs::setup_rejects_control_characters_in_remote_path_before_transfer`.
- Rule 46, shell fallback: `hosts.rs::setup_uses_the_ssh_path_when_the_login_shell_fails_or_returns_no_path`.
- Rule 49: `crates/ariadne-cli/tests/hosts.rs::worker_setup_and_status_show_each_completed_step_and_failure`.
- Rule 50: `ui/src/features/hosts/worker-setup.test.tsx::starts setup and shows retained steps and a failure`.
- Rule 52: `ui/src/features/hosts/hosts-page.test.tsx::titles the screen Hosts and opens the worker setup from its Set up worker action`.

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
- Desktop selections: `ui/src/features/hosts/worker-setup.test.tsx::sends explicit tool and credential selections`.

## Sources

`crates/ariadne-client/src/endpoint.rs`, `crates/ariadne-api/src/hosts.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/hosts/sync.rs`,
`crates/ariadne-daemon/src/timeouts.rs`,
`crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`crates/ariadne-daemon/src/http/query/fan.rs`, `docs/configuration.md`,
`docs/telemetry.md`.

SSH setup sources: `crates/ariadne-daemon/src/hosts/setup.rs`, `ui/src/features/hosts/worker-setup.tsx`.

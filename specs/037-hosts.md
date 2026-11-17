---
id: 037
status: current
updated: 2026-11-17
areas: [api, daemon, client, cli, docs]
commits: []
tests:
  - crates/ariadne-client/src/endpoint.rs
  - crates/ariadne-daemon/tests/it/hosts.rs
  - crates/ariadne-daemon/tests/it/ssh_forward.rs
  - crates/ariadne-daemon/tests/it/host_sync.rs
  - crates/ariadne-daemon/tests/it/learned_permissions.rs
  - crates/ariadne-daemon/tests/it/host_queries.rs
  - crates/ariadne-cli/tests/hosts.rs
  - crates/ariadne-cli/src/commands/query.rs
  - crates/ariadne-daemon/src/hosts.rs
  - crates/ariadne-daemon/src/hosts/sync.rs
---

# Hosts

## Scope

In: the `[hosts.<name>]` tables of `config.toml`, their validation, the
`GET /v1/hosts` route, `HostDto`, the `host_updated` event, `ssh_bin`, the
SSH forward the daemon keeps for each `ssh://` host, and the push of the local
catalogs and settings to each host. Out: a Rust SSH library, a reverse forward
from the far daemon, cloning a repository on a host, any write from a host
back to this daemon and any desktop change. CLI selection follows spec 014.
The transports and the event stream are spec 012's.
`GET /v1/hosts` route, `HostDto`, the `host_updated` event, `ssh_bin`, and the
SSH forward the daemon keeps for each `ssh://` host. Out: a Rust SSH library and
a reverse forward from the far daemon. CLI selection follows spec 014. The
transports and the event stream are spec 012's. The desktop app's reading of
the hosts — the host picker, the Hosts section of its settings and one stream
per host — is spec 015's (rules 51 to 60).

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
3. The name `local` is refused. A name has only letters, digits, `-` and `_`.
4. `GET /v1/hosts` returns `HostDto[]`. The first row is `local`; the others
   follow in file order. `HostDto` has `name`; `kind` (`local`, `direct`,
   `ssh`); `endpoint` (the address a client on this machine uses: for `local`
   the `tcp_listen` URL when set, else the socket path; for `direct` the
   configured value; for `ssh` null until a forward is up); `status` (`ready`,
   `connecting`, `down`); `error`; `remote`; `sync` (rule 22).
5. An `ssh` host is `down`, with a null `endpoint` and the error
   `ssh forward not started`, until its forward starts.
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
    its `endpoint` is `http://127.0.0.1:<port>`.
12. When the process exits, the host is `down`, `endpoint` is null and `error`
    is the last non-empty stderr line of the process (else its exit status).
    The daemon starts it again after `ssh_retry`, doubled after each exit up
    to 60 seconds and reset by a `ready`; the host goes back to `connecting`.
13. Daemon shutdown ends every ssh child process.
14. One forward carries requests, event streams and answers both ways. The
    forward is the user's own `ssh`: their keys, agent and `~/.ssh/config`
    apply.
15. CLI host selection follows spec 014.

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
    `default_thresholds`), and `flavour`. `device` is not pushed: the host
    picks its own best device for the flavour. A status event that changes
    none of these pushes nothing. A device set on one host with
    `permissions ai set --device --host NAME` (spec 014, rule 32) stays there:
    no sync sends it.
29. Writes go to the local daemon. Nothing is read back from a host into
    this daemon.
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
    stderr. With `all` the table has a `HOST` column from the label. Without
    `--host` the route is unchanged.
21. The OpenAPI document carries the routes of rule 16.

## Acceptance criteria

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
  (`host_sync.rs::a_failed_push_is_failed_with_its_error_and_tried_again_until_it_lands`,
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
  (`host_sync.rs::each_change_of_the_sync_publishes_one_host_updated`).
- Rule 28, device half: the device goes to the named host alone
  (`tests/hosts.rs::permissions_device_is_written_on_the_named_host_only`).

The tests run a stub `ssh` (`tests/it/common/ssh.rs`) that relays its `-L`
port to its remote, with a second harness daemon as the far machine. The sync
tests serve the second harness daemon on a loopback port, and give the local
one a `direct` host that points at it.
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
  `commands/query.rs::tests::a_host_moves_the_route_under_the_hosts_prefix`).
- Rule 21 (`host_queries.rs::openapi_carries_the_host_query_routes`).

The query tests run two harness daemons: the local one with a `direct` host
that points at the second, served on a loopback port.

The forward tests run a stub `ssh` (`tests/it/common/ssh.rs`) that relays its `-L`
port to its remote, with a second harness daemon as the far machine.

## Sources

`crates/ariadne-client/src/endpoint.rs`, `crates/ariadne-api/src/hosts.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/hosts/sync.rs`,
`crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`docs/configuration.md`.
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/query/fan.rs`, `docs/configuration.md`,
`docs/telemetry.md`.

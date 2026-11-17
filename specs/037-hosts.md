---
id: 037
status: current
updated: 2026-11-17
areas: [api, daemon, client, docs]
commits: []
tests:
  - crates/ariadne-client/src/endpoint.rs
  - crates/ariadne-daemon/tests/it/hosts.rs
  - crates/ariadne-daemon/tests/it/ssh_forward.rs
  - crates/ariadne-daemon/tests/it/host_sync.rs
  - crates/ariadne-daemon/tests/it/learned_permissions.rs
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
   `connecting`, `down`); `error`; `remote`; `sync` (rule 15).
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

15. `sync` is `state` (`in_sync`, `pending`, `failed`), `error` (string or
    null) and `missing_repositories` (`MissingRepositoryDto[]`: the local
    repository's `id`, and `forge`, its `owner/repo`, or null for a
    repository with no forge) and `skipped_permissions` (rule 20). `local`
    is always `in_sync` with an empty list and no skipped permission. A
    configured host starts `pending`.
16. When a host becomes `ready`, the daemon pushes, in order: every skill
    (`PUT /v1/skills/{name}`, else `POST /v1/skills` on a 404), every workflow
    (the same with the workflow routes), the repositories (rule 19), every
    learned permission (rule 20) and the AI permission settings (rule 21).
    Each item is a push of its own (rule 18). A host item with the same name
    or key is overwritten. An item that only the host has stays as it is.
    The host is `in_sync` when every push has landed.
17. After that, each local domain event of those items is pushed as it
    happens: a created or updated skill or workflow as in rule 16, a deleted
    one with `DELETE` (a 404 counts as done), and any repository event
    with rule 19. The host is `pending` while a push runs. A bus that lags
    is answered with the whole push of rule 16 again.
18. A failed push makes the host `failed`, with the reason of the last
    failure in `error`, while any push stays failed. A failed push holds back
    no other one: the later pushes run, and the failed ones are tried again,
    each on its own, after `host_sync_retry`. The wait is doubled after each
    round that leaves one failed, up to 60 seconds, and reset once none is.
    A later push of the same item ends its failed push, both when it is
    queued and when it runs, so no retry writes an older version over a
    newer one. When the host leaves `ready`, the tries stop and the host is
    `pending` with a null `error`. Its next `ready` starts the push of rule
    16 again.
19. A local repository matches a host repository on the same forge
    repository (kind, host, owner, name). One on the same base branch is
    taken first, else the first one left; each host repository matches once.
    For each match the daemon sends `PUT /v1/repositories/{id}` with the
    local `default_workflow`, `description` (empty for none),
    `permission_mode`, and `base_branch` and `forge.enabled` where they
    differ. The host keeps its own `path`. A local repository with no match
    is in `missing_repositories`. Nothing is registered on the host, and a
    missing checkout leaves `state` as it is.
20. A learned permission is pushed with `POST /v1/permissions/learned` on
    the host repository its repository matches (rule 19). The route writes
    the whole row; a row with the same repository, tool name, level and key
    keeps its id and `created_at`. An unknown repository is a 404. A deleted
    row deletes the host row with the same repository, tool name, level and
    key. A row whose repository has no match is not pushed: it is counted in
    `skipped_permissions`, recounted on each push of a repository or a
    learned permission, and leaves `state` as it is.
21. The AI permission settings are pushed with `PUT /v1/permissions/ai`:
    `enabled`, the thresholds where they are set by hand (else
    `default_thresholds`), and `flavour`. `device` is not pushed: the host
    picks its own best device for the flavour. A status event that changes
    none of these pushes nothing.
22. Writes go to the local daemon. Nothing is read back from a host into
    this daemon.

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
- Rules 4, 7 and 15 (`hosts.rs::a_daemon_without_hosts_lists_only_local`,
  `::configured_hosts_follow_local_in_file_order`,
  `::openapi_carries_the_route_the_dto_and_the_event`).
- Rules 16, 20 and 21
  (`host_sync.rs::a_ready_host_takes_every_item_and_keeps_its_own`).
- Rule 17
  (`host_sync.rs::each_later_change_reaches_the_host_and_a_delete_removes_it`).
- Rule 18
  (`host_sync.rs::a_failed_push_is_failed_with_its_error_and_tried_again_until_it_lands`,
  `::a_refused_push_holds_back_no_later_change`,
  `::a_newer_push_of_an_item_ends_its_older_failed_push`,
  `ssh_forward.rs::a_forward_to_a_socket_path_comes_up_and_goes_down`).
- Rule 19
  (`host_sync.rs::a_repository_takes_the_local_settings_by_forge_and_a_missing_one_is_listed`,
  `src/hosts/sync.rs::a_repository_pairs_by_forge_and_prefers_its_own_base_branch`,
  `::a_forge_repository_on_another_branch_pairs_once`).
- Rule 20
  (`host_sync.rs::a_ready_host_takes_every_item_and_keeps_its_own`,
  `::each_later_change_reaches_the_host_and_a_delete_removes_it`,
  `::a_learned_permission_without_a_checkout_on_the_host_is_counted_as_skipped`;
  the route is spec 022, rule 21).
- Rules 6 and 15
  (`host_sync.rs::each_change_of_the_sync_publishes_one_host_updated`).

The tests run a stub `ssh` (`tests/it/common/ssh.rs`) that relays its `-L`
port to its remote, with a second harness daemon as the far machine. The sync
tests serve the second harness daemon on a loopback port, and give the local
one a `direct` host that points at it.

## Sources

`crates/ariadne-client/src/endpoint.rs`, `crates/ariadne-api/src/hosts.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/hosts/sync.rs`,
`crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`docs/configuration.md`.

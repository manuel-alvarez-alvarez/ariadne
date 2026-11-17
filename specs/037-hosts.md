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
  - crates/ariadne-daemon/tests/it/host_queries.rs
  - crates/ariadne-cli/tests/hosts.rs
  - crates/ariadne-cli/src/commands/query.rs
  - crates/ariadne-daemon/src/hosts.rs
---

# Hosts

## Scope

In: the `[hosts.<name>]` tables of `config.toml`, their validation, the
`GET /v1/hosts` route, `HostDto`, the `host_updated` event, `ssh_bin`, and the
SSH forward the daemon keeps for each `ssh://` host. Out: a Rust SSH library, a
reverse forward from the far daemon and any desktop change. CLI selection
follows spec 014. The transports and the event stream are spec 012's.

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
   `connecting`, `down`); `error`; `remote`.
5. An `ssh` host is `down`, with a null `endpoint` and the error
   `ssh forward not started`, until its forward starts.
6. `host_updated` carries one `HostDto`. The daemon publishes one event for
   each change of a host's `status`, `endpoint` or `error`.
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
      named `host` becomes `exported_host`);
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
`crates/ariadne-daemon/src/hosts.rs`, `crates/ariadne-daemon/src/http/hosts.rs`,
`crates/ariadne-daemon/src/http/query/fan.rs`, `docs/configuration.md`,
`docs/telemetry.md`.

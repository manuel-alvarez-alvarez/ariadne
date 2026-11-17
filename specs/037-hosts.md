---
id: 037
status: current
updated: 2026-11-16
areas: [api, daemon, client, docs]
commits: []
tests:
  - crates/ariadne-client/src/endpoint.rs
  - crates/ariadne-daemon/tests/it/hosts.rs
---

# Hosts

## Scope

In: the `[hosts.<name>]` tables of `config.toml`, their validation, the
`GET /v1/hosts` route, `HostDto` and the `host_updated` event. Out: starting or
keeping an SSH forward, the CLI `--host` flag and any desktop change; later
tasks own them. The transports and the event stream are spec 012's.

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
   `ssh forward not started`, until a later task starts the forward.
6. `host_updated` carries one `HostDto`. The daemon publishes it when a host's
   `status`, `endpoint` or `error` changes. Nothing changes a host yet, so
   nothing publishes it yet.
7. The OpenAPI document carries the route, the DTO and the event.
8. `ariadned --help` lists the `hosts` table.

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

## Known gap

No code publishes `host_updated` yet, so no test sees it on the stream.

## Sources

`crates/ariadne-client/src/endpoint.rs`, `crates/ariadne-api/src/hosts.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/http/hosts.rs`, `docs/configuration.md`.

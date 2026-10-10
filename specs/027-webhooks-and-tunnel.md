---
id: webhooks-and-tunnel
status: current
updated: 2026-10-10
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/webhooks.rs
  - crates/ariadne-daemon/tests/it/tunnel.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/repo.rs
  - crates/ariadne-cli/src/commands/forge.rs
  - ui/src/features/repositories/repositories-page.test.tsx
  - ui/src/components/settings-dialog.test.tsx
  - ui/src/events/dispatch.test.ts
---

# Webhooks and tunnel

Signed forge deliveries wake the repository fetch from [026](026-pull-requests.md).

## Scope

The listener is the first surface the daemon exposes beyond the machine.
A port of its own keeps that surface narrow: two signed routes and nothing of `/v1`.
A tunnel forwards every request to its target port, so the API must use a different port.

In: the listener, authentication, hook registration, public URL handle, fetch modes, visible hook status,
and the localtunnel client that writes the public URL at run time, with its switch.
Out: Node, the `lt` binary, and a tunnel server of Ariadne's own.

The transport is the daemon's alone.
A fetch records the same pull request rows whether a delivery or the timer brought it.
Later news routing reads those rows and nothing else (026 rule 7, `after_fetch`).
So an agent is handed the same news either way.
The session task's tests prove that rule for a prompt.

## Behavior

1. Every daemon starts a webhook listener, including a daemon with no `tcp_listen`.
   `webhook_listen` is an optional socket address in `config.toml`.
   When absent, it binds `127.0.0.1:0`.
   The operating system chooses a free port, avoiding collisions between daemon instances.
   The tunnel needs only the actual bound address, which `WebhookListen::address` supplies.
   The daemon logs that address.
   A reverse proxy can use a fixed configured address.
2. `webhook_public_url` supplies the initial public URL.
   `ForgePoll::webhook_url()` returns a cloned `WebhookUrl` handle over a watch channel.
   `WebhookUrl::set(Some(url))` publishes a changed URL; `set(None)` withdraws it.
   The poll follows that channel and reconciles each integration.
3. The listener serves only `POST /webhooks/github/{repository_id}` and `POST /webhooks/gitlab/{repository_id}`.
   It has no API, documentation, or CORS middleware.
   Bodies larger than 1 MiB receive 413.
   The named integration must exist, be enabled, match the route's forge, and have a secret.
   GitHub checks `X-Hub-Signature-256`, with `sha256=` and HMAC-SHA256 over the raw bytes.
   GitLab compares `X-Gitlab-Token` with the stored secret.
   Both checks use constant-time verification.
   Missing or incorrect credentials receive 401 and cause no write or fetch.
4. An authenticated delivery records `webhook_last_delivery_at` and receives 202.
   It calls `ForgePoll::wake(repository_id)` unless its forge event header says `ping`.
   An authenticated ping records its timestamp but causes no fetch.
   The delivery triggers one fetch instead of applying its payload.
   This keeps one fetch shape for both forges.
   A missed delivery costs nothing more than the next one.
   Concurrent wakes follow the coalescing rule in 026.
5. `forge_integrations` holds `webhook_id`, `webhook_secret`, `webhook_url`, `webhook_state`,
   `webhook_error`, and `webhook_last_delivery_at`. `webhook_state` defaults to `polling`; the rest are nullable.
   Each integration receives 32 cryptographically random bytes, encoded as 64 hexadecimal characters.
   The secret is stored before creation because the forge can immediately send a ping.
   Public DTOs exclude it, and hook errors redact it.
6. An enabled integration with a public URL wants one hook at `<url>/webhooks/<kind>/<repository_id>`.
   Reconciliation creates a missing hook, checks a stored hook at startup, and updates a changed URL.
   GitHub requests `pull_request`, `pull_request_review`, `pull_request_review_comment`, `issue_comment`,
   `check_suite`, `check_run`, `status`, and `issues` events.
   GitLab requests `merge_requests_events`, `note_events`, `pipeline_events`, and `issues_events`, with its token.
   Both forges keep TLS verification enabled.
   A GitHub URL update uses the configuration endpoint to preserve the secret.
   A GitLab URL update supplies its token again.
7. A successful registration or update stores the hook ID and URL and enters `live`.
   The worker enters `WakeOnly` and fetches once to catch earlier changes.
   Startup and enable also fetch once, without a duplicate registration fetch.
   Delivery timestamps and other repository events do not recreate the hook.
8. A creation refusal enters `polling` and preserves the forge's message in `webhook_error`.
   No public URL means `polling`, with the reason the URL was withdrawn as the error, or no error.
   A withdrawn URL clears the public address, restores `Timer`, and wakes one fetch.
   The hook ID remains stored for an update when a public URL returns.
   A stored hook returning 404 during reconciliation clears its ID and URL and enters `polling`.
   That transition also restores `Timer` and wakes one fetch.
   Another update or deletion failure enters `failed`, retaining the hook ID and error for recovery.
   `failed` also uses `Timer`.
   URL changes or a subsequent enable retry reconciliation; no hook registration timer runs.
9. Disabling cancels the worker and deletes the stored hook.
   A missing hook counts as a successful deletion.
   The secret stays private, and a disabled integration rejects deliveries.
   Replacing the forge identity clears its hook metadata and secret atomically.
   Replacement or repository deletion removes the old hook using its original forge coordinates.
   A cleanup failure logs an error; it cannot authorize old deliveries against the new identity.
   Each changed hook status or delivery timestamp emits `repository_updated` under [012](012-http-api-events-and-usage.md).
10. `ForgeDto.webhook` contains `state`, `url`, `error`, `last_delivery_at`, and `fetch_error`.
    `fetch_error` is why the last fetch of the repository's requests failed, and null once one works.
    The poll worker writes it after every fetch, and only a change publishes `repository_updated`.
    `repo inspect` prints the same fields, and missing values appear as `-`.
    The repositories table's Webhook column shows one pill per enabled integration.
    With the tunnel switched on the pill reads `localtunnel`: green while the hook is `live` and the last fetch worked,
    red otherwise, with the tunnel, hook and fetch errors in its tooltip, or the hook URL while green.
    With the tunnel switched off it reads `polling`: green while the last fetch worked, with how to turn the tunnel on
    in Settings in its tooltip, and red with the fetch error otherwise.
    A disabled integration shows `-`. The repository form shows none of the webhook.

| State | Enabled repository fetch mode |
| --- | --- |
| `live` | `WakeOnly`: activation fetch, then explicit wakes |
| `polling` | `Timer`: immediate fetch and fallback timer |
| `failed` | `Timer`: immediate fetch and fallback timer |

A disabled integration has no worker, regardless of its retained hook status.

### The tunnel

11. The one `forge_settings` row holds `tunnel_enabled` (0 or 1, default 1) and `tunnel_subdomain` (NULL until first use).
    The schema seeds it.
    `config.toml` adds `tunnel_host` (default `https://localtunnel.me`) and `tunnel_subdomain`.
    The configured subdomain wins over the stored one.
12. The tunnel runs when `tunnel_enabled` holds, at least one integration is enabled, and `webhook_public_url` is unset.
    A set `webhook_public_url` opens no tunnel, whatever the switch, and the tunnel never touches that URL.
13. The daemon opens the tunnel with the `localtunnel-client` crate, without Node or `lt`.
    It reads the bound listener address from `WebhookListen::address`.
    It asks for the configured subdomain, else the stored one.
    Else it picks a random word and six digits and stores it before the request.
    So the URL survives a restart where the server grants the subdomain again.
14. A loopback relay sits between the client and the listener.
    It listens on `127.0.0.1` only and forwards each connection to the bound listener address and nowhere else.
    The crate reports no dropped tunnel, so the relay counts the open tunnel connections.
    Each tunnel connection opens one relay connection at once, so the count is the client's live connections.
15. One connection attempt registers with `tunnel_host` and waits for the first tunnel connection.
    `Timeouts::tunnel_connect` (default 15 s) bounds the attempt.
    A success writes the URL into `WebhookUrl`, and the tunnel is `up`.
    The hook reconcile (rules 6 and 7) then creates or updates each hook, enters `live` and `WakeOnly`, and fetches once.
16. The tunnel is `down` when an attempt fails, or when an `up` tunnel holds no open connection for `tunnel_connect`.
    Down withdraws the URL with the reason "tunnel down since <time>".
    The hook reconcile then sets each integration to `polling` with that `webhook_error`, restores `Timer`, and fetches once.
    The time is the moment the tunnel went down, and it stays the same across retries.
    One outage clock runs for the whole time the tunnel is up: an unrelated repository event does not restart it.
17. A down tunnel reconnects with a capped backoff, forever.
    The first wait is `Timeouts::tunnel_retry` (default 1 s), and each failed attempt doubles it, up to 60 s.
    A good attempt resets the wait.
    Back up, the URL returns; the reconcile updates a changed hook URL, enters `live` and `WakeOnly`,
    and fetches once to catch the deliveries lost while down.
18. `PUT /v1/forge/tunnel { enabled }` writes the switch and answers the `ForgeTunnelDto`.
    Off closes the tunnel and withdraws the URL with the reason "tunnel off".
    Each integration then reads `polling` with that error and runs on `Timer`.
    The hooks stay registered with the old URL, so a switch creates or deletes nothing on the forge.
    On opens the tunnel, and the reconcile updates each hook's URL.
    A switch off cancels a pending connection attempt.
    An attempt that succeeds after the switch went off publishes no URL, so no hook moves.
    The switch survives a restart.
19. `GET /v1/forge/tunnel` answers `{ enabled, state, url, listen, since, error }`.
    `state` is `up`, `down` or `off`, and `listen` is the bound listener address.
    `off` covers the switch off, a set `webhook_public_url`, and no enabled integration.
    Each switch write and each state change publishes `forge_settings_updated` with the same DTO (012).
20. `ariadne forge tunnel` prints the tunnel, and `ariadne forge tunnel on` and `off` set the switch.
    `repo inspect` prints a `tunnel` line under the webhook block.
    The desktop settings dialog shows the switch, a description of what the tunnel does, and its state: up with the
    URL, down with the error, or off. The switch is written when flipped, not on the daemon URL
    field's Save.
21. The shutdown signal closes the tunnel and the webhook listener at once.
    The HTTP drain, which an open event stream can hold, comes after.

| Tunnel state | Hook state of each enabled integration | Fetch mode |
| --- | --- | --- |
| `up` | `live` once the reconcile succeeds | `WakeOnly` |
| `down` | `polling`, error "tunnel down since <time>" | `Timer` |
| `off` by the switch | `polling`, error "tunnel off" | `Timer` |

## Acceptance criteria

- Random and fixed bindings stay separate from the API and expose no API or docs:
  `webhooks.rs::the_listener_binds_a_separate_random_or_fixed_port_and_serves_no_api`.
- Configuration reads both keys and defaults to a random loopback port:
  `config.rs::tests::webhook_configuration_uses_a_random_port_unless_an_address_is_given`.
- GitHub checks raw body signatures, event selection, secret generation, delivery timestamps, and the body cap:
  `webhooks.rs::github_signed_deliveries_wake_once_and_bad_signatures_ping_and_large_bodies_do_not`.
- GitLab checks tokens, event selection, escaped project paths, timestamps, and the body cap:
  `webhooks.rs::gitlab_tokens_wake_once_and_bad_tokens_ping_and_large_bodies_do_not`.
  Both delivery tests prove one initial fetch and no timer fetch while live.
- Changed URLs update hooks, withdrawal wakes a fetch, and disable deletes:
  `webhooks.rs::url_changes_update_hooks_withdrawal_restores_timer_and_disable_deletes`.
- GitLab URL updates preserve the token and withdrawal restores timer fetching:
  `webhooks.rs::gitlab_url_updates_preserve_the_token_and_withdrawal_restores_timer`.
- Creation refusal retains the forge's message and timer fetches:
  `webhooks.rs::a_refused_hook_keeps_the_forge_message_and_timer_fetches`.
- Startup checks a stored hook without another creation and fetches once:
  `webhooks.rs::startup_checks_a_stored_hook_without_creating_another_and_fetches_once`.
- A missing stored hook restores timer fetching:
  `webhooks.rs::a_missing_stored_hook_restores_timer_and_wakes_a_fetch`.
- Another update failure retains the ID, redacts the secret, and restores timer fetching:
  `webhooks.rs::a_failed_hook_update_keeps_its_id_and_reports_the_error_with_timer_fallback`.
- Replacing a remote deletes the old hook and generates a new secret on enable:
  `webhooks.rs::a_changed_remote_deletes_the_old_hook_without_reusing_its_secret_or_id`.
- Replacing and enabling a remote in one edit creates its own hook:
  `webhooks.rs::replacing_and_enabling_a_remote_in_one_edit_creates_its_own_hook`.
- CLI inspection includes the webhook block:
  `repo.rs::tests::repo_inspect_prints_the_forge_block_with_the_login`.
- The table shows `localtunnel` green with the URL, and red with the error:
  `repositories-page.test.tsx::shows localtunnel green while the hook is live, and red with the error when it is not`.
  With the tunnel off it shows `polling`, green with how to turn the tunnel on and red with the fetch error:
  `repositories-page.test.tsx::shows polling with the tunnel off: green with how to turn it on, red with the fetch error`.
- The last fetch error is written only when it changes:
  `store.rs::the_last_fetch_error_is_written_only_when_it_changes`.
- With one enabled integration, the switch on and no public URL, the tunnel opens to the stand-in at the random
  listener port, and the hook is created with the stand-in's URL. A signed delivery to that URL reaches the listener
  and triggers one fetch:
  `tunnel.rs::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`.
- A stand-in that closes flips the integration to `polling` with the error and `Timer`.
  One that answers again flips it to `live` and `WakeOnly` with one catch-up fetch and no fetch after it:
  Edits to another repository during the outage do not hold off the flip:
  `tunnel.rs::a_server_that_goes_away_restores_the_timer_and_one_that_returns_goes_live_with_one_catch_up_fetch`.
  A fetch that runs when the hook goes live coalesces the catch-up wake into one more fetch (026).
- A switch off during a held registration cancels it, and no URL or hook follows its release:
  `tunnel.rs::a_switch_off_during_registration_cancels_it_and_publishes_no_url`.
- SIGTERM closes the tunnel and the listener while an open event stream holds the HTTP drain:
  `tunnel.rs::shutdown_closes_the_tunnel_and_the_listener_while_an_event_stream_holds_the_drain`.
- The switch off closes the tunnel, gives "tunnel off" and `Timer`, publishes `forge_settings_updated`,
  and makes no hook call. On returns to `live` and `WakeOnly`. The switch survives a restart:
  `tunnel.rs::the_switch_moves_the_fetch_without_touching_the_hooks_and_survives_a_restart`.
- The configured subdomain is requested, and the stored one again after a restart:
  `tunnel.rs::the_configured_subdomain_is_asked_for_and_the_stored_one_again_after_a_restart`.
- A public URL or no enabled integration opens no tunnel:
  `tunnel.rs::a_public_url_or_no_enabled_integration_opens_no_tunnel`.
- A delivery and a timer fetch record the same pull request rows:
  `tunnel.rs::a_delivery_and_a_timer_fetch_record_the_same_pull_request_rows`.
- Configuration reads the tunnel keys and their defaults:
  `config.rs::tests::webhook_configuration_uses_a_random_port_unless_an_address_is_given`.
- `ariadne forge tunnel` prints the state and the bound address, and `on` and `off` set the switch:
  `forge.rs::tests::the_tunnel_prints_its_state_url_and_bound_address`,
  `forge.rs::tests::forge_tunnel_on_and_off_set_the_switch`.
- `repo inspect` prints the tunnel line:
  `repo.rs::tests::repo_inspect_prints_the_forge_block_with_the_login`.
- The settings dialog describes the tunnel, shows its state, and its switch sets it:
  `settings-dialog.test.tsx::SettingsDialog > says what the tunnel is for, shows its state, and its switch turns it off and on`,
  `::says why a tunnel that is on is down`.
- `forge_settings_updated` replaces the cached tunnel:
  `dispatch.test.ts::replaces the cached tunnel whole, so a screen that read up reads off`.

## Sources

`crates/ariadne-daemon/src/webhooks.rs`, `crates/ariadne-daemon/src/forge/hooks.rs`,
`crates/ariadne-daemon/src/forge/github/hooks.rs`, `crates/ariadne-daemon/src/forge/gitlab/hooks.rs`,
`crates/ariadne-store/migrations/0001_init.sql`, `crates/ariadne-store/src/webhooks.rs`,
`crates/ariadne-daemon/src/forge/tunnel.rs`, `crates/ariadne-daemon/src/http/forge.rs`.

The tunnel protocol follows `localtunnel-client` 0.1.8 (<https://github.com/kaichaosun/rlt>, MIT):
one HTTPS request answers an id, a URL, a port and a connection count,
and the client proxies each TCP connection to that port to the local address.

Hook request shapes follow the [GitHub webhook API](https://docs.github.com/en/rest/repos/webhooks)
and [GitLab project webhook API](https://docs.gitlab.com/api/project_webhooks/).

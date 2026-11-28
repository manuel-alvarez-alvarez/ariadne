---
id: webhooks-and-tunnel
status: current
updated: 2026-11-28
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/webhooks.rs
  - crates/ariadne-daemon/tests/it/forge_rate_limit.rs
  - crates/ariadne-daemon/tests/it/forge_host_limit.rs
  - crates/ariadne-daemon/tests/it/tunnel.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-daemon/src/forge/hooks.rs
  - crates/ariadne-store/src/lib.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/repo.rs
  - crates/ariadne-cli/src/commands/forge.rs
  - ui/src/features/repositories/repositories-page.test.tsx
  - ui/src/features/repositories/repository-form-dialog.test.tsx
  - ui/src/components/settings-dialog.test.tsx
  - ui/src/events/dispatch.test.ts
---

# Webhooks and tunnel

Signed forge deliveries wake the repository fetch from [026](026-pull-requests.md).

## Scope

The listener is the first surface the daemon exposes beyond the machine.
A port of its own keeps that surface narrow: one signed route and nothing of `/v1`.
A tunnel forwards every request to its target port, so the API must use a different port.

In: the listener, authentication, hook registration, public URL handle, fetch modes, visible hook status,
and the smee.io client that writes the public URL at run time, with its switch.
Out: Node, the `smee` binary, and a tunnel server of Ariadne's own.

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
3. The listener serves only `POST /webhooks/github/{repository_id}`.
   It has no API, documentation, or CORS middleware.
   Bodies larger than 1 MiB receive 413.
   The named integration must exist, be enabled, match the route's forge, and have a secret.
   GitHub checks `X-Hub-Signature-256`, with `sha256=` and HMAC-SHA256 over the raw bytes,
   by constant-time verification.
   Missing or incorrect credentials receive 401 and cause no write or fetch.
4. An authenticated delivery records `webhook_last_delivery_at` and receives 202.
   It calls `ForgePoll::wake(repository_id)` unless `X-Github-Event` says `ping`.
   An authenticated ping records its timestamp but causes no fetch.
   The delivery triggers one fetch instead of applying its payload.
   A missed delivery costs nothing more than the next one.
   Concurrent wakes follow the coalescing rule in 026; a burst of deliveries for one push — `check_run`, `check_suite` and `status` events alike — folds into that rule's one fixed debounce window, so it costs one fetch rather than one per delivery.
5. `forge_integrations` holds `webhook_id`, `webhook_secret`, `webhook_url`, `webhook_state`,
   `webhook_error`, `webhook_last_delivery_at`, and `webhook_rights_refused` (0 or 1, default 0). `webhook_state` defaults to `polling`; the rest are nullable.
   Each integration receives 32 cryptographically random bytes, encoded as 64 hexadecimal characters.
   The secret is stored before creation because the forge can immediately send a ping.
   Public DTOs exclude it, and hook errors redact it.
6. An enabled integration with a public URL wants one hook at `<url>/webhooks/<kind>/<repository_id>`.
   A smee.io channel URL serves its own path only, so through the tunnel the route goes in the query instead:
   `<channel url>?route=/webhooks/<kind>/<repository_id>`.
   Reconciliation creates a missing hook, checks a stored hook at startup, and updates a changed URL.
   GitHub requests `pull_request`, `pull_request_review`, `pull_request_review_comment`, `issue_comment`,
   `check_suite`, `check_run`, `status`, and `issues` events, with TLS verification enabled.
   A GitHub URL update uses the configuration endpoint to preserve the secret.
   Every hook request — creation and the configuration update alike — sends
   its JSON body to `gh api` on standard input (`--input -`), the secret
   included, never as a command-line argument a process list could read.
7. A successful registration or update stores the hook ID and URL and enters `live`.
   The worker enters `WakeOnly` and fetches once to catch earlier changes.
   Startup and enable also fetch once, without a duplicate registration fetch.
   Delivery timestamps and other repository events do not recreate the hook.
8. A creation refusal enters `polling` and preserves the forge's message in `webhook_error`, with the secret redacted.
   A creation refusal is a rights refusal when the forge answers 404 or names a missing scope or admin rights.
   `gh` gives "Not Found (HTTP 404)" with a hint about the `admin:repo_hook` scope.
   A rights refusal also sets `webhook_rights_refused`, and every other reconcile result clears it.
   While it holds with the integration enabled and its switch on (rule 22),
   each timer fetch first retries the registration through the same reconcile.
   A retry that succeeds enters `live` and `WakeOnly` with no restart, and that timer fetch is its catch-up fetch.
   A retry with the URL withdrawn creates no hook. Other refusals get no retry.
   No public URL means `polling`, with the reason the URL was withdrawn as the error, or no error.
   A withdrawn URL clears the public address, restores `Timer`, and wakes one fetch.
   The hook ID remains stored for an update when a public URL returns.
   A stored hook returning 404 during reconciliation clears its ID and URL and enters `polling`.
   That transition also restores `Timer` and wakes one fetch.
   Another update or deletion failure enters `failed`, retaining the hook ID and error for recovery.
   `failed` also uses `Timer`.
   URL changes or a subsequent enable retry reconciliation; only a rights refusal is retried on the timer.
9. Disabling cancels the worker and deletes the stored hook.
   A missing hook counts as a successful deletion.
   The secret stays private, and a disabled integration rejects deliveries.
   Replacing the forge identity clears its hook metadata and secret atomically.
   Replacement or repository deletion removes the old hook using its original forge coordinates.
   A cleanup failure logs an error; it cannot authorize old deliveries against the new identity.
   Each changed hook status or delivery timestamp emits `repository_updated` under [012](012-http-api-events-and-usage.md).
10. `ForgeDto.webhook` contains `state`, `url`, `error`, `last_delivery_at`, `fetch_error`, and `rights_refused`.
    `rights_refused` is true while the last registration was a rights refusal (rule 8).
    `fetch_error` is why the last fetch of the repository's requests failed, and null once one works.
    While a rate limit pauses the worker (026 rule 6), it reads `rate limited until <RFC 3339 time>` instead, whatever delivered the wake that found it.
    The poll worker writes it after every fetch, and only a change publishes `repository_updated`.
    `repo inspect` prints the same fields, and missing values appear as `-`.
    The repositories table's Webhook column shows one pill per enabled integration.
    With the tunnel switched on the pill reads `smee`: green while the hook is `live` and the last fetch worked,
    red otherwise, with the tunnel, hook and fetch errors in its tooltip, or the hook URL while green.
    With the tunnel switched off it reads `polling`: green while the last fetch worked, with how to turn the tunnel on
    in Settings in its tooltip, and red with the fetch error otherwise.
    A disabled integration shows `-`.
    A repository whose webhook is off (rule 22) reads `polling` whatever the tunnel does: green while the last fetch worked,
    with a tooltip that says the webhook is off for this repository and gives its interval.
    A rights refusal with the switch on reads `polling` whatever the tunnel does: amber while the last fetch worked,
    red otherwise. Its tooltip says a webhook needs admin rights on the repository and the `admin:repo_hook` scope
    on the gh token, gives `gh auth refresh -h <host> -s admin:repo_hook`,
    and says to turn off the webhook for this repository to stop the retries.
    The repository form shows the switch and the interval for an enabled integration only.

| State | Enabled repository fetch mode |
| --- | --- |
| `live` | `WakeOnly`: activation fetch, then explicit wakes |
| `polling` | `Timer`: immediate fetch and fallback timer |
| `failed` | `Timer`: immediate fetch and fallback timer |

A disabled integration has no worker, regardless of its retained hook status.

### The webhook switch and the poll interval

22. `forge_integrations` also holds `webhook_enabled` (0 or 1, default 1, so an existing row keeps its hook)
    and `poll_interval_minutes` (whole minutes from 1 to 60, NULL for the default of 5).
    `ForgeDto` carries `webhook_enabled` and `poll_interval_minutes`.
    `ForgeUpdate` carries `webhook` and `poll_interval_minutes`; an absent field is unchanged, and a null interval clears it.
    An interval outside 1 to 60 is refused with 400 `invalid_request`, and nothing is written.
    A forge change of identity keeps neither value.
23. With the switch off, an enabled integration wants no hook, whatever the public URL.
    Reconciliation deletes a hook it registered (a missing hook counts as deleted), clears the hook ID and URL,
    and enters `polling` with no `webhook_error`. The worker uses `Timer`.
    A failed deletion enters `failed` with the error, as in rule 8.
    With the switch on again, reconciliation registers the hook as for a new enable (rules 6 and 7).
24. The `Timer` wait of a repository is its `poll_interval_minutes`, else `Timeouts::forge_poll` (5 minutes).
    It applies while the integration is `polling` or `failed`; a `live` hook has no timer.
    The failure backoff doubles from that wait, and a rate-limit pause is never shorter than it.
    A write of the switch or the interval reaches the running worker with the repository update event,
    so it needs no restart. A new interval restarts the wait in progress.
25. `ariadne repo add` and `repo update` take `--webhook on|off`, `--poll-interval MINUTES` (1 to 60)
    and `--reset-poll-interval`. `repo inspect` prints `webhook` (`on` or `off`) and `poll interval`.

### The tunnel

11. The one `forge_settings` row holds `tunnel_enabled` (0 or 1, default 0) and `tunnel_channel` (NULL until first use).
    `0001`'s own seed writes the row with the tunnel off, and that seed runs exactly once, ever, so every fresh installation starts polling, by default. `PUT /v1/forge/tunnel` is the only other place that writes the column.
    `0013` renames `0001`'s `tunnel_subdomain` to `tunnel_channel` and clears it: a guessable localtunnel subdomain must not become a public channel.
    `config.toml` adds `tunnel_host` (default `https://smee.io`) and `tunnel_channel`.
    The configured channel wins over the stored one.
    `tunnel_subdomain` is still accepted in `config.toml`, so an old configuration loads, and nothing reads it.
12. The tunnel runs when `tunnel_enabled` holds, at least one integration is enabled, and `webhook_public_url` is unset.
    A set `webhook_public_url` opens no tunnel, whatever the switch, and the tunnel never touches that URL.
13. The daemon listens on a smee.io channel itself, with reqwest and the client's SSE parser, without Node or `smee`.
    It reads the bound listener address from `WebhookListen::address`.
    It listens on the configured channel, else the stored one.
    Else it picks 22 random letters and digits and stores them before the request.
    So the URL survives a restart. A channel is public to anyone who knows it, so the name is its only secret.
14. The channel streams each delivery as one event: the forge's headers, lowercased, beside `body` and `query`.
    The tunnel posts it on to the bound listener address and nowhere else, at the `query.route` the hook URL named.
    That post never goes through a proxy: `HTTP_PROXY` and its kin carry the channel's stream, not the loopback post.
    A route that does not start with `/webhooks/` is dropped with a warning.
    It passes on `Content-Type`, `X-Github-Event`, `X-Github-Delivery` and `X-Hub-Signature-256`.
    The body is the event's `body` text as smee.io wrote it, unparsed.
    smee.io parsed the forge's body and wrote it back compact, so the signature holds only where that
    reproduces the forge's bytes. GitHub sends compact JSON, so it does. A body that comes back different
    fails rule 3's check, and that delivery is lost; the tunnel logs the listener's refusal.
15. One connection attempt opens the channel's event stream at `<tunnel_host>/<channel>` and waits for its `ready` event.
    `Timeouts::tunnel_connect` (default 15 s) bounds the attempt.
    A success writes the channel URL into `WebhookUrl`, with rule 6's query route, and the tunnel is `up`.
    The hook reconcile (rules 6 and 7) then creates or updates each hook, enters `live` and `WakeOnly`, and fetches once.
16. The tunnel is `down` when an attempt fails, when the stream ends or fails,
    or when an `up` stream carries no event, not even smee.io's `ping`, for `Timeouts::tunnel_idle` (default 90 s).
    Down withdraws the URL with the reason "tunnel down since <time>".
    The hook reconcile then sets each integration to `polling` with that `webhook_error`, restores `Timer`, and fetches once.
    The time is the moment the tunnel went down, and it stays the same across retries.
    One silence clock runs for the whole time the tunnel is up: an unrelated repository event does not restart it.
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
    The HTTP drain comes after. Open event streams end with the shutdown, so they do not hold it (016, rule 21).

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
- GitHub checks raw body signatures, event selection, secret generation, delivery timestamps, and the body cap,
  proves one initial fetch and no timer fetch while live, and proves the
  secret and the rest of the hook body reach `gh` on standard input rather
  than as an argument:
  `webhooks.rs::github_signed_deliveries_wake_once_and_bad_signatures_ping_and_large_bodies_do_not`.
- A burst of wakes folds into one debounce fetch without continued deliveries
  moving its deadline, and a rate-limited fetch remains visible as
  `fetch_error` until its Timer or WakeOnly recovery fetch works:
  `forge_rate_limit.rs::three_wakes_inside_the_debounce_window_cause_one_fetch`,
  `forge_rate_limit.rs::continued_wakes_do_not_extend_the_debounce_window`,
  `forge_rate_limit.rs::a_rate_limited_timer_fetch_pauses_and_fetches_once_after_wakes_inside_it`,
  `forge_rate_limit.rs::a_rate_limited_wake_only_fetch_pauses_and_fetches_once_after_wakes_inside_it`,
  `forge_rate_limit.rs::a_past_rate_limit_reset_waits_for_the_poll_interval`.
- Changed URLs update hooks, withdrawal wakes a fetch, and disable deletes:
  `webhooks.rs::url_changes_update_hooks_withdrawal_restores_timer_and_disable_deletes`.
- A refusal that names no rights retains the forge's message, timer fetches, and gets no retry:
  `webhooks.rs::a_refused_hook_keeps_the_forge_message_and_timer_fetches`.
- A create refusal is a rights refusal on 404, the hook scope or admin rights, and on nothing else:
  `hooks.rs::tests::a_create_refusal_for_rights_names_404_the_hook_scope_or_admin_rights`.
- A rights refusal polls with the redacted message and `rights_refused`, each timer fetch retries it,
  and a retry that works goes `live` and `WakeOnly` with no restart:
  `webhooks.rs::a_rights_refusal_polls_and_a_timer_retry_goes_live_without_a_restart`.
- A rights refusal retries no hook with the URL withdrawn, the switch off, or the integration disabled:
  `webhooks.rs::a_rights_refusal_retries_no_hook_with_the_url_withdrawn_the_switch_off_or_disabled`.
- The row stores a rights refusal, and a new forge identity clears it:
  `store.rs::a_rights_refusal_is_stored_and_a_new_forge_identity_clears_it`.
- The table shows amber `polling` on a rights refusal, with both causes, the refresh command and the switch:
  `repositories-page.test.tsx::shows amber polling when the forge refused the hook for rights, and says how to fix it`.
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
- An existing row keeps the webhook on with no interval, and the column refuses an interval outside 1 to 60:
  `store.rs::a_forge_row_defaults_to_webhook_on_with_no_interval_and_bounds_the_interval`.
- The switch off deletes the hook, polls with no error and keeps the integration enabled, and on registers a hook again:
  `webhooks.rs::a_webhook_switched_off_deletes_the_hook_polls_without_error_and_registers_again_when_on`.
  A repository created with the switch off creates no hook:
  `webhooks.rs::a_switched_off_webhook_registers_no_hook_for_a_new_repository`.
- The API refuses an interval outside 1 to 60, and a stored interval moves the running timer and its reset restores the default:
  `webhooks.rs::the_poll_interval_is_bounded_stored_and_moves_the_timer_without_a_restart`.
- The CLI sets the switch and the interval and resets the interval, and refuses one outside 1 to 60:
  `repo.rs::tests::repo_update_sets_the_webhook_switch_and_the_poll_interval_or_resets_it`.
  `repo inspect` prints both:
  `repo.rs::tests::repo_inspect_prints_the_webhook_switch_and_the_poll_interval`.
- The table shows green `polling` with the webhook off, and its tooltip says so with the interval:
  `repositories-page.test.tsx::shows green polling with the webhook off, and says so with the interval`.
- The form shows the switch and the interval of an enabled integration only, sends what changed, and refuses an interval outside 1 to 60:
  `repository-form-dialog.test.tsx::shows the webhook switch and interval of an enabled integration, and sends what changed`,
  `repository-form-dialog.test.tsx::refuses an interval outside 1 to 60`,
  `repository-form-dialog.test.tsx::shows no webhook fields for a disabled integration`.
- CLI inspection includes the webhook block:
  `repo.rs::tests::repo_inspect_prints_the_forge_block_with_the_login`.
- The table shows `smee` green with the URL, and red with the error:
  `repositories-page.test.tsx::shows smee green while the hook is live, and red with the error when it is not`.
  With the tunnel off it shows `polling`, green with how to turn the tunnel on and red with the fetch error:
  `repositories-page.test.tsx::shows polling with the tunnel off: green with how to turn it on, red with the fetch error`.
- The last fetch error is written only when it changes:
  `store.rs::the_last_fetch_error_is_written_only_when_it_changes`.
- A fresh database starts the tunnel switch off; reopening it afterward,
  whichever choice it already holds, on or off, keeps it untouched, since
  0001's seed never runs a second time:
  `store.rs::a_fresh_database_starts_the_tunnel_off_and_an_existing_one_keeps_its_choice`.
  A database left with an empty `_sqlx_migrations` table by a run killed
  before 0001 ever committed still runs it, tunnel-off seed included:
  `store.rs::a_database_with_an_empty_migrations_table_still_runs_every_migration`.
  A fresh installation, polling by default, still fetches its enabled
  repositories by timer, unaffected by the switch:
  `pull_requests.rs::timer_wake_only_and_disable_control_repository_fetches`.
- With one enabled integration, the switch on and no public URL, the tunnel opens to the stand-in at the random
  listener port, and the hook is created with the stand-in's channel URL and its query route. A signed compact delivery
  to that URL, with non-ASCII text, reaches the listener and triggers one fetch. A spaced body the stand-in writes back
  compact, as smee.io does, fails the signature and records no delivery and no fetch:
  `tunnel.rs::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`.
- A stand-in that closes flips the integration to `polling` with the error and `Timer`.
  One that answers again flips it to `live` and `WakeOnly` with one catch-up fetch and no fetch after it:
  Edits to another repository during the outage do not hold off the flip:
  `tunnel.rs::a_server_that_goes_away_restores_the_timer_and_one_that_returns_goes_live_with_one_catch_up_fetch`.
  A fetch that runs when the hook goes live coalesces the catch-up wake into one more fetch (026).
- A stream that stays open but stops its pings for `tunnel_idle` makes the tunnel `down`:
  `tunnel.rs::a_silent_server_counts_as_down`.
- A switch off during a held registration cancels it, and no URL or hook follows its release:
  `tunnel.rs::a_switch_off_during_registration_cancels_it_and_publishes_no_url`.
- SIGTERM closes the tunnel and the listener, and ends an open event stream:
  `tunnel.rs::shutdown_closes_the_tunnel_and_the_listener_and_ends_an_open_event_stream`.
- The switch off closes the tunnel, gives "tunnel off" and `Timer`, publishes `forge_settings_updated`,
  and makes no hook call. On returns to `live` and `WakeOnly`. The switch survives a restart:
  `tunnel.rs::the_switch_moves_the_fetch_without_touching_the_hooks_and_survives_a_restart`.
- The configured channel is listened on, and the stored random one again after a restart:
  `tunnel.rs::the_configured_channel_is_asked_for_and_the_stored_one_again_after_a_restart`.
- With `HTTP_PROXY` set to a dead proxy and the channel host in `NO_PROXY`, a delivery still reaches the listener:
  `tunnel.rs::a_proxy_in_the_environment_never_carries_the_post_to_the_listener`.
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
`crates/ariadne-daemon/src/forge/github/hooks.rs`,
`crates/ariadne-store/migrations/0001_init.sql`, `crates/ariadne-store/migrations/0013_tunnel_channel.sql`,
`crates/ariadne-store/migrations/0035_forge_webhook_rights_refused.sql`,
`crates/ariadne-store/src/webhooks.rs`,
`crates/ariadne-daemon/src/forge/tunnel.rs`, `crates/ariadne-daemon/src/http/forge.rs`.

The tunnel protocol follows smee.io (<https://github.com/probot/smee.io>) as it answered on 2026-11-21:
`GET /<channel>` with `Accept: text/event-stream` streams a `ready` event, then a `ping` about every 30 s,
and each `POST /<channel>` as an unnamed event of the headers, lowercased, beside the parsed `body` and the `query`.
A posted body with a `\u00e9` escape, a `1.0` and extra spaces came back as `é`, `1` and none.
Any channel name is accepted, and a path below the channel answers 404.

Hook request shapes follow the [GitHub webhook API](https://docs.github.com/en/rest/repos/webhooks).

Signature verification pairs `hmac` 0.13 with `sha2` 0.11, which share the
`digest` 0.11 traits. The signed-delivery test above generates its signature
with Python's independent HMAC implementation.

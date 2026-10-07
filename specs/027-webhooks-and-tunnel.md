---
id: webhooks-and-tunnel
status: current
updated: 2026-10-08
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/webhooks.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/repo.rs
  - ui/src/features/repositories/repositories-page.test.tsx
  - ui/src/features/repositories/repository-form-dialog.test.tsx
---

# Webhooks and tunnel

Signed forge deliveries wake the repository fetch from [026](026-pull-requests.md).

## Scope

The listener is the first surface the daemon exposes beyond the machine.
A port of its own keeps that surface narrow: two signed routes and nothing of `/v1`.
A tunnel forwards every request to its target port, so the API must use a different port.

In: the listener, authentication, hook registration, public URL handle, fetch modes, and visible hook status.
Out: the tunnel that supplies a public URL automatically.
A later task extends this spec with that tunnel.

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
5. The additive migration adds `webhook_id`, `webhook_secret`, `webhook_url`, `webhook_state`,
   `webhook_error`, and `webhook_last_delivery_at` to `forge_integrations`.
   Existing rows default to `polling`, with nullable metadata.
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
   No public URL means `polling` with no error.
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
10. `ForgeDto.webhook` contains `state`, `url`, `error`, and `last_delivery_at`.
    The repository table and form show all four fields read-only.
    `repo inspect` prints the same fields.
    Missing values appear as `-`.

| State | Enabled repository fetch mode |
| --- | --- |
| `live` | `WakeOnly`: activation fetch, then explicit wakes |
| `polling` | `Timer`: immediate fetch and fallback timer |
| `failed` | `Timer`: immediate fetch and fallback timer |

A disabled integration has no worker, regardless of its retained hook status.

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
- Migration preserves existing integrations and a restorable backup:
  `store.rs::webhook_migration_preserves_existing_integrations_and_a_recoverable_backup`.
- CLI inspection includes the webhook block:
  `repo.rs::tests::repo_inspect_prints_the_forge_block_with_the_login`.
- The table shows all four facts:
  `repositories-page.test.tsx::shows webhook state URL error and last delivery`.
- The form shows all four facts read-only:
  `repository-form-dialog.test.tsx::shows every webhook field without editable controls`.

## Sources

`crates/ariadne-daemon/src/webhooks.rs`, `crates/ariadne-daemon/src/forge/hooks.rs`,
`crates/ariadne-daemon/src/forge/github/hooks.rs`, `crates/ariadne-daemon/src/forge/gitlab/hooks.rs`,
`crates/ariadne-store/migrations/0008_webhooks.sql`, `crates/ariadne-store/src/webhooks.rs`.

Hook request shapes follow the [GitHub webhook API](https://docs.github.com/en/rest/repos/webhooks)
and [GitLab project webhook API](https://docs.gitlab.com/api/project_webhooks/).

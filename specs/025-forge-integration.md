---
id: forge-integration
status: current
updated: 2026-10-07
areas: [core, api, store, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/forge_integration.rs
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/repo.rs
  - ui/src/features/repositories/repository-form-dialog.test.tsx
  - ui/src/features/repositories/repositories-page.test.tsx
---

# Forge integration

Which forge a repository's remote is on, and whether Ariadne works with it.
The daemon reads the forge off the checkout's remote. The user enables the
integration and pins the model of each of its two roles. Ariadne speaks to
the forge only through the forge's own CLI, `gh` for GitHub and `glab` for
GitLab.

## Scope

In: the detection of the remote, the `forge_integrations` row, the switch,
the two pins, the forge CLI client, the `gh_bin` and `glab_bin` keys, and the
command-line and desktop surfaces over them.

Out: what a repository is (002), how a task ends and the forge it publishes
to (005), how a pin is checked (011), the command tree as a whole (014), and
the desktop app as a whole (015).

## Behavior

1. A repository has at most one forge row, in the table
   `forge_integrations`, keyed by `repository_id`. The row holds `kind`
   (`github` or `gitlab`), `host`, `owner`, `name`, `remote`, `enabled`,
   `login`, `babysit_model`, `babysit_effort`, `review_model`,
   `review_effort`, `detected_at` and `updated_at`. `host`, `owner` and
   `name` are stored lower-cased. A repository with no usable remote has no
   row. Deleting the repository deletes its row.
2. The remote is `origin`. Where there is no `origin` and the checkout has
   exactly one remote, that remote stands in. Any other checkout has no
   usable remote.
3. The daemon reads the URL with `git remote get-url <remote>`. It reads the
   SSH form (`git@host:owner/name`, `ssh://git@host[:port]/owner/name`) and
   the HTTPS form (`https://host/owner/name`), with or without `.git` and a
   trailing slash. The last path segment is `name`; the segments before it
   are `owner`, so a GitLab group path such as `group/sub` is one owner. A
   local path or a `file://` URL names no forge.
4. Host `github.com` is `github`, and host `gitlab.com` is `gitlab`. Another
   host is `github` when `gh auth status --hostname <host>` exits 0, else
   `gitlab` when `glab auth status --hostname <host>` exits 0, else the
   repository has no forge.
5. The daemon detects the forge at registration, on every
   `PUT /v1/repositories/{id}`, and once for every repository at daemon
   start, off the request path. A detection writes only what changed: a row
   for a remote that appeared, none for one that went away, and the new
   remote for one that moved. A remote that now names another forge
   repository disables the row and clears its login, and keeps its pins: the
   sign-in and rule 9 were checked against the old one.
6. Every write of the row publishes `repository_updated`, carrying the
   repository with its forge (012). There is no event kind of its own.
7. `RepositoryDto.forge` is a `ForgeDto` — `kind`, `host`, `owner`, `name`,
   `remote`, `enabled`, `login`, `babysit_model`, `babysit_effort`,
   `review_model`, `review_effort` — or null where the repository has no row.
   Goals carry their repositories with the same `forge`.
8. `CreateRepositoryRequest` and `UpdateRepositoryRequest` take `forge`, a
   `ForgeUpdate` of `enabled`, `babysit_model`, `babysit_effort`,
   `review_model` and `review_effort`. An absent field is unchanged. A forge
   change on a repository with no row is refused with 409
   `forge_unavailable`. A refusal writes nothing. The repository write and
   the forge write are one store transaction, and the request publishes one
   event after the commit.
9. Enabling runs `auth status --hostname <host>` of the CLI of the kind. A
   CLI that is not installed, or not signed in to the host, refuses the
   enable with 409 `forge_unauthenticated` and the probe's own message. On
   success the daemon stores the login: `gh api user --jq .login --hostname
   <host>`, or the `username` of `glab api user --hostname <host>`.
   Disabling clears the login and keeps the pins. One forge repository is
   enabled on one repository row at a time. The same checkout can be
   registered once per base branch (002), so an enable on a second row that
   shares `host`, `owner` and `name` with an enabled row is refused with 409.
   The message names the first row. The daemon checks this before the
   probe. A partial unique index enforces it again inside the transaction of
   rule 8, so of two enables at once one is refused and changes nothing: no
   repository field, and no row on a registration.
10. A role's model is checked as a goal's pin is (011): a model that is no
    catalog model, or one that is turned off, is refused with 400. An empty
    model clears the role's pin and its effort. An effort alone moves on the
    role's model; with no model it is refused with 400. A role with no pin is
    allowed: that role starts no session.
11. `gh_bin` and `glab_bin` are keys of `<home>/config.toml`, listed by
    `ariadned --help`. Each is a path taken as it stands, or a bare name
    looked up on the daemon's own `PATH`, as `python_bin` is (022). `None` is
    `gh` or `glab` on that `PATH`. The client runs the CLI as a child process
    with no stdin, and gives it 30 seconds.
12. The forge client is `crates/ariadne-daemon/src/forge/`: `mod.rs` holds
    `ForgeKind`, the `Remote` parser, the detection and
    `ForgeClient::for_repository`; `github/` and `gitlab/` each hold one
    forge's CLI calls.

## Command line

13. `ariadne repo add` and `ariadne repo update` take `--forge on|off`,
    `--babysit-model`, `--babysit-effort`, `--review-model` and
    `--review-effort` (014). A model is `AGENT:MODEL`, or `""` to clear the
    role. An effort is any effort, or `default`. No forge flag sends no
    `forge`.
14. `ariadne repo ls` has a FORGE column: `<kind> <owner>/<name> on|off`, for
    example `github acme/widgets on`, or `-`. `ariadne repo inspect` prints
    the forge block: `forge`, `forge remote` (the remote and the host),
    `forge login`, `babysit` and `review`, each pin as `<model> @ <effort>`,
    or `-`.

## Desktop

15. The repositories table has a Forge column: the kind, `owner/name`, and
    `on` or `off`, or `none` (015).
16. The repository dialog shows a Forge section while it edits a repository.
    The section shows the detected remote read-only, an Enabled switch, and
    a pin picker with effort for each role: "Babysitter runs on" and
    "Reviewer runs on" (`features/models/pin-picker.tsx`). A Clear button
    beside a pinned role clears it. A repository with no row says that no
    remote was detected. Registration shows no Forge section: the daemon
    detects nothing before it opens the checkout. The dialog sends only what
    changed of the forge, and a refusal to enable lands on the switch in the
    daemon's words.

## Acceptance criteria

- A repository with an `origin` on github.com registers with `kind = github`,
  and `owner` and `name` parsed from the SSH and the HTTPS form, with and
  without `.git` and a trailing slash
  (`forge_integration.rs::a_github_origin_registers_with_owner_and_name_from_every_url_form`,
  `forge/mod.rs::tests::a_remote_url_is_read_in_its_ssh_and_https_forms`).
- A checkout with no remote has `forge = null`, and a forge change there is
  refused with `forge_unavailable`
  (`forge_integration.rs::a_checkout_with_no_remote_has_no_forge`). The only
  remote stands in for a missing `origin`, and two remotes with no `origin`
  name none (`::the_only_remote_stands_in_for_a_missing_origin`).
- Another host is the forge whose CLI is signed in to it, and `glab`'s
  `username` is the login
  (`forge_integration.rs::another_host_is_the_forge_whose_cli_is_signed_in_to_it`).
- Enabling with the stub's `auth status` at exit 1 is refused with 409 and
  the probe's message. With exit 0, the stub's login is stored and one
  `repository_updated` carries `forge.enabled = true` and `forge.login`
  (`forge_integration.rs::enabling_needs_the_cli_signed_in_and_stores_its_login`).
- A pin that is no catalog model is refused with 400, and a role without a
  pin is accepted
  (`forge_integration.rs::a_pin_must_be_a_catalog_model_and_a_role_may_have_none`).
  Disabling keeps the pins (`::disabling_keeps_the_pins`).
- The same checkout on two base branches enables the integration on one row.
  The second is refused with 409 that names the first, and disabling the
  first lets the second enable
  (`forge_integration.rs::one_forge_repository_is_enabled_on_one_repository_row_at_a_time`,
  `store.rs::one_forge_repository_is_enabled_on_one_row`). Of two enables at
  once, the refused one changes no field of its repository
  (`forge_integration.rs::concurrent_enables_on_two_rows_leave_the_loser_unchanged`),
  and a refused registration leaves no repository row
  (`::concurrent_registrations_that_enable_leave_no_repository_for_the_loser`).
- The row is read with its repository, lower-cased, and goes with it
  (`store.rs::a_forge_integration_is_read_with_its_repository_and_goes_with_it`).
- A changed remote URL is detected on the next edit and at daemon start, and
  a remote that went away leaves no forge
  (`forge_integration.rs::a_changed_remote_is_detected_on_the_next_edit_and_at_daemon_start`).
- `gh_bin` and `glab_bin` are read from `config.toml`
  (`config.rs::tests::the_forge_cli_keys_are_read`).
- `repo add` and `repo update` take the forge flags
  (`repo.rs::tests::repo_add_and_update_take_the_forge_flags`), `repo ls`
  shows the FORGE column (`::repo_ls_shows_the_forge_column`), and `repo
  inspect` prints the forge block with the login
  (`::repo_inspect_prints_the_forge_block_with_the_login`).
- The repositories table shows the Forge column
  (`ui/src/features/repositories/repositories-page.test.tsx::shows the forge
  each remote is on, and whether it is enabled`).
- The form shows the detected remote and enables the forge with both pins
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::the forge
  integration > shows the detected remote, and enables the forge with both
  pins`), sends nothing of the forge where nothing changed (`::sends nothing
  of the forge where nothing of it changed`), clears a role (`::clears a
  role's pin with the empty model the daemon spells it as`), puts a refusal
  on the switch (`::puts a refusal to enable on the switch, in the CLI's own
  words`), and shows no Forge section on registration or where nothing was
  detected (`::shows no forge while registering: nothing is detected before
  the daemon opens the checkout`, `::says where no remote was detected, and
  offers nothing to enable`).

## Sources

`crates/ariadne-store/migrations/0004_forge_integrations.sql`,
`crates/ariadne-store/src/forge.rs`, `crates/ariadne-daemon/src/forge/`,
`crates/ariadne-daemon/src/http/repositories.rs`,
`crates/ariadne-daemon/tests/it/common/forge.rs`,
`crates/ariadne-cli/src/commands/repo.rs`,
`ui/src/features/repositories/repository-form-dialog.tsx`,
`ui/src/features/repositories/forge.ts`.

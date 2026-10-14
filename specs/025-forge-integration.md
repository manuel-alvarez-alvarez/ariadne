---
id: forge-integration
status: current
updated: 2026-10-14
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
integration; every review of a request starts manually, on a pin asked
directly on its own row (029), never on a repository setting. Ariadne
speaks to the forge only through the forge's own CLI, `gh` for GitHub and
`glab` for GitLab.

## Scope

In: the detection of the remote, the `forge_integrations` row, the switch,
the forge CLI client, the `gh_bin` and `glab_bin` keys, and the
command-line and desktop surfaces over them.

Out: what a repository is (002), how a task ends and the forge it publishes
to (005), how a pin is checked (011), the command tree as a whole (014), and
the desktop app as a whole (015).

## Behavior

1. A repository has at most one forge row, in the table
   `forge_integrations`, keyed by `repository_id`. The row holds `kind`
   (`github` or `gitlab`), `host`, `owner`, `name`, `remote`, `enabled`,
   `login`, `detected_at` and `updated_at`. There is no pin of any kind on
   this row: the current task-column agent keeps its request under the
   workflow rule in [030](030-workflows.md), and a review's pin lives on the
   pull request's own row, asked for directly (029). `host`, `owner` and
   `name` are stored lower-cased. A repository with no usable remote has no
   row. Deleting the repository deletes its row.
   Hook metadata and the nested DTO block follow [027](027-webhooks-and-tunnel.md).
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
   repository disables the row and clears its login: the sign-in and rule 9
   were checked against the old one.
6. Every write of the row publishes `repository_updated`, carrying the
   repository with its forge (012). There is no event kind of its own.
7. `RepositoryDto.forge` is a `ForgeDto` — `kind`, `host`, `owner`, `name`,
   `remote`, `enabled`, `login` — or null where the repository has no row.
   Goals carry their repositories with the same `forge`.
8. `CreateRepositoryRequest` and `UpdateRepositoryRequest` take `forge`, a
   `ForgeUpdate` of `enabled` alone. An absent field is unchanged; an
   unknown one, `review_model`, `review_effort` and `babysit_model` among
   them, is refused with 422. A forge change on a repository with no row is
   refused with 409 `forge_unavailable`. A refusal writes nothing. The
   repository write and the forge write are one store transaction, and the
   request publishes one event after the commit.
9. Enabling runs `auth status --hostname <host>` of the CLI of the kind. A
   CLI that is not installed, or not signed in to the host, refuses the
   enable with 409 `forge_unauthenticated` and the probe's own message. On
   success the daemon stores the login: `gh api user --jq .login --hostname
   <host>`, or the `username` of `glab api user --hostname <host>`.
   Disabling clears the login. One forge repository is
   enabled on one repository row at a time. The same checkout can be
   registered once per base branch (002), so an enable on a second row that
   shares `host`, `owner` and `name` with an enabled row is refused with 409.
   The message names the first row. The daemon checks this before the
   probe. A partial unique index enforces it again inside the transaction of
   rule 8, so of two enables at once one is refused and changes nothing: no
   repository field, and no row on a registration.
10. `gh_bin` and `glab_bin` are keys of `<home>/config.toml`, listed by
    `ariadned --help`. Each is a path taken as it stands, or a bare name
    looked up on the daemon's own `PATH`, as `python_bin` is (022). `None` is
    `gh` or `glab` on that `PATH`. The client runs the CLI as a child process
    with no stdin, and gives it 30 seconds.
11. The forge client is `crates/ariadne-daemon/src/forge/`: `mod.rs` holds
    `ForgeKind`, the `Remote` parser, the detection and
    `ForgeClient::for_repository`; `github/` and `gitlab/` each hold one
    forge's CLI calls.
12. `POST /v1/forge/detect` reads a checkout's remote the same way
    registration does, without registering or writing anything: `{path}` in,
    a `ForgeDetectionDto` (`kind`, `host`, `owner`, `name`, `remote`) out, or
    null where the checkout has no usable remote. `path` must be absolute and
    an existing git work tree; either failure is refused with 400. This is
    what the desktop create form probes ahead of registering, since nothing
    is detected server-side before the daemon has opened the checkout for
    real (rule 5).

## Command line

13. `ariadne repo add` and `ariadne repo update` take `--forge on|off`
    (014). There is no `--review-model`, `--review-effort` or
    `--babysit-model`: every review's pin is asked for directly on its own
    request, never on the repository (029). No forge flag sends no `forge`.
14. `ariadne repo ls` has a FORGE column: `<kind> <owner>/<name> on|off`, for
    example `github acme/widgets on`, or `-`. `ariadne repo inspect` prints
    the forge block: `forge`, `forge remote` (the remote and the host) and
    `forge login`.

## Desktop

15. The repositories table has a Forge column over two lines: the kind,
    with `· off` where the integration is disabled, then `owner/name`; or
    `none` (015).
16. The repository dialog shows one switch, "GitHub integration" or "GitLab
    integration": while editing, for a repository with a forge row, described
    by the detected `host/owner/name`, the CLI it goes through, and the
    login; while registering, for a path rule 12's probe finds one for,
    fired off the path field losing focus and off typing settling there,
    off by default since nothing is enabled yet. A repository with no row, a
    path with no remote on either forge, and a path not yet probed show no
    switch. The dialog sends `forge.enabled` on an edit only when the switch
    moved, and on a registration only when the probe found a forge; a
    refusal to enable lands on the switch in the daemon's words, either way.
    "Supervise posts" ([002](002-repositories-branches-and-worktrees.md) rule
    13) sits directly below the switch in both dialogs and shows only while
    it is on; turning the switch off hides the field at once without losing
    what it held, and turning it back on shows that same value again.

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
- A database at migration 0003, still carrying `forge_integrations.review_model`
  and `review_effort`, migrates to 0004, which drops both columns, with no
  error (`store.rs::an_existing_database_drops_the_forge_review_pin_columns`).
- Enabling with the stub's `auth status` at exit 1 is refused with 409 and
  the probe's message. With exit 0, the stub's login is stored and one
  `repository_updated` carries `forge.enabled = true` and `forge.login`
  (`forge_integration.rs::enabling_needs_the_cli_signed_in_and_stores_its_login`).
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
- `repo add` and `repo update` take `--forge` and reject `--review-model`
  and `--review-effort` as unknown flags
  (`repo.rs::tests::repo_add_and_update_take_the_forge_flag_and_reject_the_review_pin_flags`),
  `repo ls` shows the FORGE column (`::repo_ls_shows_the_forge_column`), and
  `repo inspect` prints the forge block with the login
  (`::repo_inspect_prints_the_forge_block_with_the_login`).
- The repositories table shows the Forge column
  (`ui/src/features/repositories/repositories-page.test.tsx::shows the forge
  each remote is on over two lines, and whether it is enabled`).
- `POST /v1/forge/detect` answers a detected forge without registering
  anything, null for a checkout with no usable remote, and 400 for a
  relative path or one that is not a git work tree
  (`forge_integration.rs::detect_reads_the_forge_without_registering`,
  `::detect_answers_null_for_a_checkout_with_no_forge_remote`,
  `::detect_refuses_a_relative_or_non_git_path`).
- The create form shows the switch, off, once a probed path answers with a
  forge, shows none for a path with no forge, and sends `forge.enabled` on
  register only once the switch was turned on
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::shows the
  forge switch, off, once a path with a forge is entered`, `::shows no forge
  switch for a path with no forge`, `::sends forge enabled in the create
  request once the switch is turned on`).
- Supervise posts sits below the switch and only while it is on, in both
  dialogs, and keeps its value across the switch going off and back on
  (`::saves the supervise posts switch`, `::hides supervise posts under an
  off forge switch, keeping its value`).

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/migrations/0004_drop_forge_review_pin.sql`,
`crates/ariadne-store/src/forge.rs`, `crates/ariadne-daemon/src/forge/`,
`crates/ariadne-daemon/src/http/repositories.rs`,
`crates/ariadne-daemon/src/http/forge.rs`,
`crates/ariadne-daemon/tests/it/common/forge.rs`,
`crates/ariadne-cli/src/commands/repo.rs`,
`ui/src/features/repositories/repository-form-dialog.tsx`,
`ui/src/features/repositories/forge.ts`.

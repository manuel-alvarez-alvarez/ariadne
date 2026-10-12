# Audit: release delivery and local install/service (RELEASE)

## Question

Can untrusted changes or substituted release inputs compromise published
binaries or installed services?

Tracer: repository/workflow input -> CI and release build -> signed artifact
and download -> installed binary and service.

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (the planning reference; the
merge commit of PR #87, `git rev-parse HEAD` confirms the match), on branch
`audit-repository-changes-through-release-assvb0`, worktree
`/Users/malvarez/.ariadne/worktrees/q4r9v3t3/33assvb0-eng`. The repository was
clean at that commit; nothing in `.github/`, `scripts/`, `.githooks/`,
`.cargo/`, `.config/`, `release-please-config.json`,
`.release-please-manifest.json` or `docs/install.md` changed afterward in
this task.

## Threat assumptions

- Installed coding-agent CLIs are trusted programs; this audit does not try
  to contain a *deliberately* malicious agent process (that is EXEC's scope).
- Repository content, forge content, browser content, network responses and
  other users' files are potentially hostile: a *trusted* agent can still be
  steered by them (prompt injection) into issuing file writes or shell
  commands it would not have chosen on its own.
- Permission guarantees documented for the installer and the daemon
  (`docs/install.md`, `docs/permissions.md`) are checked even though the
  agents running inside Ariadne are trusted.
- No attack was run against the user's real daemon, repositories, accounts or
  GitHub. Every reproduction below runs against a disposable `mktemp`
  directory with stand-in `launchctl`/`systemctl`, never the host's.
- No release was published and no workflow run was triggered; `release.yml`
  and `release-please.yml` were read, not executed.

## Coverage

### Reviewed in full
- `.github/workflows/ci.yml`, `release.yml`, `release-please.yml`
- `.github/AGENTS.md`, `.github/RELEASING.md`
- `scripts/install.sh`, `uninstall.sh`, `lib.sh`, `make-signing-cert.sh`,
  `release-notes.sh`, `AGENTS.md`
- `.githooks/pre-commit`, `pre-push`, `lib.sh`
- `.cargo/config.toml`, `.config/nextest.toml`
- `release-please-config.json`, `.release-please-manifest.json`
- `docs/install.md`, `specs/016-install-service-and-release.md`
- `crates/ariadne-daemon/src/config.rs` (where `ARIADNE_HOME`/`~/.ariadne`
  is resolved at runtime, to check it agrees with the installer)
- `crates/ariadne-daemon/src/ai_permissions/derive.rs` (the `risk_tags` the
  permission layer derives — `outside_workspace`, `root_or_home_delete`,
  `persistent_change` — to check what, if anything, flags a write outside a
  task's own worktree)

### Read for context, not line-by-line
- `docs/permissions.md` (the permission-mode table and what `learn` records)
- `scripts/tests/install-linux.sh`, `install-command.sh` (reused as the
  pattern for this report's own reproduction harness)
- `.githooks/commit-msg` (conventional-commit gate; out of this task's scope)

### Excluded, with reason
- `ui/src-tauri/` Tauri shell internals — owned by `ui/AGENTS.md`; the
  release-asset staging step that packages it (`release.yml`'s "Stage the
  release assets") is in scope and was reviewed, the Tauri app's own code is
  not.
- `crates/ariadne-daemon/src/agents/mod.rs` process-spawn path — EXEC's scope
  per the cross-task contract; read only far enough (one `grep` for a
  sandboxing primitive) to support the RELEASE-001 prerequisite discussion
  below, not audited as EXEC would.
- Dependency advisories for `ui/package-lock.json` / `Cargo.lock` — SUPPLY's
  scope.
- macOS Gatekeeper/notarization internals beyond what `docs/install.md`
  already states (not notarized, by design) — no further OS-internals
  research was useful to this question.
- A live run of `release.yml` or `make-signing-cert.sh --set-secrets` —
  would touch a real GitHub repository's secrets and releases; explicitly
  out of scope ("Do not exercise attacks against ... external services").

## Findings

### Confirmed vulnerabilities

#### RELEASE-001 — The install manifest is executed as shell, not read as data; a forged `~/.ariadne/install.env` gets arbitrary code execution and an attacker-chosen `rm -rf`, even under `uninstall.sh --dry-run`

**Severity: High.** **Confidence: Confirmed (reproduced).**

**Affected files/lines**
- `scripts/uninstall.sh:69` — `. "$ARIADNE_MANIFEST"` (the main shell, not a
  subshell)
- `scripts/uninstall.sh:108,120` — `rm -f "$ARIADNE_PREFIX/..."` and
  `rm -rf "$ARIADNE_APP"`, using values the manifest set
- `scripts/uninstall.sh:163` — `rm -rf "$ARIADNE_HOME"` under `--purge`
  (`ARIADNE_HOME` itself is not manifest-controlled — it comes from the
  `ARIADNE_HOME` environment variable or `~/.ariadne` — so this line is not
  part of the forgery surface, but it runs in the same already-sourced shell)
- `scripts/install.sh:355` — `OLD_PREFIX="$(. "$ARIADNE_MANIFEST" && ...)"`
  (a subshell — isolates the *environment* but not the *side effects* of an
  injected command)
- `scripts/install.sh:546` — the same pattern for `ARIADNE_DESKTOP_ICON`
- `scripts/install.sh:704-719` — where a legitimate manifest is written
  (the format a forged one has to imitate)

**What the code does.** `ui_locations` (`scripts/lib.sh:87-103`) sets
`ARIADNE_MANIFEST="$ARIADNE_HOME/install.env"`. `install.sh` writes that file
with `cat > "$ARIADNE_MANIFEST" <<EOF ... EOF` holding
`KEY="value"` assignments. `uninstall.sh` then does `. "$ARIADNE_MANIFEST"` —
literally sourcing it as a bash script, not parsing it as `KEY=VALUE` text.
Anything else written into that file runs as the user invoking
`uninstall.sh`, with that user's full privileges, before a single plan line
is printed — including under `--dry-run`, since the source happens at
`uninstall.sh:69` and the dry-run short-circuit is at `uninstall.sh:90-93`,
*after* it.

Once the file is sourced, `ARIADNE_PREFIX`, `ARIADNE_APP`,
`ARIADNE_DESKTOP_ENTRY` and `ARIADNE_DESKTOP_ICON` are whatever the file set
them to, and the uninstaller unconditionally `rm -f`s/`rm -rf`s those paths
later in the same run (`uninstall.sh:108,120,135-136`) — no check that they
sit under `$PREFIX`, `/Applications`, `$HOME/Applications` or
`$XDG_DATA_HOME`. Given that sourcing is already full code execution, this
second effect is not an independent escalation, but it is the literal answer
to the audit's item 6 ("can forged metadata broaden deletion"): yes, to any
path the forger names, by either mechanism.

**Prerequisite.** The attacker needs write access to
`$ARIADNE_HOME/install.env` (default `~/.ariadne/install.env`) at some point
*before* the user next runs `install.sh` or `uninstall.sh` — the payload does
not need to run at the same time as the write. `mkdir -p "$ARIADNE_HOME"` in
`install.sh:703` and `ui_locations` do not set an explicit mode, so the
directory's permissions follow the creating process's umask (typically
`755` — not group/other-writable on a normally configured machine). So this
is not remotely exploitable by an unrelated local user under default
permissions; it requires some other primitive that already lets something
write inside the invoking user's own `$HOME/.ariadne`.

That primitive is plausible in this project's own threat model, not just
hypothetically: `crates/ariadne-daemon/src/config.rs:111` resolves the
daemon's `worktree_root` to `root.join("worktrees")`, i.e. every task
worktree — including the one this audit itself runs in
(`/Users/malvarez/.ariadne/worktrees/q4r9v3t3/33assvb0-eng`) — is a
subdirectory of `$ARIADNE_HOME`. A `grep` across `crates/` for
`sandbox|chroot|landlock|seccomp|namespace|jail|bubblewrap|firejail` found no
OS-level containment of an agent's child process (the handful of hits are
unrelated identifiers, e.g. `ai_permissions` fixtures). `docs/permissions.md`
confirms the permission layer is a *behavioral* gate on tool calls (`auto`,
`ask`, `learn`, `ai`), not a filesystem boundary: `auto` — the default mode —
"selects an allowing option automatically" with no mention of restricting the
*path* a write or a `Bash` command touches. `ai_permissions/derive.rs` does
derive an `outside_workspace` risk tag (and `root_or_home_delete`,
`persistent_change`) for exactly this shape of action, which is evidence the
project already treats "a write outside the task's own workspace" as
materially different from one inside it — but by the documented `auto`
behavior, a risk tag alone does not block the call. **This audit did not
run the permission layer end to end to confirm whether `auto` mode actually
lets a tagged `outside_workspace` write through** — that confirmation, and
the question of whether a trusted-but-prompt-injected agent process can reach
`$ARIADNE_HOME/install.env` in practice, is EXEC's/the permission layer's
boundary, not RELEASE's; see "Design questions for the orchestrator" below.
Independent of that open question, the vulnerability in `install.sh`/
`uninstall.sh` is real and reproduced on its own terms: **whatever** writes
the manifest, sourcing it as code is the defect, and it is the one this task
owns and reports.

**Reproduction.** `security/audit/2026-10-12/08-release-and-install/repro-RELEASE-001.sh`,
self-contained (builds its own `mktemp` `$HOME`, copies `install.sh`/
`uninstall.sh`/`lib.sh` unmodified, stubs only `launchctl`/`systemctl` so
nothing touches a real service). Run:

```sh
bash security/audit/2026-10-12/08-release-and-install/repro-RELEASE-001.sh
```

Expected behavior: a manifest is just configuration; `--dry-run` previews
without changing anything; `ARIADNE_APP` names a path under the installer's
own footprint.

Observed behavior (sanitized; full run is reproducible verbatim):
```
== 1. uninstall.sh --dry-run ==
  [3/5] Removing ~/Documents
  ...
  Dry run - nothing was changed.
OBSERVED: .../PWNED exists — the injected line ran during --dry-run.

== 2. uninstall.sh (the real run) ==
  [3/5] OK   Removing ~/Documents
  ...
OBSERVED: .../PWNED exists — the injected line ran again.
OBSERVED: .../home/Documents (the forged ARIADNE_APP) was deleted.
```
`--dry-run` claims "nothing was changed" while the forged line has already
run; the real run both re-runs it and deletes the forged `ARIADNE_APP` path
verbatim.

**Counter-evidence / what would invalidate this finding.** If
`$ARIADNE_HOME` is created with a mode that excludes write by anything other
than the owning process, and nothing ever gets a lower-trust write path into
it (the open question above resolves to "no agent task, even prompt-injected,
can write outside its own worktree"), the prerequisite collapses to "the
attacker already has an arbitrary-file-write primitive against the user's
own `$HOME`" — at which point sourcing `install.env` adds no capability an
attacker with that primitive did not already have (they could edit
`~/.bashrc` instead). The finding would then downgrade from "a vulnerability
in the installer" to "the installer is one of many ambient files that
amplify an unrelated file-write bug into immediate code execution," which is
still worth fixing (defense in depth, and it uniquely also gets a `--dry-run`
to execute code and a `rm -rf` of an attacker-chosen path) but is not itself
the root cause.

**Remediation (smallest).** Read `install.env` as data, not as code: a
`KEY="value"` line parser (e.g. a `while IFS='=' read -r key value` loop that
strips quotes and only accepts the fixed set of keys `install.sh` actually
writes), or write/read it as a flat `key\tvalue` list consumed with pure
string operations, never `. "$file"`. Keep the manifest's existing fields
unchanged; this is a parser swap, not a format change.

**Regression criterion.** A manifest holding a line that is not one of the
known `KEY="value"` assignments (e.g. a stray shell command, or a key
`uninstall.sh`/`install.sh` does not recognize) must not run as code and
must not stop the uninstall from reading the keys that *are* well-formed. A
test parallel to this report's `repro-RELEASE-001.sh` — assert the sentinel
file this task used (`PWNED`) is never created, and that `rm -rf` only ever
runs on a path provably under `$PREFIX`, `/Applications`,
`$HOME/Applications` or `$XDG_DATA_HOME` — would prove it; a compatibility
decision for the user is whether `uninstall.sh` should additionally refuse
(rather than silently drop) a manifest value that fails that path check, see
below.

---

### Hardening suggestions

#### RELEASE-002 — `gh attestation verify` is not pinned to the release workflow's identity

**Severity: Low (no demonstrated exploit under the repo's current
permissions).** **Confidence: Plausible.**

**Affected:** `scripts/install.sh:410` —
`gh attestation verify "$_asset" --repo "$RELEASE_REPO"`.

This is the **only** check the installer enforces before installing a
downloaded binary — see "Available vs. enforced" below. It verifies that
some workflow run in `$RELEASE_REPO`, authenticated via GitHub's OIDC
`id-token`, produced a Sigstore-backed provenance attestation whose subject
digest matches the downloaded file. It does **not** pass `--signer-workflow`
or `--cert-identity`, so it accepts an attestation from *any* workflow in
that repository that holds `id-token: write` and `attestations: write` —
not only `.github/workflows/release.yml`. Today that is the only workflow
with those permissions (`ci.yml` and `release-please.yml` both declare no
such permissions, and `release-please.yml` additionally sets
`permissions: {}` and acts under a PAT), and reaching `release.yml`'s build
job requires either a maintainer-driven release publish or
`workflow_dispatch`, which itself requires repository write access — so
there is no demonstrated path for a lower-privileged actor to forge an
accepted attestation today.

**Why it matters anyway.** The check is the installer's *only* gate; if a
future workflow (a bot integration, a second release channel, a careless
copy-paste) is ever added with `attestations: write` for any reason, it
silently becomes a second source install.sh will trust, with no change to
install.sh needed on the attacker's part. Pinning the signer now costs
nothing and removes that failure mode by construction rather than by
"nothing else has those permissions today."

**Remediation.** Add `--signer-workflow .github/workflows/release.yml` (or
the equivalent `--cert-identity` regex anchored to that workflow path) to
the `gh attestation verify` call.

**Regression criterion.** An attestation produced by a workflow at a
different path (a copy of `release.yml` under another name, verified with
a stand-in `gh` the way `scripts/tests/install-command.sh` already stands in
for `gh attestation verify`) is rejected; one produced by
`release.yml` itself still passes.

#### RELEASE-003 — The installing user's live `$PATH` is baked verbatim into the persistent daemon service

**Severity: Low (depends on a pre-existing unsafe `$PATH` at install time;
not attacker-controlled by this flow alone).** **Confidence: Plausible.**

**Affected:**
- `scripts/install.sh:625` (Darwin): `<key>PATH</key><string>$PATH</string>`
  inside the generated `launchd` plist, `scripts/install.sh:611-641`
- `scripts/install.sh:666` (Linux): `Environment="PATH=$PATH"` inside the
  generated `systemd --user` unit, `scripts/install.sh:657-679`

Both are deliberate, documented behavior (`scripts/install.sh:623-624,
665`: "launchd services get a bare PATH ... so bake in the installing user's
PATH") — `ariadned` needs `tmux`, `git` and agent CLIs resolved the way the
user's own shell resolves them. The installer copies that `$PATH` with no
filtering. If the invoking shell's `$PATH` at install time contains a
relative entry, a world- or group-writable directory, or any directory an
untrusted party can place a same-named binary into (a known risk with tools
like `direnv`, local `node_modules/.bin` entries, or a stale project-specific
PATH export sourced from a dotfile), that entry is captured into a service
definition that starts at login and persists across shell sessions and
reboots, independent of whether the live shell's `$PATH` is later cleaned
up. Every binary the daemon or an agent it spawns resolves by bare name
(`git`, `cargo`, `npm`, `python3`, `nvidia-smi`, `gh`, `glab` —
`crates/ariadne-daemon/src/config.rs` resolves several of these from `PATH`
specifically when no explicit path is configured) is then subject to
hijacking by whatever that captured entry pointed at, for as long as the
service runs.

**Remediation.** At minimum, warn (not block — this must stay non-fatal, per
the installer's own idiom of warning rather than failing on a PATH that
excludes `$PREFIX`, `scripts/install.sh:769-772`) when a component of
`$PATH` is relative or world/group-writable, naming the offending entry,
before baking it into the plist/unit. A stronger fix — filtering such
entries out of the captured `PATH` rather than only warning — needs the
user's answer on whether a tool currently resolved only via such an entry
(e.g. a `direnv`-shimmed interpreter) should keep working after an install;
see "Compatibility decisions" below.

**Regression criterion.** Installing with a `$PATH` containing a
world-writable directory prints a named warning; the generated plist/unit
still contains exactly the caller's `$PATH` (so nothing already relying on
an unusual `$PATH` entry silently breaks) unless the user opts into
filtering.

---

### Unverified candidates

- **Whether an installed agent task (this audit's own task included) can
  write outside its own worktree, specifically to `$ARIADNE_HOME` itself.**
  Evidence gathered (`config.rs`'s `worktree_root`, the absence of any OS
  sandboxing primitive, `docs/permissions.md`'s description of `auto` as
  behavioral-only) makes this plausible but this task did not drive the
  daemon's actual permission pipeline end to end to confirm it. This is the
  load-bearing prerequisite for RELEASE-001's severity and is flagged to the
  orchestrator below rather than claimed as proven.
- **Floating-tag GitHub Actions in `ci.yml`** (`actions/checkout@v5`,
  `dtolnay/rust-toolchain@stable`, `taiki-e/install-action@v2`,
  `Swatinem/rust-cache@v2`, `actions/setup-node@v5`) are a generic
  supply-chain surface (a compromised upstream Action release could run
  arbitrary code during CI) but this is already documented and deliberately
  accepted in `.github/AGENTS.md` ("`ci.yml` uses floating tags ... since
  nothing there is attested"); not re-litigated here, and arguably SUPPLY's
  territory if it is to be revisited.
- **`ci.yml` declares no `permissions:` block** while running on
  `pull_request` (including, implicitly, forks) and executing untrusted
  build/test code (`cargo build`/`test`/`clippy`, `npm ci`/`test`, which can
  run arbitrary `build.rs`, proc-macro, postinstall or test code). Considered
  and rejected as a confirmed vulnerability below.

### Rejected hypotheses

- **"A fork PR's CI run could abuse an implicit write-scoped
  `GITHUB_TOKEN`."** Rejected. `ci.yml` references no `secrets.*` anywhere,
  and GitHub hard-codes a read-only `GITHUB_TOKEN` for `pull_request` (not
  `pull_request_target`) runs triggered by a fork, regardless of the
  repository's or organization's default token-permission setting. A
  same-repo branch PR could in principle have a write-scoped token if the
  org default is write, but a contributor who can open such a PR already has
  push access, so this grants no privilege they lack. Explicit
  `permissions: contents: read` at the top of `ci.yml` would still be good
  practice (defense in depth against a future `pull_request_target` mistake
  or an org default change) but is not fixing a reachable vulnerability
  today, so it is listed under "unverified candidates," not as a finding.
- **"The release archives could carry a path-traversal or symlink
  payload that escapes the staging directory on extraction."** Rejected.
  `scripts/install.sh` verifies every file in the staging directory with
  `gh attestation verify` (looping over `"$STAGE_DIR"/*`,
  `scripts/install.sh:409-412`) *before* any `tar -xzf` of it
  (`scripts/install.sh:427,461,469`), and the attestation subject is a
  content digest, so a tampered archive fails verification rather than
  reaching `tar`. The archives themselves are produced by the trusted
  `release.yml` build job with fixed, reviewed contents (binaries at the
  tarball root, or a single `.app`/plain binary + `icon.png`), so there is
  no untrusted producer of archive entries to test a symlink/traversal
  payload against, short of a compromised CI run — a different, already
  covered-by-pinning-and-permissions threat, not an extraction-logic bug.
- **"`install.sh`'s desktop-entry or plist generation is injectable via
  `$PREFIX` or the app name."** Rejected. `$PREFIX` is operator-supplied
  (`--prefix`, the installing user's own choice), the app name is a fixed
  literal, and nothing downstream of them is `eval`'d — they are written
  into a `.desktop`/plist file as data, not executed.

## Available vs. enforced (required distinction)

| Thing | Available | Enforced by `install.sh` before installing |
| --- | --- | --- |
| Build provenance attestation | Yes, on every release asset (`release.yml`'s "Attest the release assets" step) | **Yes** — the sole gate, `gh attestation verify "$_asset" --repo "$RELEASE_REPO"` for every file in the staging directory (`scripts/install.sh:409-412`); a failure calls `ui_die`, which exits before any `tar`/`install -m 755`. |
| SHA-256 checksums (`checksums-<target>.txt`, published in the Release body table by `scripts/release-notes.sh`) | Yes, one line per asset, human-readable on the Release page | **No.** `install.sh` never downloads a `checksums-*.txt` file or compares a digest; the table exists for a human to cross-check by hand, not for the script to enforce. |
| macOS code signature (self-signed "Ariadne Code Signing") | Yes, on release assets built on macOS runners and on local builds that find the identity | **No, for downloaded assets.** `install.sh` runs `codesign --force`/`--verify` only in the `--build-from-source` branch (`scripts/install.sh:366-370,213-219` of that branch); a downloaded asset is never `codesign --verify`'d by the installer, consistent with `docs/install.md`'s own statement that the certificate is "self-signed, not notarized" and exists to keep macOS privacy grants stable across builds, not to gate installation. |
| Quarantine clearing | N/A (a side effect, not a check) | `clear_quarantine` runs unconditionally *after* attestation succeeds, never as a substitute for it. |

The documentation (`docs/install.md:100-118`) matches this table: it states
only that "the installer verifies each file with `gh attestation verify`
before installing it," and separately that the signing certificate is
self-signed/not notarized. No doc claims checksum verification is enforced.
No gap was found between what is documented and what the code enforces; the
gap, if any, is that the single enforced check (RELEASE-002) is not pinned as
tightly as it could be.

## Design questions for the orchestrator

1. **Cross-boundary (for EXEC / the permission-model owner):** can an agent
   task's process — in particular one steered by a hostile repository via
   prompt injection, inside an otherwise-trusted agent CLI — write to a path
   outside its own worktree, e.g. `$ARIADNE_HOME/install.env` directly?
   `ai_permissions/derive.rs` already derives an `outside_workspace` risk
   tag for such an action; `docs/permissions.md` describes `auto` (the
   default mode) as choosing an allowing option automatically with no stated
   exception for that tag. If `auto` does let an `outside_workspace`-tagged
   write through, RELEASE-001's prerequisite is met by this project's own
   normal operation, not just by an unrelated local-privilege bug, and its
   severity should be read as reachable rather than merely plausible. This
   audit did not drive the permission pipeline to confirm this either way,
   and recommends whichever task owns `ai_permissions`/EXEC confirm it.
2. **Compatibility decision for RELEASE-001's remediation:** should
   `uninstall.sh`, on finding an `ARIADNE_APP`/`ARIADNE_PREFIX`/etc. value in
   the manifest that is not a plausible path under the installer's own
   footprint (`$PREFIX`, `/Applications`, `$HOME/Applications`,
   `$XDG_DATA_HOME`), refuse and say so, or silently skip only that one
   removal and continue the rest of the uninstall? The former is stricter
   but means a manifest hand-edited for a legitimate reason (a user who
   moved their own `$PREFIX` by hand, say) could start failing; the latter
   is friendlier but could silently leave a legitimate removal undone. This
   needs the user's answer, not an assumption.
3. **Compatibility decision for RELEASE-003:** should a risky `$PATH` entry
   found at install time only be warned about (current idiom elsewhere in
   the installer, e.g. the `$PREFIX`-not-on-`$PATH` warning at
   `scripts/install.sh:769-772`), or filtered out of what gets baked into
   the service definition? Filtering could break a setup that currently
   relies on such an entry (e.g. a `direnv`-managed interpreter); needs the
   user's answer.

## Recommendation

Fix RELEASE-001 first and alone: swap `. "$ARIADNE_MANIFEST"` for a
`KEY="value"`-only reader in both `install.sh` and `uninstall.sh`. It is a
small, mechanical change (the manifest's own writer already only ever emits
that shape), it closes both the code-execution and the broadened-deletion
paths at once, and `--dry-run` executing attacker-controlled code before
printing "nothing was changed" is, on its own, a correctness bug independent
of any question about how the manifest got forged. Treat RELEASE-002 and
RELEASE-003 as follow-on hardening once question 1 above is answered,
since that answer changes how urgently RELEASE-001-adjacent defense in depth
is needed.

## Confidence

- RELEASE-001: **High** confidence in the vulnerability itself (reproduced
  twice, against the unmodified scripts). **Medium** confidence in its
  real-world severity, pending the orchestrator's answer to design question
  1 above.
- RELEASE-002, RELEASE-003: **Medium** confidence these are worth fixing as
  hardening; **low** urgency, since neither has a demonstrated exploit path
  under the repository's current configuration.
- Everything under "Rejected hypotheses": confident in the rejection, each
  with a specific counter-observation cited above.

## What was not checked (coverage gaps)

- This audit ran entirely on macOS (Darwin 25.6.0, the host's own OS); the
  Linux installer branches (`systemd --user`, GNOME `.desktop`/icon
  registration, the `libwebkit2gtk-4.1.so.0` probe) were read, and the
  RELEASE-001 reproduction's logic is OS-independent (it never reaches the
  OS-specific branches — the forged manifest is sourced, and the deletions
  it drives, before any launchd/systemd call), but no Linux-specific install
  behavior was executed. `scripts/tests/install-linux.sh` already covers
  that surface and was read, not re-run.
- No real GitHub release, workflow run, or `gh attestation verify` call
  against GitHub's actual attestation API was exercised — doing so would
  require publishing a release or dispatching a workflow against the real
  repository, both excluded by scope. RELEASE-002's claim about
  `gh attestation verify`'s default scoping is based on reading
  `scripts/install.sh`'s invocation and the documented semantics of the
  `--signer-workflow`/`--cert-identity` flags, not an observed bypass.
- `make-signing-cert.sh` was read but not run (it mutates the login
  keychain's trust settings and needs a one-time interactive macOS
  authentication prompt per its own comments); no defect was found in it
  worth a disposable-keychain reproduction.
- Dependency advisories for the Action versions pinned/floated across the
  three workflows were not individually queried against an advisory
  database; this task treated the repo's own `.github/AGENTS.md` pinning
  rationale as the record of that decision rather than re-auditing each
  Action's current CVE status, which is closer to SUPPLY's mandate.

# Audit: release delivery and local install/service (RELEASE)

## Revision note

This report was returned by review (verdict FAIL, reviewed commit
`ffc93bde87d843d3a9a798ea35a60dfba5641004`) with five must-fix items. This
revision: isolates the RELEASE-001 reproduction fully (an earlier version
could leak into a caller's real `ZDOTDIR`/`XDG_DATA_HOME`); separates
RELEASE-001's remediation into a non-executing parser *and* a per-field
deletion-authorization check, since a pure `KEY="VALUE"` parser alone still
permits deleting an unrelated directory (reproduced below, scenario 0);
reclassifies RELEASE-001's end-to-end reachability as unverified rather than
implying a permission-model bypass; corrects the `gh attestation verify`
proposal and the signing/verification table against the CLI's documented
flags; adds the previously-missing release-flow evidence (cache scoping,
checkout credential persistence, manual-dispatch inputs, secret scope, the
build window during which the macOS signing keychain is unlocked); corrects
the unconditional fork-token rejection; and promotes the dismissed
"service-file injection" hypothesis to a new confirmed finding, RELEASE-004,
with its own reproduction, after the reviewer showed "no shell eval" does
not mean "no injection" — `launchd`/`systemd` parse the generated files
themselves.

This report was then returned a second time (verdict FAIL, reviewed commit
`c3cbb4751fcb68dfa1fd282ee530c0cbdd7690b9`) with three further must-fix
items. This revision: rewrites RELEASE-001's deletion-authorization proposal
per-field for all eight manifest keys the parser accepts (the prior version
covered only `ARIADNE_APP`/`ARIADNE_DESKTOP_ENTRY`/`ARIADNE_DESKTOP_ICON`/
`ARIADNE_PREFIX` and would have rejected a normal Linux install's own
`ARIADNE_APP="$PREFIX/ariadne-desktop"`; the reviewer independently
reproduced deletion of two more unrelated files through
`ARIADNE_BASH_COMPLETION`/`ARIADNE_PLIST`, which the prior proposal did not
cover at all — reproduced here too, scenario 0b), withdraws the overclaim
that canonicalization closes the symlink/TOCTOU race (it narrows, and does
not close, that race — recorded as an accepted residual limitation), and
adds positive regression criteria for ordinary default and custom-prefix
installs alongside the rejection cases; rewrites RELEASE-004's remediation
to escape `$PREFIX` as well as `$PATH` for both target formats, after the
reviewer's own unmodified-installer reproduction showed the single-line,
newline-free, NUL-free payload already proven to inject a plist key would
have sailed through the prior proposal's newline/NUL rejection unchanged,
and corrects the systemd claim to the actual quoting grammar rather than
"reject newlines, done"; narrows the release-flow evidence's keychain-window
and upload-token paragraphs to what step ordering actually establishes
(timing, not isolation, and the full `contents`/`id-token`/`attestations`
write scope available throughout the job, not only at the specific upload
step); and fixes several precision errors the reviewer listed separately
(a wrong step-order claim and an overstated OS-coverage claim in "What was
not checked"; a `$(cat ...)`-based comparison in the RELEASE-001 fixture
that discards trailing newlines, now `cmp`; and an inaccurate claim that
testing real attestation verification would require publishing a new
release — corrected by actually running one, read-only, against this
repository's existing `v0.11.0` release; see RELEASE-002).

A third round added one more thing, at the orchestrator's separate request:
an investigation of the `.githooks/pre-push` test failure the second
review's check run reported
(`kept_requests::the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side`),
against the unchanged application baseline. See "Pre-existing
`.githooks/pre-push` failure" below — it is intermittent, not caused by
this audit's diff, and not a RELEASE finding; it is recorded and a
follow-up is proposed, not fixed here.

## Question

Can untrusted changes or substituted release inputs compromise published
binaries or installed services?

Tracer: repository/workflow input -> CI and release build -> signed artifact
and download -> installed binary and service.

## Examined commits

- Application commit: `a17283486d5fe89591edfff08832712936d73030` (the
  planning reference; `git rev-parse HEAD` on the worktree confirms the
  match), on branch `audit-repository-changes-through-release-assvb0`. The
  repository was clean at that commit; nothing in `.github/`, `scripts/`,
  `.githooks/`, `.cargo/`, `.config/`, `release-please-config.json`,
  `.release-please-manifest.json` or `docs/install.md` changed during this
  task or its revision.
- Reviewed commit (this report's prior revision): `ffc93bde87d843d3a9a798ea35a60dfba5641004`.

## Threat assumptions

- Installed coding-agent CLIs are trusted programs; this audit does not try
  to contain a *deliberately* malicious agent process (that is EXEC's scope).
- Repository content, forge content, browser content, network responses and
  other users' files are potentially hostile: a *trusted* agent, or a
  trusting human copying install instructions from somewhere untrusted, can
  still be steered into issuing file writes, shell commands, or installer
  flags it would not have chosen unprompted.
- Permission guarantees documented for the installer and the daemon
  (`docs/install.md`, `docs/permissions.md`) are checked even though the
  agents running inside Ariadne are trusted.
- No attack was run against the user's real daemon, repositories, accounts or
  GitHub. `repro-RELEASE-001.sh` runs `install.sh`/`uninstall.sh` under
  `env -i` plus an explicit, complete set of home/config/data variables
  inside a throwaway `mktemp` directory, with stand-in `launchctl`/
  `systemctl` on `PATH` (though on this task's Darwin host, `uninstall.sh`'s
  own `OS="$(uname -s)"` takes the Darwin branch every time, so only the
  `launchctl` stand-in is actually exercised; no `uname` stand-in is used
  there). `repro-RELEASE-004.sh` adds no `uname` stand-in either: it does
  not go through `ui_locations`'s OS detection at all, on either platform —
  it extracts the two heredocs directly by `sed` line range and sources
  each one with the needed variables set by hand, so the Linux unit
  heredoc runs unconditionally on this macOS host by construction, not by
  fooling `uname` into reporting Linux. One
  real, read-only `gh attestation verify` call was made against this
  repository's own already-published `v0.11.0` release (see RELEASE-002) —
  not against the user's daemon or any mutable state.
- No release was published and no workflow run was triggered; `release.yml`
  and `release-please.yml` were read, not executed.
- The repository's GitHub visibility was checked, not assumed: `gh repo view
  --json visibility` reports `"visibility":"PUBLIC"` for
  `manuel-alvarez-alvarez/ariadne` (the `origin` remote). This matters for
  one of the corrections below.

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
  task's own worktree; see the corrected framing in RELEASE-001 below)

### Read for context, not line-by-line
- `docs/permissions.md` (the permission-mode table and what `learn` records)
- `scripts/tests/install-linux.sh`, `install-command.sh` (the pattern this
  report's own RELEASE-004 reproduction reuses to replay install.sh's
  generated-file logic without a real build)
- `.githooks/commit-msg` (conventional-commit gate; out of this task's scope)

### Primary sources consulted for external technical claims
- `gh attestation verify`'s flag semantics: `https://cli.github.com/manual/gh_attestation_verify`
  (fetched; quoted under RELEASE-002)
- `GITHUB_TOKEN` permissions for fork pull requests, and the private-repository
  exception: `https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax`
  and `https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository`
  (fetched; quoted under "Rejected hypotheses")
- GitHub Actions cache scoping across branches and forks:
  `https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/caching-dependencies-to-speed-up-workflows`
  (fetched; quoted under "Release-flow evidence")
- `actions/checkout`'s `persist-credentials` default:
  `https://github.com/actions/checkout` (fetched; quoted under "Release-flow
  evidence")
- `systemd.syntax(7)`'s comment-line rule, quoting/escaping grammar
  (quotes wrap "a whole item", items are whitespace-separated, C-style
  escapes including `\n`/`\"`/`\\` are supported) and `systemd.service(5)`'s
  `ExecStartPre=` semantics: `https://man.archlinux.org/man/systemd.syntax.7`,
  `https://man.archlinux.org/man/systemd.service.5` (fetched; quoted under
  RELEASE-004). `freedesktop.org`'s own manual pages returned HTTP 403 to
  this tool; the Arch Linux mirror, which republishes the same upstream
  systemd man pages verbatim, was used instead and is named as such rather
  than passed off as the canonical host. `systemd.exec(5)`'s specific
  `Environment=` section (which would additionally confirm whether one line
  can set more than one variable) could not be fetched from any of four
  mirrors tried (`freedesktop.org`, two Arch Linux URLs, Debian's manpage
  mirror) — each either 403'd or returned a page truncated before that
  section. RELEASE-004's systemd remediation below relies only on the
  general quoting grammar actually confirmed, not on that specific detail;
  the gap is recorded rather than papered over.
- A real, read-only `gh attestation verify` call, made against this
  repository's own already-published `v0.11.0` release (`gh release
  download v0.11.0 --repo manuel-alvarez-alvarez/ariadne --pattern
  'ariadne-aarch64-apple-darwin.tar.gz'`, then `gh attestation verify` on
  the downloaded file, in a scratch `/tmp` directory removed immediately
  after) — not a hypothetical read of the manual. Quoted and detailed under
  RELEASE-002.
- The GitHub Actions runner's own source, to settle exactly which
  credentials a `run:` step's shell process receives automatically versus
  only through an explicit workflow `env:` mapping:
  `https://raw.githubusercontent.com/actions/runner/main/src/Runner.Worker/GitHubContext.cs`
  (its `_contextEnvAllowlist` excludes `token` by name) and
  `https://raw.githubusercontent.com/actions/runner/main/src/Runner.Worker/Handlers/ScriptHandler.cs`
  (sets `ACTIONS_ID_TOKEN_REQUEST_URL`/`ACTIONS_ID_TOKEN_REQUEST_TOKEN`
  directly on every script step's environment). Fetched and quoted under
  "Release-flow evidence" below.
- GitHub's Security Advisory Database, queried directly via
  `gh api "/advisories?ecosystem=actions&per_page=100"` (56 advisories, no
  `Link` pagination header, so this is the complete current set for that
  ecosystem) — see "Action advisory check" below. The GitHub Advisories *web
  search* (fetched via a generic URL) was tried first and found unreliable
  for this purpose (full-text search surfaced unrelated packages — a NuGet
  mapping library, an unconfirmable npm CVE number — that do not actually
  name the Actions in question); the authenticated `gh api` call against
  GitHub's own advisory endpoint is the one this report relies on.

### Excluded, with reason
- `ui/src-tauri/` Tauri shell internals — owned by `ui/AGENTS.md`; the
  release-asset staging step that packages it (`release.yml`'s "Stage the
  release assets") is in scope and was reviewed, the Tauri app's own code is
  not.
- `crates/ariadne-daemon/src/agents/mod.rs` process-spawn path and the
  `ai_permissions` decision pipeline's actual runtime behavior — EXEC's
  scope per the cross-task contract. This audit read `derive.rs`'s risk-tag
  logic (static analysis of what gets *labeled*) but did not drive the
  daemon's permission pipeline end to end, and does not claim to know what
  `auto` mode does with an `outside_workspace`-tagged request; see
  RELEASE-001's corrected reachability discussion.
- Dependency advisories for `ui/package-lock.json` / `Cargo.lock` (npm/cargo
  ecosystems) — SUPPLY's scope. Only the GitHub Actions ecosystem, which
  gates the release/install path this task owns, was checked.
- macOS Gatekeeper/notarization internals beyond what `docs/install.md`
  already states (not notarized, by design) — no further OS-internals
  research was useful to this question.
- A live run of `release.yml`, a real `systemctl --user`/`launchctl`
  registration, or `make-signing-cert.sh --set-secrets` — would touch a
  real GitHub repository's secrets/releases or a real host service;
  explicitly out of scope. RELEASE-004's systemd claim is therefore backed
  by `systemd.service(5)`/`systemd.syntax(7)`'s documented grammar rather
  than an executed unit; this is recorded as a limitation, not elided.

## Findings

### Confirmed vulnerabilities

#### RELEASE-001 — The install manifest is executed as shell, not read as data; a forged `~/.ariadne/install.env` gets arbitrary code execution, an attacker-chosen `rm -rf`, and a forced `--purge`

**Severity: the sink is High impact. End-to-end reachability from an
in-scope hostile-input route is unverified — see "Reachability" below; do
not read this as an unconditional High.**
**Confidence: sink Confirmed (reproduced four separate ways). Reachability:
Unverified.**

**Affected files/lines**
- `scripts/uninstall.sh:69` — `. "$ARIADNE_MANIFEST"` (the main shell, not a
  subshell)
- `scripts/uninstall.sh:41-50` — `PURGE` is parsed from `--purge` *before*
  line 69's source; sourcing runs after, in the same shell, so the manifest
  can simply reassign `PURGE=1` regardless of what was on the command line
- `scripts/uninstall.sh:108,120` — `rm -f "$ARIADNE_PREFIX/..."` and
  `rm -rf "$ARIADNE_APP"`, using values the manifest set
- `scripts/uninstall.sh:161-165` — `if [ "$PURGE" = 1 ]; then rm -rf
  "$ARIADNE_HOME"; fi`. `$ARIADNE_HOME` is itself resolved by `ui_locations`
  *before* the manifest is sourced (from the `ARIADNE_HOME` environment
  variable or `~/.ariadne`), so a forger does not need to overwrite it —
  they already know its value, since it is exactly where they had to write
  `install.env` in the first place. What they get for free is `PURGE`: a
  forged `PURGE=1` makes this line run on a plain `uninstall.sh`, with no
  `--purge` on the command line at all, and wipe the daemon's entire data
  directory (database, worktrees, run dir, logs).
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

Three distinct effects follow, and they do not all require the same fix:
1. **Code execution.** Sourcing is sufficient on its own; any line that is
   not a `KEY="value"` assignment just runs.
2. **Deletion of an attacker-named path.** Independent of (1): once the file
   is sourced, `ARIADNE_PREFIX`, `ARIADNE_APP`, `ARIADNE_DESKTOP_ENTRY` and
   `ARIADNE_DESKTOP_ICON` are whatever the file set them to, and the
   uninstaller unconditionally `rm -f`s/`rm -rf`s those paths later in the
   same run — no check that they sit under `$PREFIX`, `/Applications`,
   `$HOME/Applications` or `$XDG_DATA_HOME`. **This does not need an
   injected command.** Reproduction scenario 0 below uses a manifest with
   *only* well-formed assignments — nothing a non-executing parser would
   reject — and still gets an unrelated directory deleted.
3. **A forced `--purge`.** Also independent of (1): a bare `PURGE=1`
   assignment (again, no injected command) reaches the `rm -rf
   "$ARIADNE_HOME"` at `uninstall.sh:161-165` regardless of the actual
   command-line flags. Reproduction scenario 3 below confirms this.

**Reachability — corrected.** The prior revision of this report called
RELEASE-001 "Confirmed" at "High" severity for the whole chain, including
reaching the write. That overstated it. What the reproductions below prove
is the **sink**: given a forged manifest, by whatever means, the three
effects above happen. What they do not prove, and what this audit did not
establish, is an **in-scope route from hostile input to writing that
manifest**. Specifically:
- `mkdir -p "$ARIADNE_HOME"` (`scripts/install.sh:703`, and `ui_locations`)
  sets no explicit mode, so the directory's permissions follow the creating
  process's umask — typically `755`, not group/other-writable. This is not
  remotely exploitable by an unrelated local user under default permissions.
- This audit observed, by static reading only, that
  `crates/ariadne-daemon/src/config.rs:111` resolves the daemon's
  `worktree_root` to `root.join("worktrees")` — i.e. task worktrees
  (including the one this audit itself ran in) are filesystem
  subdirectories of `$ARIADNE_HOME` — and that a `grep` across `crates/` for
  `sandbox|chroot|landlock|seccomp|namespace|jail|bubblewrap|firejail` found
  no OS-level containment of an agent's child process. **Neither
  observation is evidence that an agent process can actually write outside
  its own worktree.** A worktree being a filesystem subdirectory of
  `$ARIADNE_HOME` says nothing about what permission check, if any, gates a
  write to a *sibling* path (`$ARIADNE_HOME/install.env`) from a process the
  daemon spawned — that is a question about the daemon's and the agent CLI's
  own write-path handling, which this task did not instrument or test. The
  absence of an OS sandbox is likewise not proof that `auto` mode (the
  default permission mode) would let such a write through; `auto` is
  described in `docs/permissions.md` as choosing an allowing option when one
  exists, and `ai_permissions/derive.rs` separately derives an
  `outside_workspace` risk tag for a path-escaping action — but this audit
  did not drive that decision pipeline, and a risk-tag's *existence* is not
  evidence of what `auto` does with it, nor of same-user process capability
  translating into an actual permission bypass. Both are open questions,
  stated as such in "Design questions for the orchestrator" below, not
  treated as established facts anywhere in this finding.
- Independent of that open question, this audit did not identify any other
  in-scope route (a CI/release step, an installer step, a documented
  permission guarantee) that writes attacker-influenced content to
  `$ARIADNE_HOME/install.env`. If one exists, it was not found; its absence
  from this report is a coverage gap, not a claim that none exists.

**What this means for severity.** The sink (code execution + deletion +
forced purge, once the manifest is forged) is Confirmed and High-impact —
whoever can write that one file gets all three effects, trivially, as shown
below. Whether that is *reachable* from this audit's hostile-input threat
model, short of an attacker who already has an unrelated arbitrary-file-write
primitive against the user's own `$HOME` (at which point `install.env` is
one of many files that would turn that primitive into code execution — so
would `~/.bashrc`), is **unverified**. This finding should be read, and
prioritized, as "fix the sink regardless" rather than "a demonstrated
privilege escalation."

**Reproduction.** `security/audit/2026-10-12/08-release-and-install/repro-RELEASE-001.sh`,
self-contained: every invocation of `install.sh`/`uninstall.sh` runs under
`env -i` plus an explicit, complete set of `HOME`/`ARIADNE_HOME`/`ZDOTDIR`/
`XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`TMPDIR` pointed inside a throwaway
`mktemp` directory — nothing from the real caller's environment reaches the
scripts under test, and two "ambient sentinel" files (standing in for the
caller's *real* `ZDOTDIR/.zshrc` and `XDG_DATA_HOME` desktop entry) are
checked byte-for-byte unchanged at the end. An earlier version of this
script omitted `ZDOTDIR`/`XDG_DATA_HOME` from the child environment, which —
on a caller whose own shell exports either — let `uninstall.sh`'s
`ui_strip_block`/GNOME-entry removal reach the caller's real dotfiles; the
review that returned this report demonstrated exactly that hole
independently. Every assertion below is a hard `exit 1` on failure, not a
printed note. Run:

```sh
bash security/audit/2026-10-12/08-release-and-install/repro-RELEASE-001.sh
```

Expected behavior: a manifest is just configuration; `--dry-run` previews
without changing anything; `ARIADNE_APP`/`PURGE` only ever reflect what the
real installer or the command line set; nothing outside the fixture changes.

Observed behavior (sanitized; the real run reproduces this verbatim, exit
code 0):
```
== 0. deletion does not need code execution: a pure KEY="VALUE" manifest is enough ==
OBSERVED: Unrelated.app was deleted by a manifest containing only well-formed assignments —
          a parser that refuses to execute the file would still have read ARIADNE_APP and
          handed it, unchecked, to rm -rf.

== 0b. the same is true of ARIADNE_BASH_COMPLETION and ARIADNE_PLIST, independent of ARIADNE_APP ==
OBSERVED: both unrelated-completion (via ARIADNE_BASH_COMPLETION) and unrelated-plist (via
          ARIADNE_PLIST) were removed — two more manifest fields with no authorization check,
          not covered by an ARIADNE_APP-only fix.

== 1. uninstall.sh --dry-run ==
  [3/5] Removing ~/Documents
  ...
  Dry run - nothing was changed.
OBSERVED: .../PWNED exists — the injected line ran during --dry-run, which claims "nothing was changed".

== 2. uninstall.sh (the real run) ==
  [3/5] OK   Removing ~/Documents
  ...
OBSERVED: .../PWNED exists — the injected line ran again.
OBSERVED: .../home/Documents (the forged ARIADNE_APP) was deleted.

== 3. a forged manifest can also force --purge without the flag ==
  data      deleted (--purge)
  [5/5] OK   Deleting ~/.ariadne (database, worktrees, run dirs, logs)
OBSERVED: .../home2/.ariadne was deleted by a plain 'uninstall.sh' invocation — the forged
          manifest's PURGE=1 reached scripts/uninstall.sh:161-165 even though --purge was never
          passed on the command line.

== 4. isolation check ==
OK: ambient ZDOTDIR/.zshrc is byte-for-byte unchanged (cmp).
OK: ambient XDG_DATA_HOME desktop entry is byte-for-byte unchanged (cmp).
PASS: RELEASE-001 reproduced (code execution, deletion with and without code execution,
      unauthorized completions/plist deletion, and a forced --purge), fixture isolation held.
```

**Counter-evidence / what would invalidate this finding.** If no agent task,
even a prompt-injected one, can ever write outside its own worktree (an
answer this audit did not obtain — see design question 1), the prerequisite
for reaching the sink collapses to "the attacker already has an
arbitrary-file-write primitive against the user's own `$HOME`," at which
point sourcing `install.env` adds no capability an attacker with that
primitive did not already have. The sink would then be better described as
"the installer is one of many ambient files that amplify an unrelated
file-write bug into code execution and deletion," which is still worth
fixing (defense in depth, plus the uniquely bad `--dry-run` behavior and the
forced-`--purge` escalation) but would not itself be the root cause of a
privilege crossing.

**Remediation — split into two independent fixes, per the review's
correction.**

1. **Stop executing the manifest.** Read `install.env` as data: a
   `KEY="value"` line parser (e.g. a `while IFS='=' read -r key value` loop
   that strips quotes and only accepts the fixed set of keys `install.sh`
   actually writes — `ARIADNE_PREFIX`, `ARIADNE_BASH_COMPLETION`,
   `ARIADNE_ZSH_COMPLETION`, `ARIADNE_PLIST`, `ARIADNE_UNIT`, `ARIADNE_APP`,
   `ARIADNE_DESKTOP_ENTRY`, `ARIADNE_DESKTOP_ICON`), discarding — not
   executing — any other line. This closes effect (1) only.
   - *Acceptance criterion:* a manifest line that is not one of those eight
     `KEY="value"` assignments never executes (a sentinel file a test
     places in such a line, as `repro-RELEASE-001.sh` scenario 1/2 do, must
     never be created), and the well-formed keys that *are* present are
     still read correctly.
2. **Authorize deletions independently of parsing, for every field the
   parser accepts — not only `ARIADNE_APP`.** A value read out of the
   manifest must not be trusted as a deletion target just because it
   parsed. The prior revision of this proposal covered only `ARIADNE_APP`/
   `ARIADNE_DESKTOP_ENTRY`/`ARIADNE_DESKTOP_ICON`/`ARIADNE_PREFIX`, with an
   allowed-roots list (`/Applications`, `$HOME/Applications`,
   `$XDG_DATA_HOME/applications`, `$XDG_DATA_HOME/icons/hicolor`) that
   would have **rejected a normal Linux install**: Linux's `ARIADNE_APP` is
   `$PREFIX/ariadne-desktop` (`scripts/install.sh:209-212`'s
   `install_app_bundle`, Linux branch — under the chosen prefix, not under
   any of those four roots), and it left `ARIADNE_BASH_COMPLETION`,
   `ARIADNE_ZSH_COMPLETION`, `ARIADNE_PLIST` and `ARIADNE_UNIT` with no
   check at all, even though all four reach a bare `rm -f` exactly the same
   way (`uninstall.sh:146`; `lib.sh:151` inside `ui_stop_daemon`'s Darwin
   branch; `lib.sh:157` inside its Linux branch). The review reproduced
   deletion of two unrelated files through the last two of these alone,
   with no `ARIADNE_APP` line in the manifest at all — reproduced again
   here, scenario 0b. The corrected policy below covers all eight fields
   the parser in fix 1 accepts:

   | Field | Rule |
   | --- | --- |
   | `ARIADNE_PREFIX` | Must be a non-empty absolute path (today an empty or relative value — e.g. a manifest written with `PREFIX=""` — would make `rm -f "$ARIADNE_PREFIX/ariadne"` resolve relative to the uninstaller's own working directory). Deletions from it use the fixed literal basenames `ariadne`/`ariadned` only (already true today, `uninstall.sh:108`) — the manifest never supplies a filename for this one. |
   | `ARIADNE_APP` | Canonicalize (see the symlink caveat below), then accept **either** shape: (a) basename exactly `"$APP_NAME.app"` (`Ariadne Desktop.app`) and canonical parent exactly `/Applications` or `$HOME/Applications`; **or** (b) basename exactly `ariadne-desktop` and canonical parent exactly the already-validated `$ARIADNE_PREFIX` — this is what preserves both a normal Linux install (default `$PREFIX`) and a custom `--prefix` one, per the review's note. |
   | `ARIADNE_BASH_COMPLETION` | Basename exactly `ariadne`, canonical parent exactly `${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions` (recomputed fresh from the current environment, the same way `ui_locations` already does — never trusted from the manifest for the *directory* half, only compared against it). |
   | `ARIADNE_ZSH_COMPLETION` | Basename exactly `_ariadne`, canonical parent exactly `${XDG_DATA_HOME:-$HOME/.local/share}/zsh/site-functions`. |
   | `ARIADNE_PLIST` | Basename exactly `$ARIADNE_PLIST_LABEL.plist` (`dev.ariadne.daemon.plist`), canonical parent exactly `$HOME/Library/LaunchAgents`. |
   | `ARIADNE_UNIT` | Basename exactly `ariadned.service`, canonical parent exactly `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user`. |
   | `ARIADNE_DESKTOP_ENTRY` | Basename exactly `$ARIADNE_DESKTOP_ID.desktop` (`dev.ariadne.ui.desktop`), canonical parent exactly `${XDG_DATA_HOME:-$HOME/.local/share}/applications`. |
   | `ARIADNE_DESKTOP_ICON` | Basename exactly `$ARIADNE_DESKTOP_ID.png` (`dev.ariadne.ui.png`) — **not** a fixed parent, since the directory encodes the icon's size, read from the PNG at install time by `icon_size()` and otherwise defaulted to `256x256`: accept canonical path matching the pattern `${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/<digits>x<digits>/apps/dev.ariadne.ui.png`, i.e. every component fixed except one `<width>x<height>` directory matched by a digits-x-digits pattern, not a fixed string. |
   | `PURGE` | Not one of the parser's accepted keys at all (per fix 1) — it is a command-line-only flag; a `PURGE=` line in the manifest is simply never read. |

   **Symlink/TOCTOU caveat, stated honestly rather than claimed away —
   corrected a second time.** The prior revision of this proposal said
   canonicalizing "so a symlink cannot redirect the check after it passes"
   — that overstates what canonicalization buys. Resolving the path with
   `realpath`/`readlink -f` *before* the basename/parent check closes the
   case where the manifest names a symlink that already points somewhere
   disallowed at check time; it does **not** close the case where a path
   component is swapped *between* the check and the later `rm`/`rm -rf` —
   a classic check-then-use race, reachable by an attacker who can write
   into an ancestor of the path being validated. The prior revision also
   named two supposed mitigations for this, both wrong: BSD/macOS `rm`'s
   `-P` flag overwrites a file's contents before unlinking it (a
   secure-delete behavior) and has nothing to do with symlink or
   physical-path resolution; and `O_NOFOLLOW` only stops an `open()` call
   from following a symlink at the *final* path component — it does
   nothing to stop an **ancestor** directory component of the same path
   being swapped to a symlink, which is exactly the race this caveat is
   about. Both are withdrawn rather than corrected, since no portable,
   `bash 3.2`-expressible primitive this audit found actually closes an
   ancestor-directory swap race. The honest statement is simply that this
   residual race is not closed by this remediation, full stop — an open
   limitation, not a risk the user has reviewed and accepted (no such
   acceptance has happened; it is an unresolved question, the same way
   RELEASE-001's end-to-end reachability is, not a settled one).
   - *Acceptance criteria, both directions:*
     - **Rejection (negative) cases:** an `ARIADNE_APP` value that is
       syntactically a valid path but names something other than the
       expected app bundle under an allowed root (scenario 0's
       `Unrelated.app`) is refused, not deleted; an `ARIADNE_BASH_COMPLETION`
       or `ARIADNE_PLIST` value naming an arbitrary unrelated file
       (scenario 0b) is likewise refused; a `PURGE=1` line in the manifest
       (scenario 3) has no effect on whether `--purge` runs.
     - **Acceptance (positive) cases, so the fix does not break ordinary
       use:** a manifest written by an unmodified `install.sh` for a
       default install (`ARIADNE_PREFIX="$HOME/.local/bin"`,
       `ARIADNE_APP` absent or, on Linux, `"$HOME/.local/bin/ariadne-desktop"`)
       is accepted and every field it names is still removed; the same for
       a custom `--prefix /opt/ariadne/bin` install
       (`ARIADNE_APP="/opt/ariadne/bin/ariadne-desktop"` on Linux) — this is
       the case the prior revision's proposal would have silently broken.

**Compatibility decision needed (design question 2, unchanged by this
correction):** when a manifest value fails the authorization check in fix
2, should `uninstall.sh` refuse the whole run (safer, but a hand-edited
manifest for a legitimate reason — e.g. a user who moved `$PREFIX` by hand
without re-running `install.sh` — could start failing) or skip only that
one removal and continue the rest of the uninstall (friendlier, but could
silently leave a legitimate removal undone)? This needs the user's answer.

---

#### RELEASE-004 — `$PREFIX` (and `$PATH`) are interpolated unescaped into the generated `systemd` unit and `launchd` plist; a crafted value injects a new service directive or plist key

**Severity: High.** **Confidence: Confirmed (reproduced: generation of both
formats proven; the systemd execution consequence is supported by
`systemd.service(5)`/`systemd.syntax(7)`'s documented grammar, not an
executed service — see "Limitation" below).**

The prior revision of this report rejected this as a hypothesis, reasoning
"nothing downstream of `$PREFIX`/the app name is `eval`'d — they are written
into a `.desktop`/plist file as data, not executed." That reasoning is
wrong: `launchd` and `systemd` are themselves parsers of the files
`install.sh` generates. A value that is never passed to a shell can still
break out of *that parser's* grammar if it is interpolated unescaped, the
same class of bug as an HTML/SQL injection where "no `eval`" was never the
relevant guarantee.

**Affected files/lines**
- `scripts/install.sh:658-673` (Linux): the `systemd --user` unit,
  `Environment="PATH=$PATH"` and `ExecStart=$PREFIX/ariadned`, both
  unescaped
- `scripts/install.sh:612-641` (Darwin): the `launchd` plist,
  `<array><string>$PREFIX/ariadned</string></array>` and
  `<key>PATH</key><string>$PATH</string>`, both unescaped
- `$PREFIX` comes from `--prefix DIR` or the `PREFIX` environment variable
  (`scripts/install.sh:64,75`); `$PATH` comes from the invoking shell's
  environment at install time

**What the code does, and the reproduction.** `security/audit/2026-10-12/08-release-and-install/repro-RELEASE-004.sh`
extracts the *exact* heredoc install.sh writes today, by `sed` line range
read from the real file at run time (not a hand copy — if a future edit
moves these lines, the script fails loudly with a clear error rather than
silently testing stale code), and replays it with a crafted `$PREFIX`:

- **systemd unit:** `PREFIX` = `"$WORK/out/prefix\nExecStartPre=-/usr/bin/touch $WORK/out/UNIT-INJECTED\n#"`.
  The generated unit file then contains `ExecStartPre=-/usr/bin/touch
  .../UNIT-INJECTED` as its own line (confirmed with `grep -qxF`), with the
  trailing `/ariadned` from the template absorbed into a `#`-comment line.
  `systemd.syntax(7)` (primary source, quoted above): "Empty lines and
  lines starting with `#` or `;` are ignored" — confirming the injected
  line is read as a directive, not folded into a comment, and the template's
  own leftover text is what becomes the harmless comment.
  `systemd.service(5)` (primary source, quoted above): multiple
  `ExecStartPre=` lines "are executed one after the other, serially," and
  "`ExecStart=` commands are only run after all `ExecStartPre=` commands
  that were not prefixed with a `-` exit successfully" — confirming that,
  on a live service, the injected command would run (every time the service
  starts, including the `systemctl --user restart ariadned.service`
  `install.sh` itself runs at `scripts/install.sh:677`) with the invoking
  user's privileges.
- **launchd plist:** `PREFIX` =
  `/tmp/x</string></array><key>INJECTED</key><string>yes</string><key>Ignored</key><array><string>X`.
  The generated plist is then **well-formed XML** (not merely corrupted) —
  chosen deliberately, since a value that produces a *parse error* is a
  weaker demonstration than one a validating reader accepts. `python3`'s
  `plistlib.load` parses the result into a dict holding an attacker-named
  top-level key, `INJECTED: 'yes'`, and a `ProgramArguments` value the
  installer never intended (`['/tmp/x']` instead of
  `['$PREFIX_real/ariadned']`).

Both are reproduced in one run:
```sh
bash security/audit/2026-10-12/08-release-and-install/repro-RELEASE-004.sh
```
Sanitized output (exit code 0):
```
OBSERVED: a second, syntactically valid directive (ExecStartPre=...) was injected as its own line.
          systemd.unit(5) runs every ExecStartPre before ExecStart; had this been a live
          'systemctl --user restart ariadned.service' ..., that command would have run with
          the invoking user's privileges, every time the service starts.
...
OBSERVED: the parsed plist now has an attacker-named top-level key: INJECTED='yes'
          ProgramArguments is now: ['/tmp/x'] (not the single ariadned path install.sh intended)
PASS: RELEASE-004's generation-time injection reproduced for both the systemd unit and the launchd plist.
```

**Limitation, stated explicitly.** No real `systemd --user` or `launchd` was
run against either generated file — doing so is out of this audit's scope
("do not... change the host service"). The claim that the injected
`ExecStartPre=` line would actually execute rests on the two primary-source
citations above, not on an observed service start. The plist claim is
stronger in this respect, since `plistlib` (a real, independent plist
parser, not this audit's own code) did parse the result and did produce the
attacker-named key — that half is observed, not only documented.

**Reachability.** This does **not** depend on the open cross-boundary
question RELEASE-001 does. `$PREFIX` is attacker-influenceable through two
ordinary, in-scope routes: the `--prefix` flag an operator types (which
could come from copy-pasted install instructions on a hostile fork's
README, forum post, or an AI agent's prompt-injected suggestion — all named
as hostile sources in this audit's threat assumptions), or the `PREFIX`
environment variable, which an untrusted `.envrc`/dotfile sourced into the
installing user's shell could set without the user ever typing `--prefix`
at all. Neither requires any pre-existing write access to `$ARIADNE_HOME`
or any permission-model bypass. `$PATH` is equally unescaped in the same two
templates, though it is a weaker vector in practice (a directory name would
need to itself contain a newline or XML metacharacter to matter, which is
unusual but not impossible — e.g. a project-local `.envrc`-style PATH
addition with a crafted directory name).

**Relationship to RELEASE-003.** Related but distinct: RELEASE-004 is about
the *generated file's structure* being corruptible by an unescaped value;
RELEASE-003 (below) is about a *syntactically ordinary* `$PATH` entry being
captured verbatim and later resolved by bare-name lookups. Fixing
RELEASE-004 (escaping/validating `$PREFIX`/`$PATH` for each target format)
does not fix RELEASE-003 (a risky-but-well-formed `$PATH` directory still
gets captured and still gets resolved later); both inputs are shared, but
the defect and the fix are independent.

**Remediation — corrected.** The prior revision of this proposal rejected
(for `$PREFIX`) only a newline or a NUL byte, and escaped (for `$PATH`
only, not `$PREFIX`) just `&`/`<`/`>` for the plist. That proposal is
insufficient on its own terms: the plist payload actually proven above to
inject `INJECTED='yes'` —
`/tmp/x</string></array><key>INJECTED</key><string>yes</string><key>Ignored</key><array><string>X`
— contains **neither a newline nor a NUL byte**. Rejecting those two bytes
in `$PREFIX` would have let that exact payload through unchanged. The
corrected fix escapes **both** `$PREFIX` and `$PATH`, for **each** target
format, rather than rejecting a narrow byte set in one of them:

- **For the plist:** XML-escape (`&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`)
  both `$PREFIX` and `$PATH` before interpolating into `<string>...</string>`.
  This alone closes the reproduced payload, since it relies entirely on
  literal `<`/`>` characters.
- **For the systemd unit:** per `systemd.syntax(7)`'s confirmed quoting
  grammar (quoted above: quotes wrap "a whole item," opening only at the
  start of an item or after unquoted whitespace; items are whitespace-
  separated; C-style escapes are supported, including `\n` for a literal
  newline, `\"` for a literal double quote, and `\\` for a literal
  backslash), a **newline-rejection-only** fix is not enough either,
  independent of the plist issue: a value containing an unescaped `"` can
  still end an item early and change how the rest of the line is
  tokenized, with no newline involved at all — the review's point that
  "its `Environment=` directive also interprets quotes, escapes, and
  multiple assignments." The correct fix is to represent the value as a
  single systemd "item" regardless of its content: wrap it in double
  quotes and backslash-escape, in order, every literal backslash (`\` →
  `\\`), every literal double quote (`"` → `\"`), and every literal
  newline (converting the control byte to the two-character sequence `\n`,
  since a *physical* line break in the unit file always starts a new line
  for the parser regardless of quote state — only the textual escape
  sequence represents an embedded newline *within* one item). This is not
  independently confirmed for every systemd directive from a single
  fetchable source (see the `systemd.exec(5)`/`Environment=` gap recorded
  under "Primary sources consulted" above), but it follows directly from
  the general grammar `systemd.syntax(7)` does confirm, which governs every
  unit-file assignment, `Environment=` and `ExecStartPre=`/`ExecStart=`
  alike.
- `install.sh` already refuses other invalid combinations outright (e.g.
  `--build-from-source` with `--version`); a defense-in-depth refusal of a
  `$PREFIX` containing a NUL byte (which no filesystem path can legitimately
  contain, unlike a newline — POSIX filesystems do not forbid a literal
  newline in a filename, so "no legitimate path can contain one" was itself
  an overstatement in the prior revision, now corrected) can still be kept
  *alongside* proper escaping, as a belt-and-suspenders check, but escaping
  is what actually closes the finding — rejecting bytes is not a substitute
  for it.

**Regression criterion.** Regenerating the unit/plist from a `$PREFIX` or
`$PATH` containing the exact reproduced payloads above (the XML payload
with no newline/NUL, and a value containing an unescaped `"`) produces a
file with exactly the directives/keys `install.sh` intended — no second
`ExecStartPre=` line or altered `Environment=` tokenization, no unexpected
top-level plist key — and the *literal* value (once correctly escaped) is
still what a human or `plistlib`/`systemd-analyze verify` reading the file
back would see as the original string, not a mangled one. Verifiable by
re-running `repro-RELEASE-004.sh` against the fixed script and asserting
its `grep`/`plistlib` checks now fail to find the injected content
(inverting today's assertions), plus a new case using the quote-based
(not newline-based) payload this correction adds.

**Design question for the orchestrator (restated below with the corrected
remediation):** proper escaping should make an unusual `$PREFIX` install
correctly rather than fail outright — but should `install.sh` warn when it
had to escape something unusual, or do so silently? See design question 4
below for the full framing, including the separate NUL-byte question.

---

### Hardening suggestions

#### RELEASE-002 — `gh attestation verify` is not pinned to the release workflow's identity, and checks neither the expected source ref nor digest

**Severity: Low (no demonstrated exploit under the repo's current
permissions).** **Confidence: Plausible for the gap itself; the proposed
remediation's two flags are Confirmed to work as described, verified
against a real, already-published release (below), not only read from the
manual.**

**Affected:** `scripts/install.sh:410` —
`gh attestation verify "$_asset" --repo "$RELEASE_REPO"`.

This is the **only** check the installer enforces before installing a
downloaded binary — see "Available vs. enforced" below.

**Correction to the prior revision's proposal.** `gh attestation verify`'s
manual (`https://cli.github.com/manual/gh_attestation_verify`, fetched;
quoted here verbatim) documents these identity-restricting flags:

| Flag | What it enforces | Expected value |
| --- | --- | --- |
| `--signer-workflow` | "the workflow that signed the attestation matches the provided value" | `[host/]<owner>/<repo>/<path>/<to>/<workflow>` — **repository-qualified**, e.g. `github.com/manuel-alvarez-alvarez/ariadne/.github/workflows/release.yml` or, since `--repo` is already given, `$RELEASE_REPO/.github/workflows/release.yml` |
| `--cert-identity` | "the certificate's SubjectAlternativeName matches the provided value **exactly**" | an exact string, not a pattern |
| `-i`, `--cert-identity-regex` | "...matches the provided **regex**" | a regular expression — this, not `--cert-identity`, is the regex-capable flag |
| `--source-ref` | "the git ref associated with the source repository matches the provided value" | e.g. `refs/tags/v0.3.0` |
| `--source-digest` | "the digest associated with the source repository matches the provided value" | a commit SHA |

The prior revision proposed `--signer-workflow .github/workflows/release.yml`
(missing the owner/repo prefix the manual requires) and conflated
`--cert-identity` with the regex-matching flag. Both are corrected above.

**Why `--source-ref`/`--source-digest` matter, concretely, and why a
signer-path check alone does not close the gap the review flagged.**
`release.yml`'s `workflow_dispatch` path lets an operator with repository
write access rerun the asset build "by hand... against a tag" (the
workflow's own header comment, `.github/workflows/release.yml:34-40`). The
checked-out *content* always matches `inputs.tag` (`ref:
${{ env.TAG }}` at the checkout step, `release.yml:99`), but the *workflow
run's own ref* — what the attestation's provenance records as "where this
ran from" — is whatever ref the dispatch itself was started on, which can
differ from the tag. `release.yml:88-94` already detects and warns about
exactly this:
```
if [ "$REF" != "refs/tags/$TAG" ]; then
    echo "::warning::this run is on $REF, so $TAG's assets will be attested as built from $REF; rerun it from refs/tags/$TAG"
fi
```
— but only warns; nothing stops the build, the attestation, or the upload.
Pinning `--signer-workflow` alone would still accept such an asset, since it
*was* built by `release.yml`, just from the wrong ref. Only `--source-ref
"refs/tags/$RESOLVED_TAG"` (checked by the installer at verify time, against
the tag it is actually trying to install) would catch this specific,
realistic operator-error scenario. This is a process/tooling gap
(a maintainer's mistake going uncaught by automated verification), not an
externally-exploitable one — reaching `workflow_dispatch` already requires
repository write access — so it stays a hardening suggestion, not a
confirmed vulnerability; but the review is correct that a signer-path check
alone does not close it, and this revision no longer implies that it would.

**Why no attacker path exists today regardless.** Only `release.yml`'s
`build` job declares `id-token: write`/`attestations: write`
(`release.yml:60-63`); `ci.yml` and `release-please.yml` declare neither
(`release-please.yml` additionally sets `permissions: {}` at the workflow
level and acts under a PAT, not `GITHUB_TOKEN`). Reaching that job requires
either a maintainer-driven release publish or `workflow_dispatch`, both of
which already require repository write access — so there is no demonstrated
path for a lower-privileged actor to forge an accepted attestation today.
Pinning the signer now is about not depending on that remaining true forever
(a future workflow added with those permissions for an unrelated reason
would otherwise be silently trusted by the installer too).

**Remediation.** Add `--signer-workflow "$RELEASE_REPO/.github/workflows/release.yml"`
and `--source-ref "refs/tags/$RESOLVED_TAG"` to the `gh attestation verify`
call in `scripts/install.sh:410`.

**Verified for real, not only read from the manual.** The prior revision of
this report said testing real attestation verification "would require
publishing a release... against the real repository" and declined to do
so. That prerequisite was wrong: `gh attestation verify` is read-only
against GitHub's public attestation API, and this repository already has
published releases (`gh release list` shows `v0.11.0` as latest, down to
`v0.2.0`) — no new release, workflow run, or write of any kind was needed.
This audit downloaded one real asset and ran the real command, read-only,
in a scratch directory removed immediately after:
```sh
gh release download v0.11.0 --repo manuel-alvarez-alvarez/ariadne \
  --pattern 'ariadne-aarch64-apple-darwin.tar.gz' --dir /tmp/release-verify-check
gh attestation verify /tmp/release-verify-check/ariadne-aarch64-apple-darwin.tar.gz \
  --repo manuel-alvarez-alvarez/ariadne
```
This passed (exit 0) exactly as `install.sh` relies on. Decoding the
certificate `gh attestation verify --format json` returns shows its
Subject Alternative Name is literally
`https://github.com/manuel-alvarez-alvarez/ariadne/.github/workflows/release.yml@refs/tags/v0.11.0`
— confirming the corrected `--signer-workflow` value format above
character-for-character. Testing the proposed flags against this same real
asset confirms both the format and the mechanism:
```
$ gh attestation verify ... --signer-workflow "manuel-alvarez-alvarez/ariadne/.github/workflows/release.yml"
(exit 0 — passes)
$ gh attestation verify ... --signer-workflow "manuel-alvarez-alvarez/ariadne/.github/workflows/ci.yml"
Error: verifying with issuer "sigstore.dev"
(exit 1 — rejected)
$ gh attestation verify ... --source-ref "refs/tags/v0.11.0"
(exit 0 — passes)
$ gh attestation verify ... --source-ref "refs/heads/main"
Error: expected SourceRepositoryRef to be refs/heads/main, got refs/tags/v0.11.0
(exit 1 — rejected, with the exact mismatch message this report's
`--source-ref` rationale above describes)
```
This is real, observed evidence — not only the CLI manual's documented
flag semantics — that both proposed flags work exactly as this
remediation assumes, against this repository's actual release pipeline.

**Regression criterion.** An attestation produced by a workflow at a
different path, or from a ref other than the tag being installed (both
reproducible with a stand-in `gh`, the way `scripts/tests/install-command.sh`
already stands in for `gh attestation verify` — or, as just shown, against
a real asset and a real wrong flag value), is rejected; one produced by
`release.yml` itself, from the matching tag, still passes.

#### RELEASE-003 — The installing user's live `$PATH` is baked verbatim into the persistent daemon service

**Severity: Low (depends on a pre-existing unsafe `$PATH` at install time;
not attacker-controlled by this flow alone — see RELEASE-004 for the
related-but-distinct injection concern over the same inputs).**
**Confidence: Plausible.**

**Affected:**
- `scripts/install.sh:625` (Darwin): `<key>PATH</key><string>$PATH</string>`
  inside the generated `launchd` plist
- `scripts/install.sh:666` (Linux): `Environment="PATH=$PATH"` inside the
  generated `systemd --user` unit

Both are deliberate, documented behavior (`scripts/install.sh:623-624,665`:
"launchd services get a bare PATH ... so bake in the installing user's
PATH") — `ariadned` needs `tmux`, `git` and agent CLIs resolved the way the
user's own shell resolves them. The installer copies that `$PATH` with no
filtering. If the invoking shell's `$PATH` at install time contains a
relative entry, a world- or group-writable directory, or any directory an
untrusted party can place a same-named binary into, that entry is captured
into a service definition that starts at login and persists across shell
sessions and reboots. Every binary the daemon or an agent it spawns resolves
by bare name (`git`, `cargo`, `npm`, `python3`, `nvidia-smi`, `gh`, `glab` —
`crates/ariadne-daemon/src/config.rs` resolves several of these from `PATH`
specifically when no explicit path is configured) is then subject to
hijacking by whatever that captured entry pointed at, for as long as the
service runs.

**Remediation.** At minimum, warn (not block — consistent with the
installer's own idiom of warning rather than failing on a PATH that
excludes `$PREFIX`, `scripts/install.sh:769-772`) when a component of
`$PATH` is relative or world/group-writable, naming the offending entry,
before baking it into the plist/unit.

**Regression criterion.** Installing with a `$PATH` containing a
world-writable directory prints a named warning; the generated plist/unit
still contains exactly the caller's `$PATH` (so nothing already relying on
an unusual `$PATH` entry silently breaks) unless the user opts into
filtering.

**Design question (unchanged from the prior revision, reworded per the
review's note).** Should a risky `$PATH` entry only ever be warned about, or
should install.sh offer to filter it out of what gets baked into the
service definition? This audit does not treat the installer's existing
warn-don't-block idiom elsewhere as a binding requirement that any fix here
must also stay non-fatal — that is a real option, not a constraint, and the
choice needs the user's answer alongside RELEASE-001 question 2 and
RELEASE-004's new question.

---

### Unverified candidates

- **Whether an installed agent task can write outside its own worktree,
  specifically to `$ARIADNE_HOME` itself.** This is RELEASE-001's load-bearing
  open question; see that finding's "Reachability" section and design
  question 1 below. Restated plainly: this audit did not confirm or refute
  it, and does not treat either the absence of an OS sandbox or the
  existence of an `outside_workspace` risk tag as evidence either way.
- **Floating-tag GitHub Actions in `ci.yml`** (`actions/checkout@v5`,
  `dtolnay/rust-toolchain@stable`, `taiki-e/install-action@v2`,
  `Swatinem/rust-cache@v2`, `actions/setup-node@v5`) are a generic
  supply-chain surface (a compromised upstream Action release could run
  arbitrary code during CI); already documented and deliberately accepted
  in `.github/AGENTS.md` ("`ci.yml` uses floating tags ... since nothing
  there is attested"). The advisory check below covers the current state of
  these specific Actions and the SHA-pinned ones in `release.yml`; it does
  not re-litigate the pinning policy itself, which is arguably SUPPLY's
  territory if revisited.

### Rejected hypotheses

- **"A fork PR's CI run could abuse an implicit write-scoped
  `GITHUB_TOKEN`."** Rejected, with the prior revision's justification
  corrected. GitHub's default is a read-only `GITHUB_TOKEN` for
  `pull_request` (not `pull_request_target`) runs triggered by a fork — but
  this is **not unconditional**: GitHub documents an exception, "Send write
  tokens to workflows from pull requests," quoted from
  `docs.github.com/.../workflow-syntax`: "You can use the `permissions` key
  to add and remove `read` permissions for forked repositories, but
  typically you can't grant `write` access. The exception to this behavior
  is where an admin user has selected the **Send write tokens to workflows
  from pull requests** option in the GitHub Actions settings." That setting
  is itself restricted, per
  `docs.github.com/.../managing-github-actions-settings-for-a-repository`
  ("Enabling workflows for forks of private repositories"): "Available to
  private repositories only." This repository's visibility was checked, not
  assumed (`gh repo view --json visibility` →
  `{"visibility":"PUBLIC"}`), so the exception does not apply here, and the
  rejection's conclusion stands for this specific repository — but the
  general claim "GitHub hard-codes a read-only token for fork PRs, full
  stop" was inaccurate, and is withdrawn in favor of this conditional
  version. Separately, `ci.yml` references no `secrets.*` anywhere, and a
  same-repo-branch PR with a hypothetically write-scoped token would belong
  to an author who already has push access, so grants no privilege they
  lack. Explicit `permissions: contents: read` at the top of `ci.yml` would
  still be good defense-in-depth practice (a safeguard against a future
  visibility change, a future `pull_request_target` mistake, or an org
  default change) but is not fixing a reachable vulnerability today.
- **"The release archives could carry a path-traversal or symlink
  payload that escapes the staging directory on extraction."** Rejected.
  `scripts/install.sh` verifies every file in the staging directory with
  `gh attestation verify` (looping over `"$STAGE_DIR"/*`,
  `scripts/install.sh:409-412`) *before* any `tar -xzf` of it
  (`scripts/install.sh:427,461,469`), and the attestation subject is a
  content digest, so a tampered archive fails verification rather than
  reaching `tar`. The archives themselves are produced by the trusted
  `release.yml` build job with fixed, reviewed contents, so there is no
  untrusted producer of archive entries to test a symlink/traversal payload
  against, short of a compromised CI run — a different, already
  covered-by-pinning-and-permissions threat, not an extraction-logic bug.
- **~~"`install.sh`'s desktop-entry or plist generation is injectable via
  `$PREFIX` or the app name, rejected because nothing is `eval`'d."~~
  Withdrawn, not merely corrected — this was wrong, as the review showed.**
  See RELEASE-004 above, now a confirmed finding of exactly this shape.

## Available vs. enforced (required distinction)

| Thing | Available | Enforced by `install.sh` before installing |
| --- | --- | --- |
| Build provenance attestation | Yes, on every release asset (`release.yml`'s "Attest the release assets" step, `release.yml:268-271`) | **Yes** — the sole gate, `gh attestation verify "$_asset" --repo "$RELEASE_REPO"` for every file in the staging directory (`scripts/install.sh:409-412`); a failure calls `ui_die`, which exits before any `tar`/`install -m 755`. Not pinned to the specific signer workflow or source ref — see RELEASE-002. |
| SHA-256 checksums (`checksums-<target>.txt`, published in the Release body table by `scripts/release-notes.sh`) | Yes, one line per asset, human-readable on the Release page | **No.** `install.sh` never downloads a `checksums-*.txt` file or compares a digest; the table exists for a human to cross-check by hand, not for the script to enforce. |
| macOS code signature (self-signed "Ariadne Code Signing") | **Conditional**, for release assets: only when `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD`/`APPLE_SIGNING_IDENTITY` are all populated. If any is empty, `release.yml:163-167` prints `::warning::macOS signing secrets are unavailable; releasing unsigned macOS assets` and ships the asset unsigned rather than failing the job — "available" is not "guaranteed." Local `--build-from-source` builds are signed only when `make-signing-cert.sh` has put the identity in the login keychain. | **No, for downloaded assets, under any circumstance.** `install.sh` never calls `codesign --verify` on a downloaded asset. The *only* place `install.sh` touches `codesign` at all is signing (not verifying) a local `--build-from-source` build, at `scripts/install.sh:366-370`. `codesign --verify --deep --strict` exists only in `release.yml:213-219`, where CI verifies its own freshly-signed build *before upload* — that is a CI-side check on what gets published, not something the installer enforces on what gets downloaded. The prior revision of this table cited `install.sh:213-219` for a verify step; that was a transcription error — those line numbers are `release.yml`'s, not `install.sh`'s, and `install.sh` contains no `codesign --verify` call anywhere. |
| Quarantine clearing | N/A (a side effect, not a check) | `clear_quarantine` runs unconditionally *after* attestation succeeds, never as a substitute for it. |

The documentation (`docs/install.md:100-118`) matches this table: it states
only that "the installer verifies each file with `gh attestation verify`
before installing it," and separately that the signing certificate is
self-signed/not notarized. No doc claims checksum verification is enforced.
No gap was found between what is documented and what the code enforces; the
gap, if any, is that the single enforced check (RELEASE-002) is not pinned
as tightly as it could be.

## Release-flow evidence (added per review)

A concise trace of the parts of the release pipeline not covered by a
specific finding above, so the input-to-sensitive-operation account is
complete rather than finding-shaped:

- **Cache isolation.** `ci.yml`'s `rust`/`tauri` jobs and `release.yml`'s
  `build` job both use `Swatinem/rust-cache`, scoped per-OS/per-target key.
  GitHub's own cache-scoping rules (fetched from
  `docs.github.com/.../caching-dependencies-to-speed-up-workflows`):
  "Workflow runs can restore caches created in either the current branch or
  the default branch... Workflow runs cannot restore caches created for
  child branches or sibling branches," and a fork PR's run "can also
  restore caches created in the base branch" (read access to `main`'s
  cache) but writes from a fork PR's own branch stay scoped to that
  branch/fork — they are not restorable by `main` or by unrelated PRs. No
  path was found for a hostile fork PR to poison a cache that a trusted
  `main` build or `release.yml` (which does not share a cache scope with PR
  branches at all) would later restore.
- **Checkout credential persistence.** `release.yml`'s two checkout steps
  both pass `persist-credentials: false` (`release.yml:100,308`) explicitly
  — the ambient job token is not left in the local git config for later
  steps to use. `ci.yml`'s `actions/checkout@v5` steps pass no such option,
  so the default (`persist-credentials: true`, confirmed from
  `github.com/actions/checkout`'s own README: "The token is persisted in
  the local git config. This enables your scripts to run authenticated git
  commands. The token is removed during post-job cleanup.") applies: the
  job's `GITHUB_TOKEN` sits in `.git/config` for the rest of the `ci.yml`
  job, available to anything that job runs (`cargo build`/`test`/`clippy`,
  `npm ci`/`test`, all of which execute arbitrary third-party build/test
  code). For a fork PR this token is read-only regardless (see "Rejected
  hypotheses"); for a same-repo-branch PR, whatever write scope the ambient
  token has is exposed to that untrusted build-time code for no operational
  reason (`ci.yml` never runs an authenticated `git` command itself). This
  is a real gap against least-privilege, though — as with RELEASE-002/003 —
  not a demonstrated escalation, since a same-repo-branch PR author already
  has the write access the token would grant. Logged here as release-flow
  evidence rather than a separate numbered finding, since fixing it is a
  one-line `persist-credentials: false` addition to `ci.yml` with no
  separable remediation design of its own.
- **Manual (`workflow_dispatch`) input.** `release.yml`'s `inputs.tag` is an
  unvalidated string, used both as the checkout `ref:` and as the
  `gh release upload`/`gh release edit` target name (`release.yml:82,99,277,299`).
  Triggering `workflow_dispatch` at all requires `actions: write` on the
  repository (ordinary write-access collaborators and above) — the same
  privilege level needed to push a malicious tag in the first place, so this
  input is not an escalation path for a lower-privileged actor. Its one
  concrete risk — a maintainer dispatching from the wrong ref, producing an
  attestation whose recorded source ref does not match the tag it is
  released under — is exactly the scenario `release.yml:88-94` already
  warns about and RELEASE-002's `--source-ref` remediation would catch at
  install time.
- **Secret availability and scope, accounted for completely:**
  - `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD`/`APPLE_SIGNING_IDENTITY`
    — read only by the macOS legs of `release.yml`'s `build` job
    (`release.yml:158-181`), conditionally (see the signature row in
    "Available vs. enforced" above). Rotation and scope are documented in
    `.github/RELEASING.md`'s "The signing certificate" section; this audit
    did not re-derive that beyond confirming the three secrets are read in
    exactly the step that section describes.
  - `RELEASE_PLEASE_TOKEN` — a fine-grained PAT, per `.github/RELEASING.md`
    scoped to "Only select repositories → this one," with `Contents:
    read/write` and `Pull requests: read/write` and nothing else (no
    `Workflows` permission, so it cannot itself alter a workflow file).
    Blast radius if leaked: this one repository's contents and PRs, not the
    account's other repositories.
  - `GITHUB_TOKEN` — ambient, scoped per job by the `permissions:` block
    declared for that job: `contents: write`/`id-token: write`/
    `attestations: write` for `release.yml`'s `build` job
    (`release.yml:60-63`), `contents: write` only for its `notes` job
    (`release.yml:296-297`), none declared (workflow default) for `ci.yml`,
    and `permissions: {}` (none at all — the PAT does everything) for
    `release-please.yml`. Each is scoped to the single job and repository,
    and expires when the job ends.
  - **The release-upload token's authority, specifically — corrected a
    second time.** The prior revision said the job's `permissions:` block
    (`contents: write`, `id-token: write`, `attestations: write`,
    `release.yml:60-63`) means "any code running at *any* step of that job
    ... has that same token available via the ambient `GITHUB_TOKEN`/OIDC
    environment for the job's entire duration." That conflated four
    distinct things the review asked to be told apart, and was wrong about
    the one that matters most for the `npm ci`/`tauri build` scenario the
    next point discusses:
    - **Permissions** (the `permissions:` block) set the *scope* GitHub
      will honor if and when a token derived from them is actually used —
      they are an authorization ceiling, not a delivery mechanism. Granting
      `contents: write` does not, by itself, put a credential anywhere.
    - **Action context/input** — a `uses:`-based action step (not a `run:`
      shell step) can read `${{ github.token }}` through its own
      `action.yml`-declared input defaults (several first-party actions,
      including `actions/attest-build-provenance`, default a
      `github-token`-shaped input to `${{ github.token }}`) without the
      *workflow* needing to map anything. That is scoped to the action's
      own process, not exported to any other step's shell.
    - **Explicitly mapped `GH_TOKEN`/`GITHUB_TOKEN`** — a `run:` step's
      shell process receives a context value as a literal environment
      variable only when the workflow's own `env:` (or `with:`) block maps
      it there. `release.yml:275` does exactly that, and *only* for the
      "Upload the release assets" step
      (`env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`); no other `run:` step
      in the job — including `npm ci` and `npm run tauri build`, the two
      the next point is about — has any such mapping, so their process
      environment never contains `GITHUB_TOKEN`/`GH_TOKEN` at all,
      regardless of the job's `permissions:` scope. This is confirmed from
      the runner's own source, not inferred: `Runner.Worker/GitHubContext.cs`'s
      `_contextEnvAllowlist` — the fixed set of `github.*` fields the
      runner will turn into environment variables for a step —
      **excludes `token`** by name; `secrets.*` values reach a step's
      environment only through that step's own explicit `env:`/`with:`
      mapping, never automatically.
    - **Separate OIDC request credentials** — this is the one piece that
      *is* genuinely ambient to every step in the job once `id-token:
      write` is granted, confirmed in the same runner source:
      `Runner.Worker/Handlers/ScriptHandler.cs` sets
      `Environment["ACTIONS_ID_TOKEN_REQUEST_URL"]` and
      `Environment["ACTIONS_ID_TOKEN_REQUEST_TOKEN"]` directly, for every
      script step's process, independent of any workflow `env:` mapping.
      Those two variables are a *request* credential: whoever holds them
      can ask GitHub's OIDC provider to mint a short-lived JWT for the
      job's identity (what `attest-build-provenance` itself does) — they
      are not a REST API token and grant no direct `contents`/`attestations`
      write access on their own.

    So the accurate statement, for the next point's scenario specifically:
    code running during `npm ci`/`tauri build` has **no** `GITHUB_TOKEN`/
    `GH_TOKEN` in its environment (confirmed by the allowlist exclusion
    above) and **no** direct `contents`/`attestations` API access; what it
    *does* have, ambiently, is the ability to request an OIDC JWT for the
    job's identity via the two `ACTIONS_ID_TOKEN_REQUEST_*` variables —
    narrower than this report's prior two revisions both claimed, in
    different directions. `gh release upload ... --clobber` overwrites a
    same-named asset on that one release, using the credential explicitly
    mapped at `release.yml:275`; since attestation happens immediately
    before upload within the same job run (`release.yml:268-277`), a
    `--clobber`'d asset carrying a matching, freshly-produced attestation
    is simply what a normal, successful run of this workflow produces —
    not, by itself, a security guarantee of any kind; see the next point
    for what such a match does and does not establish about the content
    being attested.
- **Build execution timing relative to the macOS signing keychain —
  narrowed to what ordering actually establishes.** On the macOS legs of
  `release.yml`'s `build` job, `cargo build --release --locked --package
  ariadne-cli --package ariadne-daemon` (`release.yml:153-154`) is
  *invoked* before the signing certificate is imported
  (`release.yml:156-181`); `npm ci` (`release.yml:183-185`) and `npm run
  tauri build -- --bundles app` (`release.yml:188-194`) are both invoked
  after the import and before the keychain is deleted
  (`release.yml:279-286`, guarded by `if: always()`). That is the full
  extent of what step ordering establishes: **when each command starts**,
  nothing about containment between them. All of these steps run as the
  same user, in the same job, on the same runner filesystem, with no
  isolation boundary between steps in a GitHub Actions job — there is no
  sandbox separating what `cargo build` (invoked earlier) can leave behind
  from what `npm ci`/`tauri build` (invoked later, with the keychain
  unlocked) subsequently read or execute. The prior revision's "that build
  never has keychain access" was accurate only for *that step's own direct
  `codesign` usage*; it did not, and should not have been read to, rule out
  an earlier step planting a file (a modified source, a cached artifact, an
  environment variable persisted via `$GITHUB_ENV`) that a later,
  keychain-windowed step then reads or executes. This audit did not find
  such a planting mechanism in the current workflow and is not claiming one
  exists — only that "ordering" is not the same claim as "isolation," and
  the two should not be conflated. The concrete, established fact remains:
  `npm ci`/`tauri build` execute third-party dependency code (`npm`
  postinstall scripts; Tauri's own build pipeline) while the imported
  "Ariadne Code Signing" identity is unlocked and usable by `codesign`, and
  that is a residual, unmitigated window regardless of what does or does
  not happen in the earlier `cargo build` step. This is exactly what the
  previous point's "matching attestation" does **not** protect against:
  `actions/attest-build-provenance` attests that *this job, running this
  workflow, produced this artifact* — a true statement even if the job's
  own dependencies were hostile, since the attestation is generated from
  whatever `dist/*` actually contains after the build steps finish, not
  from any claim about those steps' inputs being trustworthy. A matching
  attestation is a strong, specific guarantee against **post-hoc tampering
  after the job completed** (swapping the asset before upload, or after);
  it is **not** a guarantee against **compromised code within the job
  itself**, which is precisely the scenario this keychain-window point is
  about. Evaluating the actual trustworthiness of those dependencies is
  SUPPLY's mandate, not re-done here; what this audit adds is the timing
  fact itself — which steps run
  inside that window — since that is `release.yml`'s own structure,
  squarely RELEASE's to report, stated as a residual risk rather than as
  something ordering disproves. No fix is proposed here (reordering would
  need `npm ci`/`tauri build` to run before cert import, which the
  `APPLE_SIGNING_IDENTITY` env var Tauri reads to self-sign depends on
  being available *during* the build — so this is a real structural
  tension, not a one-line change, and is flagged to the orchestrator as
  evidence rather than as a numbered finding with its own remediation).

## Action advisory check (per review: evidence, not a pinning-convention substitute)

Queried GitHub's own Security Advisory Database directly:
```sh
gh api "/advisories?ecosystem=actions&per_page=100"
```
Returned 56 advisories (no `Link` pagination header in the response, so this
is the complete current set under `ecosystem=actions`, not a truncated
first page). None of the 56 name any of the Actions this repository's
workflows use: `actions/checkout`, `dtolnay/rust-toolchain`,
`taiki-e/install-action`, `Swatinem/rust-cache`, `actions/setup-node`,
`actions/attest-build-provenance`, `googleapis/release-please-action`,
`mikepenz/action-junit-report`. This is a real, primary-source negative
result (as of 2026-10-12), not an absence of effort: an earlier attempt
using GitHub's web advisory *search* (fetched as a generic URL and
summarized) surfaced unrelated hits for two of these Actions' names — a
NuGet library (`GHSA-f5v8-v6q3-q4h6`, "Meridian.Mapping") that merely
*mentions* `actions/attest-build-provenance` as part of its own verification
process, and an unlocatable npm CVE number — neither of which actually names
a vulnerability in the Action itself. That search method was abandoned in
favor of the authenticated `gh api` call above once it proved unreliable.
This check does not cover the npm/cargo ecosystems (SUPPLY's scope) or the
unpinned floating-tag Actions' *future* risk (the pinning-policy question
already recorded under "Unverified candidates").

## Design questions for the orchestrator

1. **Cross-boundary (for EXEC / the permission-model owner), restated
   without overclaiming:** can an agent task's process write to a path
   outside its own worktree, e.g. `$ARIADNE_HOME/install.env` directly? This
   audit found no OS-level sandbox and noted that `ai_permissions/derive.rs`
   derives an `outside_workspace` risk tag for such an action, but
   **neither observation answers the question** — this audit did not drive
   the daemon's permission pipeline to see what `auto` mode (the default)
   actually does with a request carrying that tag. RELEASE-001's severity
   should be read as conditional on this answer, not as already reachable.
2. **Compatibility decision for RELEASE-001's remediation:** when a
   manifest value fails the deletion-authorization check (fix 2), should
   `uninstall.sh` refuse the whole run, or skip only that one removal and
   continue? See RELEASE-001 above for the tradeoff.
3. **Compatibility decision for RELEASE-003:** should a risky `$PATH` entry
   found at install time only be warned about, or filtered out of what gets
   baked into the service definition? Filtering could break a setup
   currently relying on such an entry (e.g. a `direnv`-managed interpreter).
4. **Compatibility decision for RELEASE-004, restated with the corrected
   remediation in mind:** proper escaping (wrap-and-backslash-escape for
   the unit, XML-escape for the plist) should make an unusual `$PREFIX`
   install correctly rather than fail — but should `install.sh` still warn
   when a `$PREFIX`/`$PATH` needed escaping (so an operator who did not
   expect a quote or newline in their own prefix finds out), or escape
   silently? Separately: should a `$PREFIX` containing a NUL byte (which no
   filesystem path can represent at all, unlike a newline) still be
   rejected outright as a belt-and-suspenders check even once escaping is
   correct? See RELEASE-004 above.

## Recommendation

Fix RELEASE-004 and RELEASE-001's two independent halves (parsing, and
deletion/purge authorization) together — they are the two confirmed
findings, and RELEASE-004 in particular needs no answer to design question
1 to be worth fixing now, since its reachability does not depend on it.
Treat RELEASE-002, RELEASE-003 and the release-flow evidence items (`ci.yml`
`persist-credentials`) as follow-on hardening. Answering design question 1
should happen in parallel, since it determines whether RELEASE-001 is
"fix the sink, which is good practice regardless" or "fix an actively
reachable privilege crossing" — a prioritization input, not a blocker to
starting the fix.

## Confidence

- RELEASE-001: **High** confidence in the sink itself (reproduced six ways:
  code execution under `--dry-run`, code execution on a real run, deletion
  from a manifest with *no* injected command at all via `ARIADNE_APP`, the
  same via `ARIADNE_BASH_COMPLETION`/`ARIADNE_PLIST`, a forced `--purge`
  with no `--purge` flag, and byte-for-byte isolation holding throughout).
  **Low** confidence either way on end-to-end reachability from an in-scope hostile
  input — explicitly unverified, not merely "unstated."
- RELEASE-004: **High** confidence in the generation-time injection for
  both formats (reproduced, with the plist half independently parsed by a
  real, separate plist implementation). **Medium** confidence in the
  systemd execution consequence specifically, since it rests on documented
  grammar rather than an executed service — stated as a limitation, not
  elided.
- RELEASE-002: **Medium** confidence the gap is worth fixing as hardening
  (no demonstrated exploit path under the repository's current
  configuration); **High** confidence the proposed fix itself works,
  verified against a real, already-published release rather than only the
  manual. RELEASE-003: **Medium** confidence, **low** urgency, same
  reasoning as before.
- The release-flow evidence items (cache isolation, checkout credentials,
  manual inputs, secret scope, the signing-keychain build window): **High**
  confidence in the facts stated (each is a direct code/doc citation or a
  primary-source quote); these are evidence, not independently-scored
  findings.
- Everything under "Rejected hypotheses": confident in each rejection as
  corrected above, with a specific counter-observation or primary-source
  citation for each.

## Pre-existing `.githooks/pre-push` failure — investigated, not a RELEASE finding

The second review's required-hook run (`.githooks/pre-push`, run against
this task's branch range rather than `--all`, log at
`/var/folders/d9/l1r5tzbs0plbdrhx7_hfkbnm0000gn/T/release-review2-rmi90jsr/pre-push.log`)
reported `cargo nextest run --workspace --profile ci` failing one test:
`kept_requests::the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side`
(`crates/ariadne-daemon/tests/it/kept_requests.rs:687`), with
`assertion left == right failed ... left: 16, right: 14`, among 1925 passed
and 11 slow. The orchestrator asked this task to investigate it against the
unchanged application baseline with a bounded, targeted run, keep the
original failure and log on record, follow the repository's own rule for a
failure that disappears on retry, and report a cause and a follow-up
proposal rather than fix it here. This section is that record. It is kept
separate from the RELEASE-001–004 findings above: it is a test-suite
reliability observation, not a security finding, and not evidence bearing
on the release/install question this task answers.

**The application baseline is confirmed unchanged.** `git status --porcelain`
is clean at this task's commit, and `git diff --stat
a17283486d5fe89591edfff08832712936d73030 -- crates/` is empty: this audit's
commits touch only `security/audit/.../08-release-and-install/`, never
`crates/`. The failure is not something this task's changes could have
caused.

**Diagnostic runs performed (all logged outside the worktree, under
`/tmp/release-flake-check/`):**

1. The single named test, run alone (`cargo nextest run -p ariadne-daemon
   -E 'test(the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side)'`):
   **passed**, in 0.87s.
2. The same single test, run 15 times in a row while 8 CPU-bound busy loops
   (each `(while kill -0 $$ 2>/dev/null; do :; done) &`, tied to the
   invoking shell and killed explicitly afterward, per the root
   `AGENTS.md`'s own busy-loop technique) competed for the machine's 16
   cores: **15/15 passed.** Raw CPU contention alone does not reproduce it.
3. The full `ariadne-daemon` crate's test suite (965 tests across its two
   test binaries — `it`, the integration suite, and its other binary are
   both included by `cargo nextest run -p ariadne-daemon`, not the `it`
   binary alone — using the same `ci` profile `pre-push` uses, one thread
   per core), run three times in a row with no change to any file in
   between:
   - Run 1: **1 failed** — the exact same test, the exact same assertion,
     the exact same `left: 16, right: 14`.
   - Run 2: **1 failed** — but a *different* test,
     `switch_stats::an_exhausted_session_that_auto_switches_writes_one_switch_fact`
     (`crates/ariadne-daemon/tests/it/switch_stats.rs:219`,
     `assertion left == right failed ... left: 0, right: 1`).
   - Run 3: **965/965 passed**, no failures at all.

**Independent corroboration, kept distinct from this task's own runs.**
The orchestrator separately reported two other agents' observations,
consolidated here rather than treated as additional runs of this task's
own diagnosis:
- An agent named VIEW independently observed the same two failures this
  task's own runs 1 and 2 produced — `kept_requests`'s 16-vs-14 and
  `switch_stats`'s 0-vs-1 — on its own run (log at
  `/tmp/view-review2-check.log`, outside this task's worktree). This is
  independent corroboration that both failures are real and reproducible
  by more than one agent on more than one run, which strengthens "this is
  a genuine, pre-existing intermittent failure" — it does not, by itself,
  add a new failure mode or identify a cause beyond what this task's own
  runs already showed.
- An agent named DATA reported all 1,926 tests passing at commit
  `9944c8fa` — a **different commit** than this audit's examined
  application baseline (`a17283486d5fe89591edfff08832712936d73030`) or any
  commit on this task's own branch, on conditions (runner, concurrency,
  load) this report has no visibility into. A clean run on a different
  revision and unknown runner conditions is not evidence that the failures
  observed on *this* revision were spurious or are resolved — it is simply
  one more clean-pass data point, consistent with the intermittency this
  section already documents (this task's own run 3 was clean too, on the
  identical revision). It is recorded here as counter-evidence to weigh,
  not treated as resolving anything: the two sets of runs differ in
  revision and in runner conditions this report cannot compare directly,
  so no conclusion is drawn from the difference beyond "intermittent
  failures are, by definition, not always observed."

**Characterization — stated as a bounded observation, not a proven cause.**
This is the repository's own definition of a failure that disappears on
retry (`crates/AGENTS.md`'s "What a reviewer flags": "A test that passed
only on a second run"), and the prescribed response — proving it under the
bounded busy loop — was followed (step 2 above) and held there. What can be
said from these five runs, without overreaching: isolated execution and
raw CPU contention alone did not reproduce a failure (steps 1–2); running
the full crate's suite did, on two of three attempts, each time a
*different* test. That is as far as this evidence goes. It is consistent
with timing sensitivity tied to full-suite concurrency (shared sockets,
file descriptors, or scheduler jitter across many real daemon processes at
once) and with CPU pressure specifically *not* being the operative
variable (step 2 isolated that one factor and it did not reproduce the
failure) — but five runs do not establish *which* shared resource or
mechanism is the necessary cause, only that concurrent execution of the
full suite, and not raw CPU load in isolation, correlates with the
failures observed. Two different tests failing on two different runs, with
a third run clean, is consistent with a class of flakiness in the suite's
concurrency handling rather than a single localized bug — again, a
pattern consistent with that explanation, not a proof of it.

**Correction: the "filter to `replies()`" idea from the prior revision is
not a sufficient regression-preserving fix — flagged by the review, with
counter-evidence this revision did not have.** That idea rested on a
misquote of the test's own doc comment: `kept_requests.rs:581-585` actually
says "no `gh` call follows either refusal" — not "no `gh pr comment` call,"
which is what the prior revision wrote. The broader wording is not
incidental. `specs/026-pull-requests.md` rule 16 (line 148) states it the
same way for the daemon itself: "The daemon refuses any other conversation
reply of the agent with 400, **and makes no forge call**." Its acceptance
criterion (line 294, naming this exact test) repeats it: "A second reply to
it gets 400 and makes no forge call. A reply to a comment of its own side
also gets 400 and makes no forge call." Both are unambiguous that a refused
reply must trigger *no forge call of any kind* — not merely no `gh pr
comment` call. Narrowing the assertion to `replies(&stub)` (which filters
to only `pr comment` invocations, `kept_requests.rs:625-633`) would make
the test blind to a real regression the spec explicitly forbids: a refused
reply that mistakenly calls `gh api` (e.g. an errant comment or review
re-fetch triggered by the refusal path itself, as opposed to the
background poll's own independent cycle) would pass this narrowed
assertion merely because that call is not `pr comment`-shaped. **This
report withdraws that proposal as a fix** and marks it, instead, as what it
actually was: an incomplete diagnostic observation about where the
*flakiness* enters (the unfiltered count includes the background poll's
own calls, which `:649-652`'s comment confirms polls the stub during this
same window), not a safe replacement for what the assertion needs to keep
checking.

**Revised follow-up proposal, consistent with the spec's "no forge call"
requirement.** The fix belongs on the *polling* side, not the assertion's
scope: separate or control the background poll's own forge activity so the
test can wait it out before taking its `before` snapshot (or before each
assertion), the same way `crates/AGENTS.md` already prescribes for this
exact shape of problem — "For something that must not happen, listen for
`QUIET`" and "For a forge fetch that must be over before the stub forge CLI
answers differently, wait for `stub.fetches()`... A `QUIET` listen only
bets that it was over." Concretely, a follow-up could make the test wait
for the background poll's own fetch-completion signal (however
`tests/it/common/forge.rs`'s `stub.fetches()`/`QUIET` machinery already
exposes that for other tests in this suite) immediately before each
`before = stub.invocations().len()`/assertion pair, so that no incidental
poll call can land inside the window — while the assertion itself keeps
counting *every* invocation, unfiltered, exactly as it does today, so a
genuine regression (any refusal-triggered forge call, `pr comment` or
otherwise) still fails the test. This is offered as a follow-up task's
starting point, not implemented here: this audit touches no application
or test code, and the acceptance for whatever fix is chosen must be that a
refusal which erroneously triggers `gh api` (not `gh pr comment`) still
fails the test — a criterion the previous revision's proposal would not
have met.

**Recommendation to the orchestrator.** Treat this as pre-existing,
unrelated to this audit's diff, and already present at the examined
application commit. It does not block this report's own acceptance
criteria (the report's own checks — `shellcheck`, `bash -n`, both
reproduction scripts — are unaffected and pass). A dedicated follow-up task
against `ariadne-daemon`'s integration-test concurrency handling, starting
from the `kept_requests.rs` instance above, is the right size and owner for
fixing it — not this audit.

## What was not checked (coverage gaps)

- This audit ran entirely on macOS (Darwin 25.6.0, the host's own OS),
  corrected here on two points the review flagged. `repro-RELEASE-001.sh`
  does **not** exercise Linux branches on this host: `uninstall.sh`'s own
  `OS="$(uname -s)"` resolves to Darwin, so `ui_stop_daemon` takes the
  `launchctl` branch every run; the `systemctl` stand-in on `PATH` is
  present but unused here. Sourcing the forged manifest (and thus the code
  execution and the `PURGE=1` override) does happen before `ui_stop_daemon`
  is called (`uninstall.sh:69` vs. its first step at `:98-100`) — but the
  *deletions* the forged values drive (`ARIADNE_APP`, the completions
  files) run in later steps, **after** that service-stop step, same as a
  normal uninstall; the prior revision's claim that deletion/purge precedes
  the service call was backwards. `repro-RELEASE-004.sh` does not depend on
  `uname` at all (see the corrected threat-assumptions bullet above) — it
  replays the Linux unit-generation heredoc by direct extraction, bypassing
  OS detection entirely rather than spoofing it. No Linux-specific *service
  manager* behavior (`systemctl --user`, GNOME `.desktop`/icon
  registration, the `libwebkit2gtk-4.1.so.0` probe) was executed against a
  real Linux host by either script. `scripts/tests/install-linux.sh`
  already covers that surface and was read, not re-run.
- No release was published and no workflow run was triggered by this audit
  (both would be mutating and are excluded by scope). A real, read-only
  `gh attestation verify` call *was* made, against this repository's own
  already-published `v0.11.0` release — corrected from the prior revision's
  claim that doing so "would require publishing a release," which was
  itself inaccurate: verification is read-only and this repository already
  had releases to verify against. See RELEASE-002 for the full output. What
  was *not* exercised is a `workflow_dispatch` or a new `release: published`
  event — that would mutate the repository and remains out of scope.
- No real `systemd --user` or `launchd` instance verified RELEASE-004's
  execution consequence; see that finding's explicit "Limitation."
- `make-signing-cert.sh` was read but not run (it mutates the login
  keychain's trust settings and needs a one-time interactive macOS
  authentication prompt per its own comments); no defect was found in it
  worth a disposable-keychain reproduction.
- Dependency advisories for `ui/package-lock.json`/`Cargo.lock` (npm/cargo
  ecosystems), and the actual trustworthiness of whatever `npm ci`/`cargo
  build` pull during the signing-keychain build window described above,
  were not assessed — SUPPLY's mandate, not re-audited here beyond the
  ordering fact itself.
- The daemon's actual `ai_permissions` decision for an `outside_workspace`-
  tagged request was not exercised end to end — design question 1, left
  open for EXEC/the permission-model owner.

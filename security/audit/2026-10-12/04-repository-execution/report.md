# EXEC — Audit repository inputs through Git worktrees and agent execution

## Summary of findings

This report answers whether hostile repository metadata or paths can cause
unintended commands, file access, or process behavior in Ariadne's Git,
worktree, and agent-launch path. It went through four review rounds and
one round of relayed reviewer evidence; this section states the final
result, and each finding below carries its own
corrections where an earlier claim was wrong.

**Confirmed**
- **EXEC-005 (High):** discovery resolves and checks an absolute agent
  path, but launch spawns the bare command name in a repository-controlled
  cwd. When the daemon's own `PATH` has a relative entry ahead of the real
  agent's directory, a same-named executable in the task or review
  worktree runs instead of the trusted agent. Requires a session launch or
  relaunch, and for a review session the user's explicit request to review
  that pull request.
- **EXEC-001 (Medium-High):** a registered repository's branch name reaches
  several `git` calls as an option-parseable argument. A branch named
  `--orphan` makes task worktree creation produce an unborn branch; with
  the `--end-of-options` marker added, a branch named `--force` still
  silently cuts the worktree from the current `HEAD`. Fixes are
  command-specific; `add_worktree` needs the base resolved to an object id
  first.
- **EXEC-006 (Medium):** the review worktree's "read-only" rule is one
  sentence of prompt text. The permission modes gate only what an agent
  submits as a permission request; the default `auto` mode selects an
  agent-labeled allowing option, else the first option, and cancels when
  none is offered. Nothing in the Git or worktree layer stops writes,
  commits, or explicit-refspec pushes.

**Unverified candidates:** EXEC-002 (forge-sourced remote reaches
`git fetch` without a scheme gate), EXEC-003 (a revived task agent can fall
back to the shared checkout), EXEC-004 (a forge-sourced branch name is
embedded unquoted in a ready-made shell command handed to the keeper
agent), EXEC-007 (a tampered registry index can choose an already-present
program and its arguments for a trusted agent id).

**Open gaps:** no live daemon end-to-end run for EXEC-003/004/005/006/007;
no SSH or `git://` remote; no Windows host; cancellation reachability of
the un-timed `git` subprocesses is untested.

Not every claim below is an independent reproduction — EXEC-002, parts of
EXEC-004, and EXEC-007 are source-traced rather than executed end to end,
and are labeled that way. Where a command was run, it is shown; search for
"Repro:" to find each one. All open compatibility decisions are routed to
the orchestrator and left undecided.

## Question

Can hostile repository metadata or paths cause unintended commands, file
access, or process behavior?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (branch
`audit-repository-inputs-through-git-ahcj9j`, base `main`). The worktree was
clean throughout; nothing in `crates/`, `specs/`, or elsewhere was modified
for this audit — only `security/audit/2026-10-12/04-repository-execution/`
is new.

Git used for every reproduction below: `git version 2.55.0` (this machine's
installed `git`, run only in disposable `mktemp -d` repositories — never
against the user's own repositories, daemon, or accounts). One reproduction
(EXEC-005) also used `rustc` to compile a throwaway, disposable binary
exercising the exact `std`/`tokio` `Command` primitive `acp.rs` calls; the
binary and its test directories were temporary and are not part of this
commit.

## Threat assumptions (as given)

- Repository content, forge content, browser/network content and other
  users' files are potentially hostile; the daemon's own code is the thing
  under review, not the user.
- Installed coding-agent *programs* are trusted once the user has explicitly
  selected/installed them. We are not trying to contain a deliberately
  malicious agent *process* (e.g. constrain its shell access) once it is
  legitimately running as itself. We *are* trying to find places where
  Ariadne's own code — independent of any agent's choices — builds a
  command, path, or process from hostile repository/forge content without
  validating it, **including** a place where that code lets something
  *other than* the intended trusted program run in the trusted program's
  place (EXEC-005) — that is a failure of Ariadne's own launch logic, not a
  question of whether the agent that (never) runs is well-behaved.
- The report must distinguish repository-controlled execution from
  user-configured commands and explicitly selected trusted programs (see
  [Additional acceptance criterion](#additional-acceptance-criterion)).

## Coverage

### Subprocess entry-point inventory

Every place this codebase starts a child process, with what controls its
argv, environment, cwd, and descriptors, and how it is torn down:

**Corrected this revision: there is no single wrapper that covers every
`git` subprocess, and `acp_discovery.rs` does not reuse `probe.rs::run`.**
Both were asserted in the prior revision without checking every call site;
here is what is actually there, verified call by call:

| Call site | Program source | cwd | Env | Descriptors | Cancel/cleanup |
|---|---|---|---|---|---|
| `gitwt.rs:42-63` (`GitManager::git`, used by *most* `git` operations in that file — see the next three rows for the exceptions) | literal `"git"` | caller-supplied `repo`/`worktree` path (internal or registered-repo path, never raw repo/forge text — see table in EXEC-001 for which *arguments* are repo/forge-sourced) | inherited, plus `GIT_TERMINAL_PROMPT=0` for a `push` | `.output()` (piped, captured, not inherited) | **not** `kill_on_drop`, **no** timeout — see the correction below |
| `gitwt.rs:165-178` (`branch_exists`), `gitwt.rs:250-260` (`has_commit`), `gitwt.rs:396-405` (`is_ancestor`) | literal `"git"`, constructed directly with their own `Command::new("git")`, **bypassing `GitManager::git` entirely** — only `has_commit` adds `--end-of-options`; the other two take `repo`/`ancestor`/`descendant` the same way `GitManager::git`'s callers do, with the same unguarded-argument profile discussed in EXEC-001 | same as above | inherited (no `GIT_TERMINAL_PROMPT` override — not needed, neither command can prompt for credentials) | `.output()`, same as above | same as above |
| `probe.rs:87-98` (`run`, used by `ariadne doctor`/`/v1/doctor` probes **only** — not by `acp_discovery.rs`, corrected below) | caller-supplied `program: &OsStr` (always a `PathBuf` from `which()`/a configured command, never repository content) | not set (inherits daemon's cwd) | inherited | `.output()` under `kill_on_drop(true)` and a 3s `tokio::time::timeout` | `kill_on_drop` plus the timeout guarantees no probe outlives the report that asked for it |
| `acp.rs:963-990` (`AcpRuntime::launch`, every ACP agent session process) | `launch.program: String` — **see EXEC-005**: for a discovered registry agent this is the *bare command name*, not the absolute path discovery resolved | `launch.cwd` — the task worktree, the PR review/keeper worktree, or the repository's primary checkout, **all either repository content or derived from it** | daemon's inherited env plus `launch.env` (session/launch/goal/task ids, `ARIADNE_SOCKET`, and for `codex-acp`, `INITIAL_AGENT_MODE=agent-full-access` — none of this is repository-sourced) | piped stdio; `process_group(0)` | `kill_on_drop(true)`; `take_down`/`kill_gated` (`acp.rs:1076-1198`) signal the whole process group and are launch-id-gated so a kill cannot land on a process that already replaced the one it targeted (verified in the sub-agent's pass, see EXEC-POS list) |
| `acp_discovery.rs:612-625` (its own `probe`, a distinct function, **not** `probe.rs::run`) | `Command::new(&entry.program)` — the resolved **absolute** path, correctly, with `args` straight from `entry.command`'s tail (which, for a *registry* agent, is ultimately sourced from the downloaded index document — see EXEC-007) | daemon's own `probe_cwd` | inherited, plus `apply_agent_launch_environment` | piped stdin/stdout, **null** stderr; `kill_on_drop(true)` | timeout-wrapped by its caller |
| `acp_discovery.rs:828-844` (`delete_opencode_catalog_session`) | `Command::new(&entry.program)`, absolute | daemon's own `probe_cwd` | inherited | stdout/stderr **null**; stdin **inherited from the daemon** (no `.stdin(...)` call at all) | timeout-wrapped (`command.status()` under `tokio::time::timeout`); `kill_on_drop(true)` |
| `acp_discovery.rs:880-900` (`list_stored_sessions`) | `program: &Path`, passed in by its one caller as `self.program_of(&agent.id)` — the resolved **absolute** path, correctly | daemon's own `probe_cwd` | inherited, plus `apply_agent_launch_environment` | piped stdin/stdout, null stderr; `kill_on_drop(true)` | timeout-wrapped by its caller |
| `acp_discovery.rs:266-280` (`download_index`) | not a process — `reqwest::Client` HTTPS GET | n/a | n/a | n/a | request-scoped `reqwest` timeout (`Timeouts::registry_download`, 30s) |

**What `GitManager::git` actually runs.** The shared wrapper is not limited
to read-only plumbing. Its callers in `gitwt.rs` run, among others:
- **repository and worktree mutations:** `worktree add` (lines 80, 82, 85,
  100, 122, including `-B`, which resets an existing branch),
  `checkout --force --detach` (136) followed by `clean -fd` (138),
  `worktree remove --force` (144), `worktree prune` (150), `update-ref`
  (296, 324), `merge --ff-only` (320), and `branch -D` (342);
- **network operations:** `fetch` (266) and `ls-remote` (in
  `remote_has_branch_tip`/`remote_branch_tip`);
- **reads:** `rev-list`, `status`, `rev-parse`, `diff`, `merge-base`,
  `symbolic-ref`, `remote get-url`.

The wrapper sets `GIT_TERMINAL_PROMPT=0` only when the first argument is
`push`; no caller in `gitwt.rs` passes `push`, so that branch is not
exercised by the operations above, and a `fetch`/`ls-remote` that needs
credentials is not protected from an interactive prompt by it.

**`GitManager::git` and its three direct-`Command::new` siblings provide no
process-level containment on cancellation.** None of these four call sites
sets `kill_on_drop(true)` or wraps the call in a `tokio::time::timeout`.
Per tokio's own documentation
(<https://docs.rs/tokio/latest/tokio/process/struct.Command.html#method.kill_on_drop>),
"by default, a child process will not be killed when the `Child` handle to
it is dropped." If the future that owns one of these spawns is dropped
(for example, if an `axum` handler's future is dropped on client
disconnect, or a future caller wraps one of these calls in a timeout), the
spawned `git` process keeps running, untracked, until it exits on its own —
and given the operation list above, that can be a `fetch`, a forced
worktree removal, or a ref update, not only a read. I did not find a
`tokio::time::timeout` wrapping any `gitwt.rs` call in
`crates/ariadne-daemon/src`, and did not verify whether this codebase's
`axum`/`hyper` setup actually drops a handler's future on client
disconnect. **Whether anything currently cancels these calls in practice,
and what a surviving operation would then do, are both unresolved.** No
exploit is inferred from the missing timeout alone; this is recorded as an
open gap with its mechanism and its untested reachability.

**Registry-download path.** `download_index` (`acp_discovery.rs:266-280`)
fetches `acp_registry_url` (default
`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, a
daemon-configured constant, never repository or forge content) over
`reqwest`'s default HTTPS client (certificate validation on, no
`danger_accept_invalid_certs`), calls `.error_for_status()` (rejects a
non-2xx response), decodes it as UTF-8, and accepts it only if
`acp_index::validate` can parse it as the expected JSON shape
(`acp_index.rs:34-37`) — which checks *shape*, not *authenticity*: there is
no signature or checksum pinning beyond TLS transport security. Nothing
from a registered repository or a pull request reaches this URL, client,
or validation, so this is **not a repository/forge-content vector**. It
**is** a network-content vector, which the threat model also names: the
accepted document supplies both the program names `which()` searches for
and the argument arrays passed to whatever it finds
(`acp_index.rs:172-195`), and discovery's `probe` starts the selected
program with those arguments before any ACP negotiation
(`acp_discovery.rs:612-625`). A tampered index can therefore influence
**which file executes and with what arguments**, constrained only to files
already present on the daemon's absolute `PATH` entries and, for one of
the three candidate kinds, to the claimed package's directories. The full
trace, its trust boundary (a TLS or endpoint compromise), and its
unproved status are in EXEC-007.

### Git configuration, hooks, and filters

Explicitly tested, since Work item 2 asks for it and the original report
only reasoned about it without evidence:

**Hooks are never transferred by clone or fetch, in a clean, isolated
environment.** `Repro:` (disposable `$HOME`, so this cannot read or write
the real user's own git configuration; a unique marker path inside the
same disposable directory, so this cannot delete or collide with another
user's file — corrected this revision, replacing a fixed `/tmp/...` path)
```sh
d=$(mktemp -d); export HOME="$d/home"; mkdir -p "$HOME"
cd "$d"
git init -q -b main evil && cd evil
printf 'echo HOOK_RAN > "%s/hook_marker"\n' "$d" > .git/hooks/post-checkout
chmod +x .git/hooks/post-checkout
echo content > f.txt && git add . && git -c user.email=t@t -c user.name=t commit -q -m init
cd "$d"
git clone -q "$d/evil" victim
test -f "$d/hook_marker" && echo BAD || echo "good: hooks are purely local"
ls "$d/victim/.git/hooks" | grep -v '\.sample$' || echo "(none but .sample files)"
```
Result: `good: hooks are purely local`, no non-sample files in the fresh
clone's `.git/hooks`. Ariadne's `add_worktree`/`add_detached_worktree`
(`gitwt.rs`) operate on the *registered checkout's own* `.git` (worktrees
share one `.git/hooks` with their common repository, per git's design), and
the only way a hostile hook script could exist there is if the user handed
Ariadne a checkout whose `.git/hooks` was populated by something other than
`clone`/`fetch` — e.g. unpacking a tarball of someone else's `.git`
directory directly, rather than cloning it. This is a real but narrow
residual vector, noted and not scored as a finding (it requires the user to
bypass normal git acquisition entirely).

**A tracked `.gitattributes` naming an undefined filter is inert, in a
clean, isolated environment.** `Repro:` (same isolation corrections as
above)
```sh
d=$(mktemp -d); export HOME="$d/home"; mkdir -p "$HOME"
cd "$d"
git init -q -b main repo && cd repo
echo "*.txt filter=evilfilter" > .gitattributes
echo secret > data.txt
git add . && git -c user.email=t@t -c user.name=t commit -q -m "tracked attrs"
cd "$d"
git clone -q "$d/repo" victim2
test -f "$d/filter_marker" && echo BAD || echo "good: filter is a no-op without local filter.<name>.* config"
```
Result: `good`. A `clean`/`smudge` filter command must be set in local
(never-transferred) git config (`filter.<name>.clean`/`.smudge`); a
repository's own tracked `.gitattributes` can *name* a filter but cannot
*define* one — **by default.** The next paragraph is the conditional case
where that stops being true.

**Conditional execution path: both become live, automatically, when the
local machine already has a relative `core.hooksPath` or a
`filter.<name>.smudge`/`.clean` preconfigured before the clone.** This
does not contradict the two results just above — it is the case neither
of them tests. `core.hooksPath` can be set to a path *relative to each
repository's own worktree root* (e.g. `.githooks`, a convention several
real projects use for exactly this reason), and `filter.<name>.smudge`
can be defined once, globally, for convenience (several real dev-tool
installers do this). Once either is set, it applies to *every* repository
the user ever clones afterward — including a hostile one — and a hostile
repository's own tracked content then supplies the actual payload (the
hook script, or the filter's trigger), not just a name that resolves to
nothing. I reproduced this independently, in a disposable `$HOME` never
touching the real user's git configuration, with a clean-`HOME` control to
isolate the effect: `Repro:`
```sh
d=$(mktemp -d); export HOME="$d/fake_home"; mkdir -p "$HOME"
git config --global core.hooksPath .githooks
git config --global filter.preconfigured.smudge \
  'printf "PRECONFIGURED_FILTER_RAN\n" > '"$d"'/filter_marker; cat'
cd "$d" && git init -q -b main seed && cd seed
mkdir -p .githooks
printf '#!/bin/sh\nprintf "HOOK_RAN\\n" > '"$d"'/hook_marker\n' > .githooks/post-checkout
chmod +x .githooks/post-checkout
echo "*.secret filter=preconfigured" > .gitattributes
echo payload > data.secret
git add . && git -c user.email=t@t -c user.name=t commit -qm "tracked hook+attrs"
cd "$d" && rm -f "$d"/hook_marker "$d"/filter_marker
git clone -q "$d/seed" victim
echo "hook fired: $(test -f "$d/hook_marker" && echo yes || echo no)"
echo "filter fired: $(test -f "$d/filter_marker" && echo yes || echo no)"
# control: a clean HOME with neither global setting
unset HOME; export HOME="$d/clean_home"; mkdir -p "$HOME"
rm -f "$d"/hook_marker "$d"/filter_marker
git clone -q "$d/seed" victim_clean
echo "hook fired (clean HOME): $(test -f "$d/hook_marker" && echo yes || echo no)"
echo "filter fired (clean HOME): $(test -f "$d/filter_marker" && echo yes || echo no)"
```
Result: `hook fired: yes`, `filter fired: yes` with the preconfigured
`$HOME`; `hook fired (clean HOME): no`, `filter fired (clean HOME): no`
with the control — isolating the effect to the preexisting global config,
not anything the repository alone supplies. **This is not a repository-
content vector by itself** (the repository still cannot *create* the
dangerous local prerequisite — it can only supply the payload once that
prerequisite already exists), so it is not filed as its own EXEC finding,
but it is a real, conditional path from "register/clone a hostile
repository" to arbitrary local execution, gated entirely on a
preexisting local git configuration choice. **Hardening suggestion:**
`ariadne doctor` could cheaply warn when `core.hooksPath` is set (globally
or in the registering user's config) to a relative path, since that
specific, otherwise-convenient setting is what turns any future hostile
clone's tracked content into automatic local execution.

**Git's own upstream security advisories were checked against a primary
source** (`gh api repos/git/git/security-advisories`,
<https://github.com/git/git/security/advisories>, queried live during this
audit) for the class most relevant to "hostile repository": clone/checkout
vulnerabilities. The May 2024 batch (all published 2024-05-14) is:

| CVE | Summary | Vulnerable | Patched |
|---|---|---|---|
| CVE-2024-32002 | Recursive clones on case-insensitive, symlink-capable filesystems → RCE via a submodule tricking git into writing a hook into `.git/` | ≤2.39.3, ≤2.40.1, ≤2.41.0, ≤2.42.1, ≤2.43.3, 2.44.0, 2.45.0 | 2.39.4, 2.40.2, 2.41.1, 2.42.2, 2.43.4, 2.44.1, 2.45.1 |
| CVE-2024-32004 | RCE while cloning a specially-crafted local repository | same ranges | same versions |
| CVE-2024-32020 | Local clone lets an untrusted user modify objects in the new repo at will | same ranges | same versions |
| CVE-2024-32021 | Local clone may hardlink arbitrary user-readable files into `objects/` | same ranges | same versions |
| CVE-2024-32465 | Protections for cloning untrusted repositories can be bypassed | same ranges | same versions |

(Exact ranges/versions as returned by the GitHub Security Advisories API for
each GHSA id, not paraphrased.)

**Applicability to this codebase:** per the advisory text itself, CVE-2024-
32002's mechanism explicitly requires `git clone --recurse-submodules` (or
an equivalent submodule-populating step); CVE-2024-32004's advisory
describes the vulnerable operation as cloning. `gitwt.rs` never calls `git
clone` and `add_worktree` (`gitwt.rs:70-89`) never passes
`--recurse-submodules` or otherwise populates submodules — the *user's own*
initial acquisition of the checkout (before registering it with Ariadne) is
where a clone-time exploit would have to land, which is outside Ariadne's
own invocations. I did not find a concrete path by which Ariadne's own
`git worktree add`/`fetch`/`diff` calls reproduce the clone-time
precondition. This is a **coverage/hygiene gap, not a confirmed finding**:
`ariadne doctor`'s git-version floor (`crates/ariadne-cli/src/commands/
doctor/checks.rs:332`, 2.42, chosen for orphan-worktree support per spec
002 item 7) does not also assert the point release is ≥2.42.2, so a user on
exactly 2.42.0 or 2.42.1 passes the floor check while remaining exposed to
this batch for their *own* clones (not for anything Ariadne itself does).
Recommended as a cheap doctor-check addition; not filed as its own EXEC
finding because I could not show Ariadne's own code reaches the vulnerable
operation.

**The 2025 batch, checked this revision (the review correctly flagged the
prior pass as covering only May 2024):**

| CVE / GHSA | Summary | Vulnerable | Patched |
|---|---|---|---|
| CVE-2025-48384 / `GHSA-vwqx-4fm8-6qc9` | Arbitrary code execution through broken config quoting — a submodule path with a trailing CR is mis-read on checkout, and if a symlink points that altered path at the submodule's own hooks directory, a tracked `post-checkout` hook can run during the checkout | ≤2.43.6, 2.44.0–2.44.3, 2.45.0–2.45.3, 2.46.0–2.46.3, 2.47.0–2.47.2, 2.48.0–2.48.1, 2.49.0, 2.50.0 | 2.43.7, 2.44.4, 2.45.4, 2.46.4, 2.47.3, 2.48.2, 2.49.1, 2.50.1 |
| CVE-2024-52006 / `GHSA-r5ph-xg7q-xfrp` | Newline confusion in credential helpers (notably .NET/Node.js-based ones) can exfiltrate a stored credential to the wrong host | ≤2.40.3, 2.41.0–2.41.2, 2.42.0–2.42.3, 2.43.0–2.43.5, 2.44.0–2.44.2, 2.45.0–2.45.2, 2.46.0–2.46.2, 2.47.0–2.47.1, 2.48.0 | 2.40.4, 2.41.3, 2.42.4, 2.43.6, 2.44.3, 2.45.3, 2.46.3, 2.47.2, 2.48.1 |

(Checked live via `gh api repos/git/git/security-advisories/<ghsa-id>`
against <https://github.com/git/git/security/advisories>, same check date
as the May 2024 batch above; ranges/versions as returned by the API, not
paraphrased.)

**Applicability, assessed the same way as the May 2024 batch, including
the negative result:** CVE-2025-48384's own advisory names its workaround
as "avoid recursively cloning submodules in untrusted repositories" — the
same precondition (submodule recursion on a hostile repository) that
already ruled the May 2024 batch's clone-time class out of Ariadne's own
invocations, for the same reason: `gitwt.rs` never recurses submodules.
**Not applicable to Ariadne's own code**, by the same negative result.

CVE-2024-52006 is different in kind and *is* plausibly relevant: it is
about a **credential helper** (an external program, e.g. Git Credential
Manager — the default on Windows Git installations — supplying stored
auth tokens to git over a line-based protocol), not about clone-time
checkout mechanics. `GitManager::git` (`gitwt.rs:42-63`) sets
`GIT_TERMINAL_PROMPT=0` for a `push` (to stop an *interactive* credential
prompt) but does not disable or validate anything about whichever
credential helper the host's git configuration already names, and
`fetch_head_of`'s `from` argument (EXEC-002: forge-sourced, not
independently validated at this sink) is exactly the kind of
attacker-reachable URL this class of advisory is about. **This is a real,
plausible cross-reference to EXEC-002, not a new independent finding**: it
does not change EXEC-002's own evidence or severity, but it gives a
concrete, named mechanism (rather than only "unvalidated URL, unclear
impact") for what an unvalidated `from` could be used for *if* the host
also runs a vulnerable git version and a CRLF-confusable credential
helper — both conditions outside what this pass could check (no
credential helper was configured in any disposable reproduction; I did not
determine which credential helpers are in common use among Ariadne
hosts). Recorded as a sharpened, better-cited version of EXEC-002's
existing "contingent on FORGE/host configuration" framing, not a severity
change.

### Reviewed in full
`crates/ariadne-daemon/src/gitwt.rs`, `branch.rs`, `checkpoint.rs`,
`launcher.rs`, `probe.rs` (ariadne-core), `acp_registry.rs`
(ariadne-store), `crates/ariadne-store/src/repositories.rs`,
`crates/ariadne-store/src/tasks.rs` (branch-name slugging),
`crates/ariadne-daemon/src/http/repositories.rs`, `http/channel.rs`
(diff endpoint), `http/steps.rs` (gates), `http/pull_requests.rs` (diff
endpoint + `is_sha`), `scheduler/pull_requests.rs` (merge-landing),
`agents/mod.rs` (`link_skills`), `forge/mod.rs` (`Remote::parse`/`rewrite`),
`acp_index.rs` (`installed`/`searchable`/`found`/`installs`),
`acp_discovery.rs` (`entries`, `command_of`, `program_of`, `download_index`,
full file for this revision), `specs/002`, `specs/007`.

A background sub-agent, briefed with the same threat model, independently
read and reported on: `acp.rs` (3637 lines), `acp_calls.rs`,
`acp_transport.rs`, `agents/{acp,handoff,prompts,mod}.rs`,
`ariadne-core/src/workflow.rs`, plus follow-ups it made itself into
`forge/github/pulls.rs`, `forge/gitlab/pulls.rs`, `learned_key.rs`,
`http/skills.rs`, `ariadne-store/src/skills.rs`. Its claims are cited below
only where I independently verified them against source or reproduced them
myself; one of its framings (no discovery-to-launch TOCTOU window) was
**wrong** and is corrected by EXEC-005 below — the sub-agent read
`command_of`'s *doc comment* ("what the launcher runs... by its stable id")
and the *existence* of the absolute `program` field, but did not trace that
`command_of` itself (the one function `launcher.rs` actually calls) returns
the bare `command`, not `program`. This revision was caught by the human
review, not by me independently before submission — recorded here so the
gap in my own verification process is visible, not papered over.

### Tested boundaries (live reproductions, disposable repos/binaries only)
- Whether a git branch name can carry a leading `-`/`--`, and separately
  shell metacharacters, via a wholly ordinary `git push` (no nonstandard
  forge, no bypassing normal git commands) — confirmed yes for both, via
  `git push <remote> HEAD:refs/heads/<crafted name>`.
- For every `gitwt.rs` call shape that takes a bare branch/ref/sha argument,
  whether git treats a crafted value as (a) an error, (b) a silently
  *different but valid* operation, or (c) a literal value — tested
  individually per sink, including options each sink's *own* documented
  flag set actually offers (not just one guessed payload), see EXEC-001.
- Whether `--end-of-options` (as opposed to `--`) preserves each sink's
  correct behavior on a legitimate input *and* still rejects a crafted one
  — tested for `worktree add`, `branch -D`, `rev-list --count`,
  `ls-remote --heads`, `diff`, `checkout --detach`, and `rev-parse`. It
  holds for `branch -D`, `rev-list --count`, `diff`, and
  `checkout --detach`; it is unproven-but-harmless for `ls-remote`; it
  breaks `rev-parse`'s output unless combined with `--verify --quiet`; and
  it does **not** hold for `worktree add -b`, where a real branch named
  `--force` silently produces a worktree at the current `HEAD`. That sink
  instead needs the base resolved to an object id first (see EXEC-001).
- Whether `git`'s `ext::` transport is reachable through `GitManager::git`
  — confirmed blocked by this git's own default.
- Whether the daemon's ACP `initialize` call advertises `fs/*` or
  `terminal/*` capabilities, and whether any handler for them exists —
  confirmed neither (independently re-verified via `grep`, not only the
  sub-agent's word).
- Whether a sibling directory name can be mistaken for a registered
  repository when resolving a loose session's permission mode — confirmed
  not (`Path::starts_with` is component-wise).
- **New this revision:** whether the actual program `AcpRuntime::launch`
  spawns can be substituted by repository content when the daemon's
  ambient `PATH` contains a relative entry — confirmed yes, with a direct
  `std::process::Command` reproduction of the exact primitive `acp.rs`
  uses (EXEC-005).
- **New this revision:** whether git preserves a tracked file's executable
  bit through an ordinary clone — confirmed yes (needed as a prerequisite
  check for EXEC-005).
- **New this revision:** hooks-via-clone and gitattributes-filter-via-clone
  by default — both rejected — and, separately, confirmed *live* when a
  relative `core.hooksPath`/a `filter.<name>.smudge` is already configured
  locally beforehand, with a clean-`HOME` control isolating the effect
  (see "Git configuration, hooks, and filters").
- **New this revision:** whether a detached review worktree enforces
  "read-only" at the filesystem/git level independent of agent cooperation
  — confirmed it does not (tracked edits, a sibling-directory write outside
  the worktree, a commit, and an explicit-refspec push to a reachable
  remote all succeeded; the registered repository's own `main` ref was
  unaffected) — see EXEC-006.
- **New this revision:** whether `--end-of-options` alone preserves
  `remote_has_branch_tip`'s SHA comparison on its bare `rev-parse <branch>`
  call — tested and found it does **not** (plain `rev-parse
  --end-of-options <ref>` echoes the literal marker text as an extra
  output line, breaking the equality check for every legitimate branch);
  `--verify --quiet --end-of-options` (matching `has_commit`'s own existing
  pattern) does preserve it — see the correction in EXEC-001.

### Excluded, with reason
- **Containing a malicious *installed* agent binary's own behavior** once
  it is genuinely running as itself (tool calls it makes, what it does with
  a shell it already has) — explicitly out of scope per the task's agreed
  scope. (This is distinct from EXEC-005, where the real agent *never
  runs at all* — something else runs in its place.)
- **FORGE's own metadata ingestion/validation** — owned by the FORGE task.
  Where EXEC's sinks consume forge-sourced strings with no defense of
  their own, that is reported as cross-boundary evidence (EXEC-002,
  EXEC-004), not re-litigated as a FORGE defect.
- **PERM's permission-request answering** — owned by the PERM task.
- **DATA's persisted resume records** — owned by the DATA task.
- **A live SSH or `git://` remote.** No such remote was available in the
  sandbox; every reproduction used a local filesystem path as "the
  remote". This leaves one sub-question in EXEC-002 open rather than
  closed by testing.
- **A Windows host.** All process-spawn reproductions (EXEC-005
  specifically) were run on macOS/Darwin against POSIX `exec`-family
  semantics. Windows' `CreateProcess` search order for a bare executable
  name includes the calling process's own directory and, depending on a
  registry-level safe-search setting, the current directory, *independent
  of `PATH` content* — meaning the Windows-side risk profile for EXEC-005
  may differ (plausibly be broader, since it might not even need a
  crafted `PATH`) but I have no Windows environment to confirm this.
  Recorded as an explicit, unsupported-platform coverage gap, not asserted
  either way.
- **`agents/prompts.rs`'s full template-substitution logic** beyond the one
  skill-path join the sub-agent traced, and **`acp.rs` lines ~1600–2330**
  beyond a `Command::new`/`join(` grep sweep.
- **An end-to-end test of a tampered ACP registry index.** The download
  path is traced (see "Registry-download path" above) and filed as
  EXEC-007, an unverified candidate; no disposable registry endpoint was
  stood up to drive a daemon through it, so the full chain is untested.

## Additional acceptance criterion

**Repository-controlled execution vs. user-configured/trusted programs:**
the *program name* a session is pinned to run is never chosen by repository
or forge content — it always comes from `AgentRegistry::command_of`, fed by
either the vendored/downloaded ACP index filtered against the daemon's own
`PATH` (`acp_index.rs`), or the daemon's own `[[acp_agents]]`
configuration. Repository/forge content never supplies argv\[0\]. (The
downloaded index is *network* content rather than repository/forge
content; what a tampered index could do is EXEC-007.) **What
EXEC-005 shows is a different, narrower failure**: even though the *name*
Ariadne decides to run is never repository-controlled, the *file that
actually executes under that name* can be repository-controlled, because
the launch step re-resolves the bare name against `PATH` a second time,
in a cwd that repository content populates, without the filtering
discovery itself already does. That is still, precisely, "unintended
execution caused by hostile repository content" rather than "deliberate
execution of a trusted program" — the trusted program the daemon *believes*
it started never runs.

## Findings

### Confirmed

#### EXEC-005 — Discovery resolves and validates an absolute agent path, but launch spawns the bare command name in a repository-controlled cwd, letting repository content substitute the executed program

**Affected files/lines:**
- `crates/ariadne-daemon/src/acp_index.rs:19-32` (`Installed`, holding both
  `command: Vec<String>` — the bare name plus args, e.g. `["claude",
  "acp"]` — and `program: PathBuf` — the absolute, `PATH`-resolved,
  package-containment-checked file discovery actually found)
- `crates/ariadne-daemon/src/acp_discovery.rs:55-60` (`RegistryEntry`
  preserves both fields, built straight from `Installed`)
- `crates/ariadne-daemon/src/acp_discovery.rs:486-493` (`command_of`
  returns `entry.command.clone()` — the bare name; `program_of`, which
  would return the absolute `entry.program`, is `fn`-private and used only
  by `acp_discovery.rs:451` for probing, never by the launcher)
- `crates/ariadne-daemon/src/launcher.rs:319,328-329` (`launch`: `let
  command = self.registry.command_of(agent_id)...; let program =
  command.next()...` — `program` here is `entry.command[0]`, the bare
  name)
- `crates/ariadne-daemon/src/acp.rs:963-971` (`AcpRuntime::launch`:
  `Command::new(&launch.program).current_dir(&launch.cwd)...` — the bare
  name, in a cwd that is repository content or derived from it, with no
  `PATH` override, so the OS's own `PATH` search runs a second time,
  unfiltered, at exec time)

**Attacker-controlled input:** the *basename* of a file tracked in a
repository's branch (for a task/column-agent session, the task's own
worktree, cut from the registered repository's base branch — i.e. the
repository's own committed content) or in a pull request's head branch
(for a review session, whose worktree is checked out at the PR's head —
i.e. the content of whichever specific request the user has asked Ariadne
to review; **not** every open PR, and not anyone who merely opens one — see
the corrected reachability statement below).

**Reachability, corrected:** the prior revision claimed this is "reachable
by anyone who can open a PR against a repository Ariadne is configured to
review, with no local access to the daemon's host at all." **That
overstates it and is corrected here.** There is no repository-level
"configured for auto-review" mode that reviews every incoming PR. Per
`forge/poll.rs::wants_session` (the one function that decides whether a
request gets a review session at all):
```rust
pub(crate) fn wants_session(row: &PullRequest, integration: &ForgeIntegration) -> bool {
    let asked = match row.role.as_str() {
        "reviewer" => row.review_asked && row.review_requested && row.review_model.is_some(),
        _ => row.review_asked && row.review_model.is_some(),
    };
    asked && row.state == "open" && integration.enabled && !row.draft
}
```
every request needs `review_asked`/`review_model` (and, for a review the
user was asked for, `review_requested`) set on **that specific request's
own row** before anything happens — and spec 029 states the policy this
implements in one sentence: "gets a review session once the user asks
Ariadne for one — never on its own." The correct reachability statement
is: **the PR author does not control whether or when a review session
starts.** The user does, by explicitly asking Ariadne to review that
specific request — an entirely ordinary action (reviewing external
contributions is a primary reason this feature exists), but a required,
human-initiated one, not a standing "auto-review" exposure. Once the user
has asked, the *content* of that one request (which the PR's author does
control) is what reaches the worktree EXEC-005 is about — the finding's
mechanism is unchanged; only the "merely opening a PR is enough, with zero
interaction from the user" framing is retracted.

**Transformation:** none beyond what `git worktree add`/`checkout` already
does to populate the worktree from the branch's tree. A tracked file keeps
its executable bit through an ordinary clone/checkout (verified, see
below).

**Prerequisites, stated explicitly and in full (all three must hold):**
1. **A session for the affected seat must actually be launched or
   relaunched**, and, for a review/keeper seat specifically, the request
   must already have been asked for by name (above) — this is not automatic
   merely because a hostile repository is registered or a pull request
   exists. For a task/column-agent session this means
   `start_step_agent`/`agent_switch` (`launcher.rs`) actually spawning that
   task's agent in its worktree. For a review session it means
   `spawn_pull_request_session`/`resume_pull_request_session`
   (`launcher.rs:1648-1675`, and the resume path at `:1713-1741`) actually
   running — reached either directly (the user's own explicit review
   start) or through the scheduler's routine reconciliation
   (`scheduler/pull_requests.rs:start_pull_request_session`, gated by
   `wants_session` above) **continuing a review the user already asked
   for** — the scheduler does not independently decide to start reviewing
   something nobody asked about. All such paths converge on the same
   `Launcher::launch` (`launcher.rs:285-347`), which is also where the
   repository's `permission_mode` is resolved for every one of them (see
   EXEC-006) — the convergence is why this one fix point covers every seat.
2. **The daemon process's own ambient `PATH` environment variable must
   contain both (a) an absolute entry resolving to the real agent
   binary — needed for discovery to register the agent at all — and (b) a
   relative entry** (an empty entry, `.`, or any relative directory)
   **positioned *before* that absolute entry.** Corrected this revision:
   the prior wording additionally offered "or must simply lack an absolute
   entry for that name at all" as an alternative — that branch is not
   coherent and is withdrawn unless a separate, reachable configuration for
   it is found. `searchable()` (`acp_index.rs:65-76`) filters relative
   entries out *before* discovery's own `which()` call, so if the daemon's
   ambient `PATH` has no absolute entry for the agent's name at all,
   discovery itself finds nothing and the agent is never registered —
   `command_of` then returns `None` and `launch()` fails outright
   ("the ACP registry holds no agent `{agent_id}`"), never reaching the
   vulnerable spawn. Both (a) and (b) must hold together.
3. **The repository or pull-request content reaching that cwd must carry
   an executable file whose basename matches the pinned agent's bare
   command name** (e.g. `claude`, `codex`), with its executable bit set —
   confirmed to survive an ordinary clone (below).

Prerequisite 2 is outside repository/forge control entirely and is not
guaranteed to hold on any given deployment (see Severity). Prerequisite 3
is squarely inside this audit's threat model ("hostile repository
content"). Prerequisite 1 is an ordinary operational event for a
task/column-agent seat, but for a review/keeper seat it additionally
requires the user to have already, specifically, asked for review of the
one request that turns out to be hostile — a real but meaningfully
narrower bar than "anyone can open a PR," corrected above.

**Reproduction (exact primitive, independently compiled and run — this is
not a shell-level analogy, it is the same `std::process::Command` API
`tokio::process::Command` wraps on Unix):**
```rust
// repro.rs
use std::process::Command;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (cwd, path_var) = (&args[1], &args[2]);
    let out = Command::new("claude")          // bare name, as launcher.rs supplies
        .current_dir(cwd)                      // repository-controlled worktree
        .env("PATH", path_var)                  // the daemon's own ambient PATH
        .output().unwrap();
    print!("{}", String::from_utf8_lossy(&out.stdout));
}
```
```sh
mkdir -p trusted_bin malicious_repo_worktree
printf '#!/bin/sh\necho TRUSTED_BINARY_RAN\n' > trusted_bin/claude && chmod +x trusted_bin/claude
printf '#!/bin/sh\necho MALICIOUS_REPO_BINARY_RAN\n' > malicious_repo_worktree/claude && chmod +x malicious_repo_worktree/claude
rustc -O -o repro repro.rs
./repro malicious_repo_worktree ".:$PWD/trusted_bin"   # PATH has a leading relative entry
./repro malicious_repo_worktree "$PWD/trusted_bin:."   # same files, PATH ordered the other way
./repro malicious_repo_worktree "$PWD/trusted_bin"     # no relative entry at all
```
Observed, in order: `MALICIOUS_REPO_BINARY_RAN`, `TRUSTED_BINARY_RAN`,
`TRUSTED_BINARY_RAN`. The only variable between the three runs is the
content and ordering of `PATH`; `cwd` (the stand-in for a task/PR worktree)
is identical and contains the attacker's file throughout. This precisely
isolates the mechanism: a relative `PATH` entry positioned ahead of the
real binary's directory, combined with a repository-controlled cwd, is
sufficient and the whole of what is needed.

**Executable-bit prerequisite, separately verified:**
```sh
d=$(mktemp -d); cd "$d"; git init -q -b main repo; cd repo
printf '#!/bin/sh\necho MALICIOUS_REPO_SUPPLIED\n' > claude; chmod +x claude
git add claude && git -c user.email=t@t -c user.name=t commit -q -m x
cd "$d" && git clone -q "$d/repo" clone3
test -x "$d/clone3/claude" && echo "confirmed: +x survives an ordinary clone"
```
Result: confirmed.

**What this is not:** this is not the real `claude`/`codex`/`gemini`/etc.
binary behaving badly — it never runs. It is not a question of whether the
*agent* is trustworthy. It is Ariadne's own launch code running a
different, repository-supplied program under the name and in the role it
believes belongs to the agent it discovered, probed, and validated minutes
or days earlier. A sufficiently capable repository-supplied stand-in could
even proxy to the real agent afterward (having its own absolute-path
knowledge of where the real one lives) to avoid a visibly broken ACP
handshake, making the substitution effectively undetectable from the
daemon's side.

**Severity:** High. For a *task/column-agent* session the prerequisite is
"the daemon's own environment has a stray relative `PATH` entry" plus "the
registered repository's base branch carries a same-named executable file" —
both under the control of whoever supplied the repository, which is
exactly this audit's threat model. For a *pull-request review* session,
the repository-content half of the prerequisite is satisfiable by **the
author of whichever specific request the user has asked Ariadne to
review** — the PR author does not need any access to the daemon's host,
account, or configuration, but, corrected this revision, they also do not
get to pick which of their PRs gets reviewed or when; the user's own
explicit "review this one" action is the thing that exposes that PR's
content to the worktree in the first place (see the corrected reachability
statement above). The remaining prerequisite (the daemon's own `PATH`) is
outside repository/forge control and is not guaranteed to hold on any given
deployment — a daemon started by `systemd`/`launchd` with an explicit,
fully-absolute `PATH` in its unit file is not exposed; a daemon started
interactively from a developer's own
shell, where accidental relative `PATH` entries are a known, if
discouraged, real-world occurrence, is.

**Confidence:** High. The mechanism is proven with a direct reproduction of
the exact API call Ariadne's own code makes, with no abstraction gap
between the repro and the real code path beyond the two fields (`program`
vs `command`) already cited by file and line.

**What I could not fully confirm:** I did not run this against a live
`ariadned` daemon and a real ACP agent end-to-end (that would require
either deliberately misconfiguring a disposable daemon's `PATH` or
reproducing far more of the launch pipeline than the audit's time budget
allowed) — the reproduction isolates the one primitive that matters
(`Command::new(bare_name).current_dir(repo_content)` under the daemon's own
process), not a full daemon session. I also did not determine how common a
stray relative `PATH` entry is across real Ariadne deployments; that would
need telemetry or a user survey this audit cannot gather.

**Recommended remediation (not applied — read-only audit):** for a
`RegistryEntry` whose `source == AcpAgentSource::Registry` (a discovered
agent), spawn the already-resolved, already-containment-checked
`entry.program` as the program, with `entry.command[1..]` (its own
declared args, e.g. `acp` for `goose acp`) plus the agent's configured
flags as arguments — not `entry.command[0]`. A `command_of`-equivalent
accessor should return `(PathBuf, Vec<String>)` (program, args) rather
than a flat `Vec<String>` the launcher has to split back apart. For a
`source == AcpAgentSource::Config` entry (the user's own `[[acp_agents]]`),
the user has explicitly typed the command and accepted responsibility for
their own `PATH`; resolving it once at config-load time to an absolute
path (for consistency, and because it costs nothing) is a reasonable
secondary improvement but is lower priority than fixing the discovered-
agent path, where Ariadne itself already did the work of finding the
*right* file and then threw that answer away.

**Regression criterion:** an integration test using the repo's own ACP
stub-agent harness (`crates/ariadne-daemon/tests/it/common/acp.rs`, per
`crates/AGENTS.md`) that (a) registers the stub under a bare name findable
on a `PATH` containing a leading relative entry, (b) places a
distinguishable, differently-behaving executable of the same basename in
the session's `cwd`, and (c) asserts the session's `agentInfo` (from its
real `initialize` response, which only the genuine stub answers correctly)
matches the stub's, not the planted file's. This needs the launcher to use
the absolute path regardless of what the surrounding `PATH`/cwd contain, so
it would have failed before the fix described above and passes after it.

---

#### EXEC-001 — Hostile repository branch names reach several `git` invocations as unguarded, option-parseable arguments (CWE-88 argument injection); corrected impact and remediation

**Affected files/lines:** unchanged from the prior revision — see the table
below for the corrected, per-sink result.

**Attacker-controlled input:** unchanged — the current branch name of a
registered checkout (`resolve_base_branch`,
`crates/ariadne-daemon/src/http/repositories.rs:193-210`, which checks only
that the branch *exists*, never that its *name* is a safe bare CLI
argument), and, for the narrower forge-sourced paths,
`pull.head_branch`/`pull.base_branch`.

**Correction history:** an earlier revision claimed every reachable sink
"fails safely." That is false for `add_worktree` (`--orphan`, and `--force`
even with `--end-of-options`). Earlier revisions also proposed a universal
`--` and then a universal `--end-of-options`; both break or fail to
protect at least one sink. The table and remediation below are per sink,
with the exact command and result for every payload tried.

**Per-sink results, retested completely:**

| Sink (`gitwt.rs`) | Shape | Crafted payload tried | Result | Impact class |
|---|---|---|---|---|
| `add_worktree` (`:70-89`), `base` at line 82 | `worktree add -b <branch> <wt> <base>` | `--output=<path>` | `error: opción … desconocida`, exit 128 | fails safely |
| same | same | **`--orphan`** (with `--end-of-options`, against a real `--orphan` ref) | fails, exit 255, from an *internal* `git branch`-level reparse (not the outer marker) — see the corrected text below the table | fails safely, for this one payload |
| same | same | **`--force`** (with `--end-of-options`, against a real `--force` ref pointing at an earlier commit, `HEAD` since moved on) | **succeeds, exit 0** — creates `<branch>` from the repository's *current* `HEAD`, not the real `--force` ref's actual target | **silently wrong base**, not merely unborn: `--end-of-options` does **not** close this sink — see the corrected remediation below |
| `delete_branch` (`:341-344`) | `branch -D <branch>` | `--output=<path>`, `-fother` (attempt to force-delete an unrelated branch `other` via bundled short flags) | both: `error: switch desconocido`/`opción … desconocida`, exit 129; `other` was **not** deleted | fails safely |
| `remote_has_branch_tip`/`remote_branch_tip` (`:350-384`), first call `rev-parse <branch>` | standalone, first arg | `--output=<path>` | `rev-parse` does not recognize it as an option; echoes the literal string to stdout, exit 0, **no file written** | fails safely (harmless no-match) |
| same, second call `ls-remote --heads <remote> <branch>` | second positional, *after* `remote` | `--upload-pack=<marker command>` | confirmed via `GIT_TRACE=1`: git still invokes its normal `git-upload-pack <path>`; the marker command never runs | fails safely |
| `committed_clean` (`:26-40`), range `{base}..{branch}` as one token | one token | `--output=<path>` | rev-list has no such option; prints its own usage, exit 129 | fails safely |
| `diff` (`:414-420`), preliminary `merge-base <base> <branch>` then `diff {base}...{branch}` | `base` standalone, then one combined token | `--output=<path>` | `merge-base` rejects it (exit 129) → `diff()` takes its `EMPTY_TREE` fallback; the vulnerable `diff` line is **not reached** | fails safely for this payload |
| same | same | **`--fork-point`** | `merge-base --fork-point <branch>` **succeeds** (exit 0) — the guard does *not* universally hold | guard bypassed, but... |
| same | same | `--fork-point` carried through to the actual `diff {base}...{branch}` call | `git diff` has no `--fork-point` option; `opción inválida`, exit 129 | ...still fails safely at the final call, for every payload tried |
| same | same | **`--independent`** | also bypasses the `merge-base` guard (exit 0); also not a valid `git diff` option, so the final call still fails safely | guard bypassed, final result still safe |

**Revised conclusion:** the `merge-base` pre-check in `diff()` is **not** a
reliable guard — I found two payloads (`--fork-point`, `--independent`)
that pass it — but I could not find any payload that is *both* accepted by
`merge-base`'s own small, enumerated flag set (`-a/--all`, `--octopus`,
`--independent`, `--is-ancestor`, `--fork-point`) *and* a recognized,
state-changing `git diff` option, since the two commands' flag vocabularies
don't overlap. `add_worktree`'s `base` slot is the one sink with a
confirmed non-error impact: **a hostile repository whose branch name is
literally `--orphan` silently turns every new task's worktree creation into
an orphan branch with none of the registered repository's history**,
rather than failing. I did not exhaustively enumerate every option of
every sink's git subcommand (`rev-list` alone lists ~25); I tested each
sink's full option *reference* (shown in its own `--help`/usage text) for
anything that writes, reads elsewhere, or changes semantics, and found
exactly one such case. This is recorded as a residual gap, not asserted as
complete coverage.

**Severity:** Medium-High, revised again this revision. The `--orphan`
finding alone was "DoS-or-silent-worktree-corruption" (Medium — an obvious,
history-less worktree, likely noticed quickly). The `--force` finding is
worse in kind, not just degree: the worktree is created successfully, with
real, ordinary-looking history, silently based on the repository's current
`HEAD` instead of the branch the task was supposed to be cut from — a task
could silently work against the wrong base with no error, no empty
worktree, and nothing that looks obviously wrong at the point of creation.
Still well short of file write or code execution outside the git object
model itself, which keeps this below High rather than at it.

**Confidence:** High on every result in the table (each independently
executed, not inferred, including the `--force` bypass with two distinct
commits and a real ref pointing at the earlier one); Medium on "no sink has
a worse accepted option than what I found" for the sinks *other than*
`add_worktree` (not exhaustively enumerated there either, see above); the
`add_worktree` sink itself is now fixed with a resolve-to-object-id
approach that does not depend on enumerating git's option surface at all,
which is deliberately a stronger claim than "no worse option exists."

**Corrected remediation (the prior revision's was unsafe — see why,
below):**

The prior revision recommended inserting a bare `--` before every such
argument. This is wrong: `--` specifically tells `git diff`/`git log`/`git
checkout`-family commands "everything after this is a *pathspec*, not a
revision", which is a different meaning than "stop parsing options" and
silently breaks the very call it was meant to protect:
```sh
git diff -- 'HEAD^...task'        # exit 0, EMPTY output — WRONG, should show the real diff
git checkout --force --detach -- 'HEAD^'
# fatal: git checkout: --detach no toma un argumento de ruta 'HEAD^'   — breaks outright
```
`--end-of-options` (a distinct, newer git marker) does not carry the
pathspec meaning and was verified, per sink, to preserve correct behavior
on a legitimate value *and* still reject a crafted one:
```sh
git diff --end-of-options 'HEAD^...task'              # correct diff, exit 0
git checkout --force --detach --end-of-options 'HEAD^' # correct detach, exit 0
git diff --end-of-options '--output=/tmp/x...task'     # fatal: option must come before non-optional arguments — rejected, no file written
git worktree add -b taskA wt --end-of-options base2    # correct worktree, real base content present
git branch -D --end-of-options base2                   # correctly deletes; a crafted value after the marker is "not found", not misparsed
git rev-list --count --end-of-options main..taskC       # correct count (1), preserved
```
`ls-remote --heads origin --end-of-options main` returning the correct ref
line is **not, by itself, evidence that `--end-of-options` stopped
anything** — revision 2 already established that `ls-remote` treats
whatever follows the first non-option positional (the remote) as a literal
pattern regardless, so a successful result here does not distinguish "the
marker worked" from "nothing needed to work." Kept as harmless, not kept
as proof.

**The `--orphan` payload needed a corrected, more exact reproduction.** My
first test (used above and in the prior revision) passed `--orphan` as a
`base` value with no branch of that name actually existing, and the fixed
call failed with `fatal: referencia inválida: --orphan` (`rev-parse`-style
"invalid reference"). Retested with an *actually-existing* branch
literally named `--orphan` (created via `update-ref`, exactly as EXEC-001's
own opening reproduction shows is achievable through an ordinary clone),
the result is different, and this is the setup/result that should be
trusted:
```sh
d=$(mktemp -d); cd "$d"
git init -q -b main repo && cd repo
echo hello > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -q -m init
git update-ref 'refs/heads/--orphan' HEAD      # a real branch literally named --orphan
# the vulnerable (unfixed) call still corrupts the worktree regardless of whether --orphan is a real ref:
git worktree add -b taskV "$d/wtV" --orphan   # exit 0, unborn branch, same as before
# the FIXED call, now tested against a real --orphan ref rather than a nonexistent one:
git worktree add -b taskF "$d/wtF" --end-of-options --orphan
```
Result: `error: opción `orphan' desconocida` with a **`git branch`
usage message** (not `git worktree add`'s), exit 255 — not the
`rev-parse`-style "invalid reference" text my first test reported. For
*this specific payload*, the fixed call still refuses to create the
worktree (confirmed: `$d/wtF` does not exist afterward): `git worktree add
-b <branch>` creates the new branch through an internal branch-creation
step that re-parses the commit-ish argument on its own, independent of the
outer command's `--end-of-options`, and that inner step is what actually
rejects `--orphan` here, not the outer marker.

**That inner step is also where the fix breaks down for a different
payload, and the "security conclusion unchanged" statement above does
not generalize — withdrawn.** The review found this directly: `--orphan`
happens to be a flag the inner branch-creation step does *not* recognize,
so it errors; `--force` is a flag that step *does* recognize (`git
branch` itself has a `-f`/`--force`), so instead of erroring, the inner
step silently consumes `--force` as a flag to itself, is left with no
commit-ish of its own, and falls back to creating the branch from
**whatever the repository's current `HEAD` happens to be** — which is
not the resolved target of the real `--force` ref, and not necessarily
the registered `repo.base_branch` the task was supposed to be cut from
either. Reproduced with two distinct commits and a real `--force` ref
pointing at the earlier one:
```sh
d=$(mktemp -d); cd "$d"; git init -q -b main repo; cd repo
echo "commit A content" > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -q -m "commit A"
sha_a=$(git rev-parse HEAD)
git update-ref 'refs/heads/--force' HEAD        # real branch, named --force, pointing at commit A
echo "commit B content" > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -q -m "commit B"
sha_b=$(git rev-parse HEAD)                     # current HEAD has since moved to commit B
git worktree add -b protected "$d/wt" --end-of-options --force
git -C "$d/wt" rev-parse HEAD                   # compare against $sha_a and $sha_b
```
Result: exit 0, `$d/wt/file.txt` contains "commit B content," and
`git -C "$d/wt" rev-parse HEAD` equals `$sha_b` — **not** `$sha_a`, the
real `--force` ref's actual target. The fixed call succeeds silently with
the *wrong base*, which is worse than the `--orphan` case (a visible,
history-less worktree) because nothing here looks obviously broken: the
worktree has real history, just not the history the registered repository
and task branch were supposed to be cut from. **`--end-of-options` is
therefore not a safe, uniform fix for `add_worktree`'s `base` argument —
withdrawn as the recommendation for that one sink; see the corrected
remediation below, which replaces it with a resolve-to-an-object-id
approach that does not depend on any subcommand's internal reparsing
being safe.** `delete_branch`'s `--end-of-options` fix is unaffected by
this (retested with a real `--force` branch present: `git branch -D
--end-of-options --force` correctly deletes only that branch, because
`branch -D` does not have the nested branch-creation step `worktree add
-b` does); the `diff`/`committed_clean`/`rev-parse`/`ls-remote` fixes
are unaffected for the same reason — only `add_worktree`'s internal
branch-creation call has this specific failure mode, and the correction
below is scoped to it.

`has_commit` (`gitwt.rs:250-260`) already uses `--end-of-options` and
remains the correct, verified pattern for the sinks in the table above
*other than* `add_worktree`'s `base` argument, which needs the different
treatment just described.

`remote_has_branch_tip`/`remote_branch_tip`'s *first* call
(`rev-parse <branch>`, line 356/376) is **not** safe to fix with a bare
`--end-of-options` — doing so would have broken the function for every
legitimate branch, not just a malicious one, because plain `rev-parse`
(unlike `rev-parse --verify`) echoes back any argument it does not resolve
as a flag, including the marker itself:
```sh
real=$(git rev-parse feature)                       # 8358974af109c51061e366c4c7983ed9254a39d7
bad=$(git rev-parse --end-of-options feature)
echo "$bad"
# --end-of-options
# 8358974af109c51061e366c4c7983ed9254a39d7            <- the real SHA is only the SECOND line
```
The function's own SHA comparison (`let local = self.git(repo, &["rev-
parse", branch]).await?;` then `sha == local`) would receive the two-line
string above instead of a bare SHA and never match a real `ls-remote`
result again — a `--end-of-options`-only fix applied uniformly, as the
prior revision implied, would have turned a latent injection bug into an
always-broken safety check. The correct fix for this one call matches
`has_commit`'s *full* pattern, not just its marker:
```sh
git rev-parse --verify --quiet --end-of-options feature
# 8358974af109c51061e366c4c7983ed9254a39d7            <- single line, matches `real` exactly
git rev-parse --verify --quiet --end-of-options "--output=/tmp/poc_rp.txt"
# fatal: ... ; exit 1, no file written
```
confirmed to reproduce the plain SHA exactly (`$real == $out`) on a
legitimate branch and to still reject the crafted payload. The *second*
call in the same two functions (`ls-remote --heads <remote> <branch>`) was
already shown safe without any fix in revision 2 (args after the first
non-option positional are literal patterns here); adding
`--end-of-options` there is harmless but not required.

**Corrected remediation for `add_worktree`'s `base` argument specifically
(the sink the `--force` bypass affects):** do not pass the raw branch-name
string to `git worktree add` at all, under any marker. Resolve it to a
verified, fully-qualified object id *first*, exactly the way the fix for
`remote_has_branch_tip` already does, then pass only that resolved id
downstream:
```sh
resolved=$(git rev-parse --verify --quiet --end-of-options "refs/heads/$base")
# resolved is empty + non-zero exit if $base does not name a real branch —
# treat that the same as today's existing branch_exists() check already does
git worktree add -b "$branch" "$wt" "$resolved"
```
Verified this closes the `--force` bypass precisely: resolving
`refs/heads/--force` through `rev-parse --verify --quiet --end-of-options`
yields `$sha_a` (the real target), and `git worktree add -b protected "$wt"
"$sha_a"` then correctly creates the worktree at commit A's content — not
B's, and not an `--orphan`-style unborn branch either, since a 40-hex
object id can never be mistaken for a flag by any internal git parser,
nested or not. This is a strictly stronger fix than a parser marker:
it does not depend on knowing, for every current and future git version,
which flags every internal step downstream of `worktree add -b` does or
does not recognize.

**Regression criterion, revised:** for each guarded sink **other than**
`add_worktree`'s `base` argument, (a) a same-shaped, non-malicious
branch/ref name of equal or greater length continues to produce the
identical observable result as before the fix (the specific commands and
expected outputs above) — for `remote_has_branch_tip`'s `rev-parse` call
specifically, the fixed call's output must be byte-identical to plain
`git rev-parse <branch>`'s output on the same branch, not merely
"non-empty" — and (b) a repository whose relevant branch/ref is named
`--orphan`/`--output=<path>`/`--upload-pack=<cmd>`/`-fother` (one test per
sink) is observed to fail with a clear git error rather than succeeding
with different semantics or writing to any path outside the intended
target. For `add_worktree`'s `base` specifically: (c) resolving a real
branch named `--force` or `--orphan` through the recommended
`rev-parse --verify --quiet --end-of-options refs/heads/<name>` step
yields that branch's *actual* target object id, verified against an
independently-captured `git rev-parse <name>` taken before any other
commit moves `HEAD`, and (d) the resulting worktree's `HEAD`, content, and
first-parent history match that resolved id — not the repository's
current `HEAD` at call time, and not an unborn/orphan state — for both a
`--force`-named and an `--orphan`-named real branch, each tested with the
registered repository's actual `HEAD` deliberately pointed somewhere else
at call time, exactly as the regression reproduction above does.

---


#### EXEC-006 — "Read-only by contract" and Ariadne's permission modes are cooperative controls: nothing in the git/worktree layer enforces them at the filesystem level

**Affected files/lines:** `gitwt.rs:94-105` (`add_detached_worktree`) and
`gitwt.rs:135-140` (`checkout_detached`) — both plain `git worktree
add`/`checkout` calls; neither sets any filesystem permission, mount
option, or git-level restriction. `launcher.rs:285-347` (`launch`) is
where every session's `permission_mode` (`auto`/`ask`/`learn`/`ai`) is
resolved from the repository row and threaded into `AcpLaunch`, for every
seat alike (task agent, orchestrator, reviewer, keeper). `acp.rs:965-978`
is the actual spawn (shared with EXEC-005). `acp.rs:2052-2270` is the
`session/request_permission` handler that `auto`/`ask`/`learn`/`ai`
actually govern.

**What this finding is, precisely:** `checkout_detached`'s own doc comment
(`gitwt.rs`) calls the review worktree's tree "read-only by contract" —
language that describes an *intended convention* (the review session is
briefed not to commit) rather than a technical restriction. The task asks
to check documented permission guarantees even when installed agents are
trusted, so this is reported as what it actually is: a convention, not an
enforced boundary.

**What *is* real:** the `auto`/`ask`/`learn`/`ai` permission modes
(`acp.rs:2052-2270`) are a genuine, working control — they decide how the
daemon answers a `session/request_permission` call, with real effects
(waiting for console approval under `ask`, consulting a learned key,
consulting the AI permission model) and are threaded into every launch
uniformly via `launcher.rs:285-347`. This is not a hollow control.

**The repository's *default* mode, specifically, provides less friction
than `ask`/`learn`/`ai` — corrected this revision to state precisely what
it does, not "unconditionally yes."** `auto` (the default for every
repository until the user changes it, per spec 002) resolves every
`session/request_permission` call through `approved_option`
(`acp.rs:3183-3199`), whose actual behavior is narrower than "answer every
request yes":
```rust
fn approved_option(params: &Value) -> Option<String> {
    let options = params.get("options").and_then(Value::as_array)?;
    options
        .iter()
        .find(|option| option.get("kind").and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("allow")))
        .and_then(option_id)
        .or_else(|| options.first().and_then(option_id))   // falls back to the FIRST option, not necessarily an "allow" one
}
```
and the dispatch at `acp.rs:2159-2161` routes to it on exactly this path:
`None => (approved_option(params), "auto", None)`. **Precisely stated:**
`auto` picks the option the agent itself labeled with a `kind` starting
"allow", and *if the agent did not label any option that way*, falls back
to blindly taking whichever option is *first* in the list the agent
provided — trusting the agent's own ordering, not independently verifying
the first option is actually a safe one to pick. If the agent's `options`
array is empty, `approved_option` returns `None`, and the dispatch's
`selected` is `None`; downstream (`acp.rs:2238-2240`), that maps to
`{"outcome": "cancelled"}`, **not** to silent approval — confirmed by
reading the exact mapping (`selected.as_ref().map_or_else(|| json!({"outcome":
"cancelled"}), |option_id| json!({"outcome": "selected", "optionId": option_id}))`).
So the accurate claim is: **`auto` answers a permission request using
whatever shape of options the agent itself offers, preferring one the
agent labeled "allow" and otherwise trusting the agent's own list order,
and cancels outright if the agent offers nothing to choose from** — not
"every request is unconditionally approved." Combined with the point
below — that anything the agent does *without* submitting a request is
never seen by Ariadne at all — the practical effect is still that a
well-behaved ACP agent's own conventional "offer an allow option first"
pattern sails through with no additional friction under `auto`, and
anything the agent does through its own native tools (never asking at
all) is outside this mechanism regardless of mode. A user who wants the
friction `ask`/`learn`/`ai` add has to choose one of those explicitly.

**The entire "commit nothing, push nothing" guarantee for a review session
is one sentence of prompt text, confirmed by reading it:**
`ariadne-store/src/defaults.rs:383` (`PULL_REQUEST_REVIEW_SYSTEM_PROMPT`):
> "Work only in your worktree, detached at the head of the request. Commit
> nothing and push nothing."
This is the entirety of the enforcement mechanism for that guarantee —
natural-language instruction text, interpreted by the agent, with nothing
in `gitwt.rs`, `acp.rs`, or the permission-mode system backing it up
technically (confirmed by the reproduction below, which contains neither
this prompt nor an agent, and still succeeds at everything the prompt asks
an agent not to do).

**What is also real, and is the gap:** this control only fires for actions
the **agent's own ACP client chooses to submit** as a permission request.
The daemon's ACP `initialize` advertises no `fs`/`terminal` capability and
implements no handler for either (EXEC-POS-4, re-confirmed this revision)
— so there is no Ariadne-mediated filesystem or terminal operation to
gate in the first place. Everything the agent does to files or a shell
happens through the agent's **own native tools**, outside any ACP method
Ariadne implements. `session/request_permission` is the agent *telling*
Ariadne "I am about to do X, may I?" before using its own tools to do X —
a cooperative protocol, not a sandbox. Nothing observed in `gitwt.rs` or
`acp.rs` stops an agent process from writing, committing, or pushing in a
"read-only" review worktree without ever calling
`session/request_permission` at all.

**Reproduction** (disposable repository and bare remote only; no write
reached any path outside the disposable sandbox; the registered
repository's own primary branch was read, never altered):
```sh
# (abbreviated; full transcript retained at the evidence the orchestrator
# relayed, /tmp/exec-reviewer-capabilities.log, reproduced independently
# below against the exact primitive gitwt.rs uses)
git worktree add --detach <review-worktree> <head-sha>      # as add_detached_worktree does
cd <review-worktree>
printf "modified\n" > tracked && printf "outside\n" > ../outside-marker  # write inside AND outside the worktree
git add tracked && git commit -qm "synthetic"
git push                                    # fails: detached HEAD has no upstream (ordinary git, not a control)
git push origin HEAD:refs/heads/capability-proof   # succeeds against a disposable bare remote
git rev-parse main                           # unchanged — the registered repository's own primary ref is untouched
```
Result (independently reconfirmed): the tracked edit, the write to
`../outside-marker` (outside the worktree entirely — the worktree
boundary is not a filesystem boundary), the commit, and the explicit-
refspec push all succeed; only the bare `git push` (which needs an
upstream for a detached HEAD, an ordinary git requirement, not a security
control) fails. The repository's own `main` is never moved by any of this.

**What this does not prove, stated as plainly as the evidence allows:** no
real agent was shown to do any of this from a malicious PR comment or any
other injected instruction. Corrected this revision: the agreed scope does
**not** forbid testing this — it permits isolated daemon instances and
controlled service stand-ins, and only excludes *requiring containment of
a deliberately malicious installed agent process*. The honest statement is
narrower than "out of scope": I did not build a live daemon plus a
stand-in agent to drive this end to end within this pass's time, not that
doing so would have been disallowed. No comment-driven commit or push is
demonstrated here. A real, damaging push additionally needs real
credentials and authorization the daemon's git identity would have to
already hold — this reproduction used a disposable local bare remote
precisely because no real remote/credential was exercised. Live,
agent-specific restrictions (e.g. a given adapter's own sandbox mode,
approval settings, or read-only flag) were not measured here and may
independently narrow what a specific installed agent would actually do in
practice — "external agent configuration can still restrict execution,"
and this finding does not contradict that; it only establishes that
**Ariadne's own code** supplies no such restriction independent of the
agent's configuration and cooperation.

**Severity:** Medium. The gap is real and the reproduction is concrete;
what is not established is whether a real, specific installed agent would
actually act on untrusted content this way — that is a live-daemon
reproduction this pass did not build, not a question the agreed scope
rules out asking. It is a question that also sits partly with PERM's
permission-decision ownership and the agent's own prompt-injection
resistance, not EXEC's git/worktree layer alone.

**Confidence:** High on the mechanism (independently reproduced). Low on
severity *beyond* "the layer provides no independent technical
restriction," since what would raise it — a real agent, under realistic
conditions, actually doing this — was not built in this pass.

**Recommendation:** document, in the user-facing manual (`docs/`), that
"read-only" for a review worktree is Ariadne's own convention plus the
permission-mode system's coverage of whatever an agent chooses to submit
through it — not a filesystem-level guarantee — so a user's trust decision
about which repositories to let Ariadne review is made with accurate
information. No code change is proposed here: this is a documentation gap
about an existing, deliberate design (ACP's `fs`/`terminal` capabilities
are deliberately not implemented, matching EXEC-POS-4), not a defect to
patch. *Acceptance criterion:* the manual page describing review sessions
states explicitly that (a) "read-only" is a prompt-level instruction the
reviewer agent is briefed with, not a technical restriction on its
process, (b) the `auto`/`ask`/`learn`/`ai` permission modes govern only
actions an agent's ACP client submits as a `session/request_permission`
call, and (c) `auto` specifically picks an agent-labeled "allow" option or
else the first option offered, rather than withholding approval by
default — verifiable by an independent reader comparing the documented
text against this finding's source citations without needing to run
anything.

**Compatibility decision for the user:** whether Ariadne should ever
implement `fs`/`terminal` ACP methods specifically to *gain* enforcement
ability over what a review session does (trading "the agent uses its own
tools freely" for "the daemon mediates and can refuse") is a product
direction question, routed to the orchestrator, not decided here.

---

### Additional execution-boundary coverage (cleanup, symlinks, identity, switches, cancellation)

Five specific areas the task's Work items name, with concrete source
evidence for each rather than left to the inventory table's general
claims:

**Cleanup destinations, corrected to cover every removal site, not only
the task-level one.** `Launcher::cleanup_task` (`launcher.rs:1317-1403`)
removes **two** kinds of path, not one: it iterates every session of the
task and removes `session.worktree_path` where that session is not
user-resumed (`launcher.rs:1361-1378`), *and* separately removes the
task's own `task.worktree_path` (`launcher.rs:1379-1385`). Both are
internally-generated worktree paths
(`cfg.worktree_root.join(tail(goal_id)).join(...)`; `tail()`
(`launcher.rs:2106-2107`) just takes the last 8 characters of a ULID
verbatim — safe not because `tail()` filters anything itself, but because
`goal_id`/`task.id` are minted internally as ULIDs, whose own character set
(Crockford base32) excludes any path-traversal-meaningful character), never
a path derived from repository content. Branch deletion is similarly
gated: it only runs when `task.status() == TaskStatus::Finished`,
`task.branch != repo.base_branch` (never deletes the branch the repository
itself is on), and `self.git.branch_exists(...)` confirms it first.
`prune_worktrees` is then called unconditionally, which only prunes git's
own bookkeeping for worktrees already gone from disk — no new destination
is introduced.

A **separate** removal path exists for pull-request work:
`end_seated_pull_request_work` (`launcher.rs:1955-2029`) collects *every*
live session's `worktree_path` for that request **plus** a generated
fallback (`keeper_worktree_path`/`pull_request_worktree_path`, built the
same internally-derived way as the task case), and applies its own,
distinct guards before removing each: a keeper's worktree is kept on disk
(not removed) if it is not `git`-clean or its branch is not judged safe to
discard (`keeper_branch_is_safe_to_discard`), logged as "holds work the
request's head lacks." This is a different function with different
guards from `cleanup_task`, not a variant of it — both are destinations,
both are internally-generated paths, and both were traced for this
revision rather than only the first one.

Cleanup is documented as idempotent and the code reads that way for both
paths: every step either checks the thing it is about to touch still
needs touching, or tolerates it already being gone (`.ok()` on the removal
calls, or the explicit "leave it on disk" branch above).

**Symlink containment — scope narrowed, corrected.** EXEC-POS-6 establishes
that the *skills-folder* symlink specifically is safe: its target is
always the daemon's own configured path, never repository content, and its
teardown checks `is_symlink()` before removing rather than following it.
**This does not, by itself, establish that worktree roots, their parent
directories, or other session paths are contained against symlink
tricks** — that is a different, broader claim this pass did not check. No
evidence of a problem was found in the worktree-removal paths above either
(both operate on paths the daemon itself generated, never on a
caller-supplied or repository-supplied path), but "not found" here means
"not specifically tested for symlink substitution," which is recorded as
an explicit gap rather than implied coverage.

**Repository identity changes between validation and execution:**
already covered as EXEC-003 (the one place a session's `cwd` can resolve
somewhere other than its own task's worktree) and the `Path::starts_with`
check at `launcher.rs:300-306` (confirmed component-wise, not
string-prefix, ruling out a sibling-directory confusion). No additional
identity-switch path was found beyond those two in this pass.

**Switches and retries, scope corrected.** `switch_session`
(`launcher.rs:987-1170`) has **two** branches, and the prior revision's
"kills and waits before starting anything new" claim described only one
of them. When the switch stays on the **same underlying agent program**
(`agent_of(old.model) == agent_of(model)`, e.g. changing only the model
pin within the same CLI) — `launcher.rs:1009-1050` — the function calls
`self.acp.switch_pin(...)` (an in-process instruction to the agent that is
*already running*, not a kill) and returns without ever calling
`kill_and_wait`; no process is killed on this path because none needs to
be — the same process continues under a new pin. Only on the **other**
branch — switching to a genuinely different agent program
(`launcher.rs:1056` onward) — does the function call
`self.acp.kill_and_wait(&old.id).await` before checking
`assert_no_live_session` and starting anything new, confirming the old
process is gone before the double-staffing guard runs. The
"kill-and-wait-before-starting" guarantee therefore applies to the
cross-agent switch only, not universally. `assert_no_live_session`
(`launcher.rs:66-95`) itself checks the store for a live session *and*
calls `self.session_process_alive(&s)` (which asks the ACP runtime whether
it is actually still running, not just what the database row says) before
refusing a second spawn — a crashed-but-not-yet-marked-exited row cannot
block a legitimate retry. A `switch_session` call is further guarded
against being issued twice against the same old session by
`self.store.switched_successor(&old.id)` returning an error if one already
exists, with the prior revision's note that two racing switches are caught
by a unique store index rather than this check alone.

**Cancellation, corrected — the actual guarantee is a launch gate, not
"bookkeeping cannot precede exit."** `kill_session`
(`launcher.rs:1269-1283`) calls `self.acp.kill_gated(&session.id)`, which
immediately marks the row `Exited` (while the session `.status().is_live()`
holds), **before** the process has necessarily finished exiting —
`kill_session` itself returns as soon as the background task is spawned,
not after `reaped` resolves. The prior revision's claim that "the session's
own bookkeeping cannot race ahead of the process actually exiting" is
**wrong as stated and withdrawn**: the status row *does* change to
`Exited` ahead of the actual reap. What `kill_gated` (`acp.rs:1099-1119`)
actually provides is a `LaunchGate`: a token recorded against the session
id that a **subsequent launch of that same session** waits on before
starting (per `kill_session`'s own doc comment: "the session's next
launch waits until the fact is written, so a resume neither takes the
killed run's fact away nor adds its own turns and tokens to it") — the
real, verified guarantee is "a new process cannot start on this session
until the old one's reap and stats-recording have completed," not "the
status field cannot update early." I did not find or exercise a case where
this distinction (early status update, gated relaunch) itself lets
unauthorized work survive a cancellation — recorded as a corrected
description of the mechanism, not a reported exploit.

This is a **separate** mechanism from the `GitManager::git`/direct-`Command::new`
`git` subprocesses discussed in the corrected subprocess inventory above,
which have no equivalent `kill_on_drop`/timeout of their own — cancellation
safety for the *agent* process and for a bare `git` call are not the same
guarantee, and this report does not claim the second inherits the first's
rigor.

---

### Unverified candidates

**Moved here this revision.** EXEC-002, EXEC-003, and EXEC-004 were sitting
under "Confirmed" despite each one's own text already saying its central
claim was unverified — a categorization error, not a content error; their
text is otherwise unchanged except where noted below.

#### EXEC-002 — `git fetch`'s remote argument is taken from forge data with no scheme/shape gate at the point of use

**Affected files/lines:** `launcher.rs:1456-1463` (`review_worktree`,
no `Remote::parse` call at all before using `pull.head_repo` as `from`);
`launcher.rs:1546-1574` (`keeper_remote`, calls `forge::Remote::parse`
but returns the raw, unparsed string on **both** failure paths —
`repo.forge.is_none()` at line 1554-1555, and `Remote::parse` returning
`None` at line 1557-1558 — rather than refusing it).

**Scope:** EXEC owns `gitwt.rs::fetch_head_of` (the sink) applying no
restriction of its own. FORGE owns whether `pull.head_repo` can ever be a
value outside `https|http|ssh|git|git+ssh` in the first place.

**Exclusions:** no live SSH/`git://` remote was available to test against;
this finding does not claim RCE, only an unguarded sink receiving a value
with an allow-list computed nearby that it does not actually enforce.

**Rejected hypothesis, explicitly retested this revision:** `ext::` as an
RCE vector through this sink. `Repro:` `git fetch 'ext::sh -c "touch
<marker>"' <refspec>` against a disposable local repo →
`fatal: transporte 'ext' no permitido`. Rejected for this git's default
configuration; `GitManager::git` sets no `GIT_ALLOW_PROTOCOL` override, so
this rejection comes entirely from git's own built-in default, not from
anything Ariadne does — meaning it is not a control Ariadne can claim
credit for, only one it has not (yet) undermined.

**Named mechanism for what an unvalidated `from` could be used for, added
this revision:** see CVE-2024-52006 in the advisory section above —
credential-helper newline confusion is a concrete, real example of what an
attacker-influenced `from` could target on a host whose git version and
credential helper are both vulnerable; this does not change this finding's
own evidence or severity, it sharpens what "unclear impact" could concretely
mean.

**Severity:** Low-to-Medium, contingent on FORGE's guarantees.

**Recommendation, with acceptance criteria:**
1. Set `GIT_ALLOW_PROTOCOL=file:http:https:ssh:git` on every
   `GitManager::git` invocation whose `from`/`remote` argument can
   originate from forge or repository data. *Acceptance:* a disposable
   repository configured with a `from` value using an explicitly
   disallowed scheme (e.g. `ext::...`) is refused by git with the same
   class of error shown above, verified with this env var set **even if**
   git's own default policy changes in a future version — i.e. the test
   should not rely on git's default alone.
2. Make `keeper_remote` return an error (not `Ok(head_repo.to_string())`)
   on both paths where `Remote::parse` cannot validate `head_repo`.
   *Acceptance:* a `pull.head_repo` value that fails `Remote::parse` causes
   the keeper-session launch to fail with a named error, not to proceed
   with an unvalidated remote.

**Compatibility decision for the user:** whether #2 should also change
`review_worktree` (which currently never calls `Remote::parse` at all) to
refuse instead of proceed is a product decision — refusing could block a
legitimate review session on a fork the parser does not yet recognize.
Routed to the orchestrator rather than decided here.

---

#### EXEC-003 — A resumed column-agent session can fall back to the bare repository checkout instead of its task worktree

**Affected files/lines:** `launcher.rs:898-970` (`resume_in_place`, shared
by both immediate callers — `revive_session` and `relaunch_session` —
confirmed this revision that **both** reach `resume_in_place` directly
with no task-worktree repair step in between, i.e. there is no
intermediate guard that would catch this before the fallback at lines
917-945 is evaluated).

**Scope:** EXEC owns whether a task-seated session's `cwd` can ever resolve
to something other than that task's own worktree.

**Exclusions:** no live daemon reproduction was built (would require
inducing a non-terminal task to lose its worktree directory, then
triggering revive/relaunch on that exact session) — this remains a
code-read-based finding, not a live one.

**Severity:** Medium if reachable, Low likelihood (requires an already-
unusual state).

**Recommendation, with acceptance criteria:** either (a) make
`resume_in_place` call the same recreate-if-missing `task_worktree` logic
`start_step_agent`/`agent_switch` already use for a task-seated session, or
(b) make the bare-repository-path fallback explicit to the orchestrator
seat only, failing loudly for any other seat whose worktree is gone.
*Acceptance (either option):* a session with `seat == Agent`,
`task_id == Some(_)`, and a `worktree_path` pointing at a now-deleted
directory, when revived, either lands in a freshly recreated worktree on
its own task branch (option a) or fails with a named error rather than
silently landing in the shared repository checkout (option b) — never the
latter with a 200-level success result.

**Compatibility decision for the user:** which of (a)/(b) is preferred is a
product decision (recreate transparently vs. fail loud) — routed to the
orchestrator.

---

#### EXEC-004 — Ariadne's own code builds a shell-interpretable command from a forge-sourced branch/remote and hands it to a trusted agent as a literal instruction

*(Substantially reassessed in revision 2 — the original framing incorrectly
dismissed this as requiring a "nonstandard forge" and over-applied the
"trust the agent" exclusion to Ariadne's own command-construction code.
Further corrected this revision: the platform-acceptance claim was
overstated, and the regression criterion asked for something no valid git
branch name can be.)*

**Affected files/lines:** `launcher.rs:1579-1585` (`keeper_push_command`,
builds `format!("git push {} HEAD:{}", remote, pull.head_branch)`) and
`launcher.rs:1876-1897` (`keeper_briefing_of`, embeds that exact string in
the keeper agent's briefing as "the command that pushes a fix to it").
Both `remote` (from `keeper_remote`, which can be a forge-sourced URL per
EXEC-002) and `pull.head_branch` (the forge-reported PR head branch name)
are interpolated with no quoting.

**Mechanism, confirmed by a local reproduction — corrected claim about
what that reproduction establishes:**
```sh
git push origin 'HEAD:refs/heads/topic;echo${IFS}EXEC_MARKER_REPRO'
# pushed OK, exit 0
git ls-remote --heads origin
# 92027054...  refs/heads/topic;echo${IFS}EXEC_MARKER_REPRO
```
This proves **git itself** accepts a branch name containing shell
metacharacters through nothing more than an ordinary local `git push` —
the prior revision's "requires a nonstandard forge" premise is still
correctly disproven, since nothing about this test used a nonstandard
anything. **Corrected this revision:** a local git push does **not**, by
itself, prove GitHub's or GitLab's own server-side ref-name acceptance
rules are equally permissive — those platforms could in principle reject
or rewrite such a branch name at their own layer, and that was not tested
here (no live GitHub/GitLab repository was used, per this audit's
constraint against exercising external services). The accurate claim is
narrower: **git's own ref-name rules do not require a nonstandard forge to
produce a shell-metacharacter branch name**; whether a specific hosted
platform's own additional validation (if any) would also pass such a name
through is untested and not claimed either way.

The generated instruction, if handed to any shell for literal execution,
does exactly what the string says:
```sh
generated_command="git push origin HEAD:topic;echo\${IFS}EXEC_MARKER_REPRO"
sh -c "$generated_command"
#  * [new branch]      HEAD -> topic
# EXEC_MARKER_REPRO
```
Both halves (the push, and the injected `echo`) ran as two separate shell
commands — the push's success or failure is irrelevant to whether the
injected half executes.

**Scope boundary, corrected:** the exclusion that applies here is "we do
not need to stop a malicious installed agent from misusing its own shell
access" — it is **not** "we can ignore a daemon-constructed string merely
because a trusted agent is the one that might run it," and it is also
**not** "the agreed scope forbids testing whether a real agent would do
this" (the scope permits isolated daemon instances and controlled service
stand-ins; it only excludes requiring containment of a deliberately
malicious installed agent — not building a stand-in to observe ordinary
behavior). The agent in this scenario, if it ran the instruction verbatim,
would not be misbehaving — it would be doing exactly what a well-behaved
agent does with an instruction captioned "the command that pushes a fix."
The unsafe artifact is the string Ariadne built; whether a real agent
actually runs it verbatim rather than composing its own command is simply
not tested in this pass (see Severity).

**Remote interpolation, assessed separately:** not separately reproduced
with a crafted `remote` value in this pass (would need a crafted forge
response feeding `keeper_remote`, which is FORGE-adjacent setup), but the
same `format!` call interpolates it with the same lack of quoting; whatever
shape of value can reach `remote` (a forge-sourced URL, per EXEC-002)
carries the identical risk via the identical mechanism. Recorded as
reasoned-but-not-independently-reproduced, distinct from the branch-name
half which is fully reproduced above.

**Severity:** Medium. Confirmed mechanism (branch-name half), ordinary git
usage suffices as the prerequisite for the mechanism, but the final step
(whether an agent actually executes unmodified text it's given, rather
than composing its own command or asking for confirmation) is
agent-behavior-dependent and was not built as a live reproduction in this
pass.

**Recommendation, with acceptance criteria:** this is EXEC's own string-
construction choice, fixable unilaterally regardless of what FORGE
validates upstream (keeping FORGE's metadata-ownership and EXEC's command-
construction concerns separate, per the cross-task contract): either (a)
describe the push in prose ("push your branch to `<remote>`'s copy of
`<head branch>`") and let the agent compose its own command, removing the
ready-made shell string entirely, or (b) if a literal command is kept,
single-quote both `remote` and `pull.head_branch` with embedded-quote
escaping (POSIX shell quoting: wrap in `'...'`, replacing any `'` with
`'\''`) before interpolation. **Acceptance, corrected:** a git branch name
cannot itself contain a space (confirmed: `git update-ref 'refs/heads/has
space' HEAD` is refused — `rehusando actualizar ref con mal nombre`), so
the criterion cannot require pushing a space-containing *branch*. Test two
things separately: (i) for `pull.head_branch`, construct a real,
git-ref-valid name containing `;`, `$()`, or backticks (all permitted by
`check-ref-format`, confirmed above) and verify the generated, quoted
string, run through `sh -c`, executes no side-effect command and still
targets that literal branch name; (ii) for `remote` (not git-ref-format
constrained — it can be an arbitrary string), separately verify that a
value containing a space or shell metacharacter is quoted/rejected rather
than mis-parsed, since this argument is not limited to valid ref syntax the
way a branch name is.

---

#### EXEC-007 — A compromised or MITM'd ACP registry index can register and launch an arbitrary, already-present local program under a trusted agent's identity, with attacker-chosen arguments

**Unproved — this is accurate analysis of a traced mechanism, not a
confirmed exploit. Added this revision after the review correctly rejected
the prior claim that a tampered registry index "cannot influence the
executed file."**

**Affected files/lines:** `acp_index.rs:172-195` (`candidates`) builds the
name(s) `which()` will search for, and the arguments that will later be
passed to whatever is found, straight from the downloaded index document's
fields (`target.cmd`'s basename and `target.args` for a "binary"
distribution; `package.args` for npx/uvx). `acp_index.rs:85-109` (`found`)
applies a package-containment check (`installs()`) **only** when
`candidate.package` is `Some(_)`.

**Corrected this revision — the containment gap is broader than "binary
distributions only," which the prior text got wrong.** Re-reading
`candidates()` precisely: for *each* npx/uvx package, it pushes **two**
candidates, not one — first `Candidate { name: agent.id.clone(), args:
package.args.clone(), package: None }` (named by the agent's own id, e.g.
`"claude"`), *then* `Candidate { name: command_name(&package.package),
args: package.args.clone(), package: Some(package_dirs(&package.package)) }`
(named by the package's own command name). **Only the second of these two
has `package: Some(_)` and therefore gets the `installs()` containment
check at all.** The first npx/uvx candidate — the one named by the
agent's own id — has `package: None`, exactly like the "binary" candidate,
and is checked against `which()` alone. The prior text's claim that
containment is "true for the npx/uvx candidates" was wrong: it is true for
*one specific candidate* of the *two* each npx/uvx package contributes, not
for the npx/uvx path in general. The actual, corrected shape of the gap is:
**every "first-try" candidate across all three distribution kinds — the
binary candidate and the agent-id-named npx/uvx candidate — has zero
containment; only the package-name-named npx/uvx fallback candidate has
any.** `acp_discovery.rs:612-625` (`probe`, see the corrected subprocess
inventory) then spawns whatever was found, with the index-supplied
arguments, **before** any ACP protocol negotiation occurs — the process is
already running by the time any check could tell it isn't a real ACP
agent.

**Mechanism, traced precisely (code-level, not independently executed
end-to-end — see Limitations), and broader than a fixed interpreter
blacklist could close.** If the fetched index claimed, for some pinned
agent id, a "binary" `cmd` (or, per the correction above, simply an
agent-id-named npx/uvx candidate) resolving to some near-universally-
present program, `which()` would find the *real*, locally-present program
— there is nothing anomalous to detect, it genuinely exists on `PATH` —
register it with **zero** containment check, and the daemon would launch
it with whatever `args` the same compromised index specified. **Rejecting
a fixed list of interpreter names (`sh`/`bash`/`python3`/etc.) does not
close this**, because an interpreter name is not the only way to reach
arbitrary execution: `/usr/bin/env`, present on effectively every POSIX
system specifically *so that* other programs can invoke an interpreter by
name through it, works as well or better as a laundering target — its own
name is innocuous, and its job is exactly "look up and exec another
program from `PATH`, with the given argv." I independently confirmed the
underlying argv primitive, with a harmless marker, isolated from the
registry pipeline itself (consistent with the review's own note that this
tests the primitive, not the full pipeline):
```sh
d=$(mktemp -d)
marker="$d/exec007_marker"
env sh -c 'touch "$1"' sh "$marker"     # env invokes sh; sh receives the one explicit marker path as $1
test -f "$marker" && echo "confirmed: env invoked sh, which created $marker" || echo "NOT confirmed"
rm -rf "$d"
```
Result (exact block above, run as written): `confirmed: env invoked sh,
which created <tmpdir>/exec007_marker`, and the disposable directory was
removed afterward. (Corrected this revision: the prior block expanded `$$`
once in the child shell and once in the parent, so its `test -f` checked a
different path than the one created and could not print "confirmed" — the
primitive held, but the recorded assertion did not follow from the
commands. The block above passes one explicit path to both steps.)
This shows `env` can be named as the resolved "program" and still reach
arbitrary execution through its own argv. Other common utilities with the
same property (for example `xargs`, `find ... -exec`, or `perl -e`) were
not individually tested here and are named only to show the class is
larger than one program,
meaning a name-based blacklist is addressing only the most naive instance
of a broader class, not the class itself.

**Trust boundary, stated explicitly (the review's own instruction — do not
silently reject this path merely because the URL is configured
locally):** `acp_registry_url` is a daemon-configured constant
(`https://cdn.agentclientprotocol.com/...`), not repository or forge
content, and `download_index` (`acp_discovery.rs:266-280`) fetches it over
`reqwest`'s default HTTPS client with certificate validation on. The
prerequisite for this finding is therefore **not** "a hostile repository"
or "a hostile pull request" — it is a compromise of the registry endpoint
itself, of the CDN serving it, or of the TLS channel between the daemon
and it (a successful MITM defeating certificate validation, or a
substituted/compromised server at the configured hostname). This is a
materially different, and higher, bar than any other finding in this
report, but the task's threat model explicitly names "network requests" as
potentially hostile alongside repository/forge content, which is why it is
recorded here rather than dismissed.

**Limitations, stated plainly:** I did not stand up a fake registry
endpoint and point a disposable daemon at it — doing so would exercise the
full daemon/discovery pipeline rather than the one function I traced, and
was judged not to fit this pass's remaining time. The mechanism above is
traced through source, function by function, with exact line citations,
and the one isolated primitive (`env` indirection) is independently
confirmed; the finding as a whole is not reproduced end-to-end the way
EXEC-001/005/006 are. Treat the mechanism as well-evidenced and the
exploit as **unproved**, exactly as the review instructed — this section
makes no claim stronger than that.

**Severity:** the mechanism, if the trust boundary is defeated, is High
(arbitrary local code execution with attacker-chosen arguments, no
repository/forge prerequisite, and no dependency on an interpreter-name
blacklist since indirection defeats that class of defense); the
*likelihood* is gated entirely by a TLS/endpoint compromise this report
does not claim to have observed or have reason to believe has occurred.

**Confidence:** High on the code-level mechanism (every step cited and
re-read for this entry, including the corrected candidate-containment
shape) and on the indirection primitive (independently reproduced); Low on
real-world likelihood, which depends entirely on the trust boundary above
holding or not — a question about CDN/TLS security practice, not about
this codebase.

**Recommendation, with acceptance criteria covering indirection and every
candidate path, not only the binary one:** a name blacklist
(`sh`/`bash`/`python3`/`env`/...) is, at most, limited hardening — it
narrows the easiest exploitation path but cannot enumerate every
indirection-capable program on a real system, so it should not be
presented as closing the gap. The actual control needed is over **trusted
program identity**: every candidate that currently has `package: None`
(the binary candidate *and* the agent-id-named npx/uvx candidate) needs
some equivalent of the containment the package-name-named candidate
already gets — e.g. a recorded, pinned absolute path (or hash) per agent
id that discovery is expected to keep matching across restarts, with any
drift treated as a rejection requiring re-confirmation, rather than silent
re-trust of whatever the index says to search for next. *Acceptance
criteria:* (a) an index document naming a "binary" `cmd` of `sh`/`bash`/
`env`/`python3` for some existing agent id is rejected at
parse/`candidates()` time, before any `which()` search or spawn, exercised
for **both** the binary candidate path and the agent-id-named npx/uvx
candidate path, not only one; (b) an index document that leaves the `cmd`
unchanged but changes only `args` to something indirection-shaped (e.g.
`["-c", "<arbitrary>"]` for a previously-benign binary) is also rejected or
flagged, since identity alone does not bound what arguments a trusted
binary is run with; (c) the fix is verified against the actual resolved
path matching (or not) a previously-recorded one, not against a name
blacklist, so that a new indirection utility added to the blacklist
tomorrow is not required to keep the fix correct today.

**Compatibility decision for the user:** whether to also add index-level
integrity verification (a signature or checksum beyond TLS) is a larger
product/supply-chain decision, routed to the orchestrator rather than
decided here.

---

### Verified-safe controls (re-checked this revision; EXEC-POS-3 corrected)

1. **Task branch names cannot carry an injection-shaped prefix.**
   `crates/ariadne-store/src/tasks.rs:67-119` reduces a task's title to
   `[a-z0-9-]+`, cannot begin with `-`. Unchanged, still holds.
2. **`has_commit` already defends itself** with
   `rev-parse --verify --quiet --end-of-options` (`gitwt.rs:250-260`) — the
   in-tree precedent for EXEC-001's fix on the `rev-parse` sinks. It is not
   a precedent for `add_worktree`, which needs the base resolved to an
   object id first.
3. **PATH search for agent *discovery* rejects relative/empty entries and
   checks package containment** — `acp_index.rs:65-76`
   (`searchable`) and `acp_index.rs:121-137` (`installs`) are real and
   correctly implemented, and I independently re-verified `command_of`'s
   doc comment is **aspirational, not descriptive**: it says this is "what
   the launcher runs," but the launcher does not, in fact, run the
   absolute path this filtering exists to produce. **The previous
   report's claim that this closes a discovery-to-launch TOCTOU window is
   retracted — see EXEC-005, which is precisely that window.** What
   remains true and worth keeping: the *naming decision* (which agent id
   maps to which command) cannot be influenced by a relative `PATH` entry
   or a same-named decoy package; only the *final executed file*, at
   launch time, can be — and is.
4. **ACP declares no filesystem or terminal capability, and implements no
   handler for either.** `acp.rs:2620`
   (`v1::ClientCapabilities::new().terminal(false)`, no `.fs(...)`);
   independently re-confirmed with `grep` (not only the sub-agent's
   report) that `acp.rs` registers exactly two inbound handlers
   (`session/update`, `session/request_permission`) and none for `fs/*` or
   `terminal/*`.
5. **Repository-path resolution for a loose session cannot be confused by
   a sibling directory name.** `launcher.rs:300-306` uses
   `Path::starts_with` (component-wise), confirmed by reading the exact
   call, not a string prefix.
6. **The skills symlink is created and torn down safely.**
   `agents/mod.rs:96-104`/`125-136` — `skills_dir` is always the daemon's
   own configured path, never repository-controlled; removal checks
   `symlink_metadata().is_symlink()` before deleting, never following the
   link.

### Rejected hypotheses

- **`ext::` as an RCE vector through `fetch_head_of`.** Retested this
  revision, same result: `fatal: transporte 'ext' no permitido`, git's own
  default, not a control Ariadne implements (see EXEC-002's note on this).
- **`git ls-remote --heads <remote> <branch>` injection via a trailing
  `--upload-pack=<cmd>`-shaped `branch`.** Retested with `GIT_TRACE=1`:
  git invokes its normal `git-upload-pack <path>` regardless. Args after
  the first non-option positional are literal ref-pattern filters here,
  not re-parsed as options.
- **The `diff {base}...{branch}` sink being reachable with a dangerous
  payload through `channel.rs:454`/`steps.rs:75`.** Narrowed, not fully
  rejected, this revision: the `merge-base` guard does not block *every*
  dash-prefixed payload (see EXEC-001's `--fork-point`/`--independent`
  cases), but no payload found is *also* a dangerous `git diff` option, so
  the end-to-end chain through these two call sites remains safe for every
  payload tried.
- **A sibling directory name letting a loose session borrow another
  repository's permission mode.** Confirmed `Path::starts_with` is
  component-wise. Rejected.
- **The skills-folder symlink being poisoned by repository content.**
  `link_skills`'s `skills_dir` argument is always the daemon's own
  configured path. Rejected as a repository-content vector.
- **The ACP registry index download being a repository/forge-content
  vector specifically.** Still correct as far as it goes: the URL, client,
  and transport are all daemon-configured and TLS-protected; nothing
  repository- or forge-sourced reaches this path. Rejected *as a
  repository/forge-content vector*, which is this audit's named question.
  **Corrected this revision:** this is not the same as "a tampered index
  cannot influence the executed file" — the task's threat model explicitly
  includes hostile *network content*, and the index is fetched over the
  network. What the index can and cannot influence, traced precisely, is
  now its own finding — see EXEC-007 above, which replaces the overbroad
  claim the prior revision made here.

### Hardening suggestions (not repository/forge-content vectors; recorded because they sit on the mechanism this task asked about)

- **Skill-name path traversal via the daemon's own HTTP API, not
  repository content.** The sub-agent traced `agents/mod.rs`'s
  `sync_skills` (`dir = skills_dir.join(&skill.name)`) and
  `agents/prompts.rs:110` and found `skill.name` is never validated
  against `/`, `\`, or `..`. **Scope:** skills are created through the
  daemon's own API by whoever can reach it locally, not scanned out of a
  repository or a forge response — explicitly not a "hostile repository"
  finding, carried at the sub-agent's confidence level (I did not
  independently re-verify the write path myself, only the symlink-creation
  half). **Acceptance criterion if pursued:** a skill name containing `/`,
  `\`, or a `..` component is refused at creation (400), not silently
  joined into a path outside `skills_dir`.
- **`ariadne doctor`'s git floor (2.42) does not track git's own
  security-patched point releases** (2.42.2, per the May 2024 batch above).
  No reachable path from Ariadne's own invocations to the vulnerable
  operation was found (see Coverage), so this is hygiene, not a finding.
  **Acceptance criterion if pursued:** `doctor` additionally warns (not
  blocks) below 2.42.2 specifically, citing the CVE batch.

## Recommendation

Fix EXEC-005 first — it is the highest-confidence, highest-severity,
smallest-surface-area change (route the already-resolved absolute path
through to the actual spawn for discovered agents), and it is reachable by
the author of any request the user asks Ariadne to review, through the
PR-review path, gated by the user's own review-request action plus the
daemon-`PATH` prerequisites now stated
explicitly in its finding. Fix EXEC-001 per sink, not with one uniform
marker: `--end-of-options` for `delete_branch`/`diff`/`committed_clean`/
`checkout`, the fuller `--verify --quiet --end-of-options` pattern for
`remote_has_branch_tip`'s `rev-parse` call (a bare marker would break that
function's SHA comparison for every legitimate branch), and a
resolve-to-object-id step — never a marker alone — for `add_worktree`'s
`base` argument specifically, since that sink's internal branch-creation
step can silently consume a recognized flag (`--force`) and fall back to
the repository's current `HEAD` instead of erroring, which no option
marker closes. Treat EXEC-006 as a documentation task (describe the
actual, cooperative nature of "read-only" and the permission modes
accurately in `docs/`, including the precise, non-"always yes" shape of
`auto`) rather than a code fix, pending the orchestrator's call on whether
to pursue ACP `fs`/`terminal` mediation as a larger, separate product
direction. Route EXEC-002 and EXEC-004 to a joint EXEC/FORGE follow-up per
the compatibility decisions named in each — both still open, not decided
here. Ask the orchestrator whether EXEC-003 is worth a dedicated
reproduction task before scheduling a fix. For EXEC-007, do not treat an
interpreter-name blacklist as the fix: it is limited hardening at best,
since indirection utilities like `env` reach the same outcome; the actual
need is a trusted-identity-and-arguments control covering every
zero-containment candidate path (the binary candidate and the
agent-id-named npx/uvx candidate alike), independent of whether
index-level signing is ever pursued.

## Confidence

High on EXEC-005 (direct reproduction of the exact primitive, no
abstraction gap, prerequisites now stated explicitly) and on EXEC-001's
per-sink table, its `remote_has_branch_tip` correction, and its `--force`
bypass of the `add_worktree` fix (each cell, including both corrected
fixes, independently executed with two distinct commits and a real
dash-named ref). High on EXEC-006's
mechanism (independently reproduced, not only cited from relayed evidence)
and on the conditional hooks/filters path (independently reproduced with
an isolated `$HOME` and a clean-`HOME` control). High on EXEC-007's
code-level mechanism (every citation re-read for this entry), Low on its
real-world likelihood (gated on a TLS/endpoint compromise not observed or
claimed). Medium on EXEC-001's "no worse accepted option exists" claim
(not exhaustive). Medium on EXEC-002 (depends on FORGE guarantees not
independently verified beyond reading cited source). Medium on EXEC-004's
branch-name mechanism (fully reproduced) but Low on whether a real agent
would actually execute the generated text verbatim — not built as a live
reproduction in this pass, which is a time/scope-of-this-pass limit, not
something the agreed scope forbids testing (corrected this revision).
Low-Medium on EXEC-003 (code-read only). Low on EXEC-006's severity
ceiling specifically because what would raise it (a real agent actually
doing this from untrusted content) was not built as a live reproduction in
this pass — again a limit of this pass, not an exclusion the scope states.

## What would change this

- A live daemon reproduction of EXEC-005 (deliberately misconfigured `PATH`
  on a disposable daemon instance) would move it from "mechanism proven in
  isolation" to "proven end-to-end," though the severity assessment would
  not meaningfully change.
- Confirmation from FORGE about whether `pull.head_repo`/`head_branch` can
  ever carry a non-platform-constrained value would raise or lower
  EXEC-002.
- A live reproduction of a non-terminal task losing its worktree directory
  while still being revivable would confirm EXEC-003.
- Access to a Windows host would let EXEC-005's platform-specific note be
  confirmed or ruled out rather than left as a named gap.
- A live daemon plus a stand-in agent, built to actually follow or ignore
  the generated push command (EXEC-004) or an untrusted-content instruction
  (EXEC-006), would move either from "the layer provides no technical
  restriction" to a measured, agent-specific result — buildable within the
  agreed scope (it permits isolated daemon instances and service
  stand-ins), just not built in this pass.
- A survey of real Ariadne deployments' daemon `PATH` configurations would
  let EXEC-005's and EXEC-006's likelihood (not mechanism) be stated with
  more than a qualitative "interactive vs. service-managed" distinction.
- Standing up a disposable registry-index endpoint and pointing a
  disposable daemon at it would move EXEC-007 from a code-traced mechanism
  to an end-to-end reproduction.

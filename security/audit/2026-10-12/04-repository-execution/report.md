# EXEC — Audit repository inputs through Git worktrees and agent execution

> Revision 2. A review pass found a false-positive control, an incomplete
> option-injection characterization, an unsafe universal remediation, a
> scope error on EXEC-004, and incomplete coverage/evidence. This revision
> corrects all five, adds a new confirmed finding (EXEC-005), and adds the
> subprocess/hooks/advisory coverage the review asked for. Every claim below
> was independently reproduced by me (not merely asserted) with the exact
> commands shown; search for "Repro:" to find each one.

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

| Call site | Program source | cwd | Env | Descriptors | Cancel/cleanup |
|---|---|---|---|---|---|
| `gitwt.rs:42-63` (`GitManager::git`, used by every `git` operation in that file) | literal `"git"` | caller-supplied `repo`/`worktree` path (internal or registered-repo path, never raw repo/forge text — see table in EXEC-001 for which *arguments* are repo/forge-sourced) | inherited, plus `GIT_TERMINAL_PROMPT=0` for a `push` | `.output()` (piped, captured, not inherited) | awaited synchronously; no detached child survives the call |
| `probe.rs:87-98` (`run`, used by `ariadne doctor`/`/v1/doctor` probes) | caller-supplied `program: &OsStr` (always a `PathBuf` from `which()`/a configured command, never repository content) | not set (inherits daemon's cwd) | inherited | `.output()` under `kill_on_drop(true)` and a 3s `tokio::time::timeout` | `kill_on_drop` plus the timeout guarantees no probe outlives the report that asked for it |
| `acp.rs:963-990` (`AcpRuntime::launch`, every ACP agent process) | `launch.program: String` — **see EXEC-005**: for a discovered registry agent this is the *bare command name*, not the absolute path discovery resolved | `launch.cwd` — the task worktree, the PR review/keeper worktree, or the repository's primary checkout, **all either repository content or derived from it** | daemon's inherited env plus `launch.env` (session/launch/goal/task ids, `ARIADNE_SOCKET`, and for `codex-acp`, `INITIAL_AGENT_MODE=agent-full-access` — none of this is repository-sourced) | piped stdio; `process_group(0)` | `kill_on_drop(true)`; `take_down`/`kill_gated` (`acp.rs:1076-1198`) signal the whole process group and are launch-id-gated so a kill cannot land on a process that already replaced the one it targeted (verified in the sub-agent's pass, see EXEC-POS list) |
| `acp_discovery.rs` probe calls (`probe_version`/`probe_status`/`list_stored_sessions`, reusing `probe.rs::run`) | resolved `program: PathBuf` from discovery, or a configured command | daemon's own `probe_cwd` (a directory of its own, not a repository) | inherited | as `probe.rs::run` | as `probe.rs::run` |
| `acp_discovery.rs:266-280` (`download_index`) | not a process — `reqwest::Client` HTTPS GET | n/a | n/a | n/a | request-scoped `reqwest` timeout (`Timeouts::registry_download`, 30s) | 

**Registry-download path, traced per the review's instruction (previously
wrongly excluded):** `download_index` (`acp_discovery.rs:266-280`) fetches
`acp_registry_url` (default
`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, a
daemon-configured constant, never repository or forge content) over
`reqwest`'s default HTTPS client (certificate validation on, no
`danger_accept_invalid_certs`), calls `.error_for_status()` (rejects a
non-2xx response), decodes it as UTF-8, and accepts it only if
`acp_index::validate` can parse it as the expected JSON shape
(`acp_index.rs:34-37`) — which checks *shape*, not *authenticity*: there is
no signature or checksum pinning beyond TLS transport security. **This is
not a repository/forge-content vector** — nothing from a registered
repository or a pull request reaches this URL, this client, or this
validation — so it does not bear on the assigned question, but I traced it
rather than excluding it, per the review. What it *does* influence (which
command *names* get searched for on `PATH` — never which file executes, see
EXEC-POS-3/EXEC-005) remains unaffected by whether the index itself is
later found to be tampered with, since `which()` still only ever returns a
file genuinely present on the daemon's own `PATH`.

### Git configuration, hooks, and filters

Explicitly tested, since Work item 2 asks for it and the original report
only reasoned about it without evidence:

**Hooks are never transferred by clone or fetch.** `Repro:`
```sh
d=$(mktemp -d); cd "$d"
git init -q -b main evil && cd evil
printf 'echo HOOK_RAN > /tmp/hook_marker\n' > .git/hooks/post-checkout
chmod +x .git/hooks/post-checkout
echo content > f.txt && git add . && git -c user.email=t@t -c user.name=t commit -q -m init
cd "$d" && rm -f /tmp/hook_marker
git clone -q "$d/evil" victim
test -f /tmp/hook_marker && echo BAD || echo "good: hooks are purely local"
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

**A tracked `.gitattributes` naming an undefined filter is inert.** `Repro:`
```sh
d=$(mktemp -d); cd "$d"
git init -q -b main repo && cd repo
echo "*.txt filter=evilfilter" > .gitattributes
echo secret > data.txt
git add . && git -c user.email=t@t -c user.name=t commit -q -m "tracked attrs"
cd "$d" && rm -f /tmp/filter_marker
git clone -q "$d/repo" victim2
test -f /tmp/filter_marker && echo BAD || echo "good: filter is a no-op without local filter.<name>.* config"
```
Result: `good`. A `clean`/`smudge` filter command must be set in local
(never-transferred) git config (`filter.<name>.clean`/`.smudge`); a
repository's own tracked `.gitattributes` can *name* a filter but cannot
*define* one. This closes off the other classic git-configuration RCE
class for the same reason hooks are closed off: the dangerous half of the
mechanism is local-only.

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
  `ls-remote --heads`, `diff`, and `checkout --detach`.
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
- **New this revision:** hooks-via-clone and gitattributes-filter-via-clone,
  both tested and rejected (see "Git configuration, hooks, and filters").

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
- **The ACP registry index's own download/integrity path** — traced this
  revision (see above); concluded it is not a repository/forge-content
  vector, so no finding filed, but no longer silently excluded.

## Additional acceptance criterion

**Repository-controlled execution vs. user-configured/trusted programs:**
the *program name* a session is pinned to run is never chosen by repository
or forge content — it always comes from `AgentRegistry::command_of`, fed by
either the vendored/downloaded ACP index filtered against the daemon's own
`PATH` (`acp_index.rs`), or the daemon's own `[[acp_agents]]`
configuration. Repository/forge content never supplies argv\[0\]. **What
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
i.e. an external contributor's content, reachable by anyone who can open a
PR against a repository Ariadne is configured to review, with no local
access to the daemon's host at all).

**Transformation:** none beyond what `git worktree add`/`checkout` already
does to populate the worktree from the branch's tree. A tracked file keeps
its executable bit through an ordinary clone/checkout (verified, see
below).

**Prerequisite (the one environmental condition this depends on):** the
daemon process's own ambient `PATH` environment variable must contain a
relative entry (an empty entry, `.`, or any relative directory) positioned
*before* the absolute directory holding the real agent binary — or must
simply lack an absolute entry for that name at all. `searchable()`
(`acp_index.rs:65-76`) filters such entries out **only for the purposes of
deciding which agents to register and what `program` to record for
them** — it does not change the daemon's real `PATH`, which is exactly
what the OS consults again, unfiltered, when `acp.rs` spawns `Command::new`
with a bare name.

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
the repository-content half of the prerequisite is satisfiable by an
**external, unauthenticated contributor who can merely open a PR** against
a repository configured for auto-review — no access to the daemon's host,
account, or configuration is needed from that side at all. The remaining
prerequisite (the daemon's own `PATH`) is outside repository/forge control
and is not guaranteed to hold on any given deployment — a daemon started
by `systemd`/`launchd` with an explicit, fully-absolute `PATH` in its unit
file is not exposed; a daemon started interactively from a developer's own
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

**What changed in this revision, and why:** the prior report claimed every
reachable sink "fails safely" (command/gate failure, no state change). The
review correctly showed this is false for at least one sink
(`add_worktree`'s `--orphan`) and showed my universal remediation (`--`)
would have broken two sinks' legitimate behavior had anyone applied it.
Both are corrected below, per sink, with the exact command and result for
every payload tried.

**Per-sink results, retested completely:**

| Sink (`gitwt.rs`) | Shape | Crafted payload tried | Result | Impact class |
|---|---|---|---|---|
| `add_worktree` (`:70-89`), `base` at line 82 | `worktree add -b <branch> <wt> <base>` | `--output=<path>` | `error: opción … desconocida`, exit 128 | fails safely |
| same | same | **`--orphan`** | **succeeds, exit 0** — creates `<branch>` as an *unborn* branch with no commits, no resolvable `HEAD`, none of the base branch's tracked files | **silent semantic corruption**: the task worktree is created but is not based on the repository's history at all |
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

**Severity:** Medium, revised from the prior "DoS-only" framing to
"DoS-or-silent-worktree-corruption", still well short of file write or code
execution. The `--orphan` case is the most concrete new piece of evidence
and the reason I would not downgrade this below Medium.

**Confidence:** High on every result in the table (each independently
executed, not inferred); Medium on "no sink has a worse accepted option
than what I found", for the stated reason (not exhaustive).

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
git worktree add -b taskB wt --end-of-options --orphan # fatal: referencia inválida: --orphan — rejected (the finding's own payload, now blocked)
git branch -D --end-of-options base2                   # correctly deletes; a crafted value after the marker is "not found", not misparsed
git rev-list --count --end-of-options main..taskC       # correct count (1), preserved
git ls-remote --heads origin --end-of-options main       # correct ref line, preserved
```
`has_commit` (`gitwt.rs:250-260`) already uses `--end-of-options` and is
the existing, in-tree precedent for this fix — the recommendation is to
extend the same marker to every other sink in the table above, not to
introduce a new convention.

**Regression criterion:** for each guarded sink, (a) a same-shaped,
non-malicious branch/ref name of equal or greater length continues to
produce the identical observable result as before the fix (the specific
commands and expected outputs above), and (b) a repository whose relevant
branch/ref is named `--orphan`/`--output=<path>`/`--upload-pack=<cmd>`/
`-fother` (one test per sink) is observed to fail with a clear git error
rather than succeeding with different semantics or writing to any path
outside the intended target.

---

#### EXEC-002 — `git fetch`'s remote argument is taken from forge data with no scheme/shape gate at the point of use

*(Unchanged in substance from the prior revision; scope/exclusions/
acceptance criteria added per the review.)*

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

*(Unchanged in substance; scope/exclusions/acceptance criteria added.)*

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
code-read-based finding, not a live one, and is reported as an unverified
candidate for that reason.

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

*(Substantially reassessed this revision — the prior framing incorrectly
dismissed this as requiring a "nonstandard forge" and over-applied the
"trust the agent" exclusion to Ariadne's own command-construction code.)*

**Affected files/lines:** `launcher.rs:1579-1585` (`keeper_push_command`,
builds `format!("git push {} HEAD:{}", remote, pull.head_branch)`) and
`launcher.rs:1876-1897` (`keeper_briefing_of`, embeds that exact string in
the keeper agent's briefing as "the command that pushes a fix to it").
Both `remote` (from `keeper_remote`, which can be a forge-sourced URL per
EXEC-002) and `pull.head_branch` (the forge-reported PR head branch name)
are interpolated with no quoting.

**Corrected premise:** the prior revision assumed shell-metacharacter
branch names require a nonstandard forge. This is false and was
disproven directly:
```sh
git push origin 'HEAD:refs/heads/topic;echo${IFS}EXEC_MARKER_REPRO'
# pushed OK, exit 0 — entirely ordinary git/GitHub/GitLab usage accepts this
git ls-remote --heads origin
# 92027054...  refs/heads/topic;echo${IFS}EXEC_MARKER_REPRO
```
and the generated instruction, if handed to any shell for literal
execution, does exactly what the string says:
```sh
generated_command="git push origin HEAD:topic;echo\${IFS}EXEC_MARKER_REPRO"
sh -c "$generated_command"
#  * [new branch]      HEAD -> topic
# EXEC_MARKER_REPRO
```
Both halves (the push, and the injected `echo`) ran as two separate shell
commands — the push's success or failure is irrelevant to whether the
injected half executes, matching the review's own finding using a
nonexistent remote.

**Corrected scope boundary:** the exclusion that applies here is "we do
not need to stop a malicious installed agent from misusing its own shell
access" — it is **not** "we can ignore a daemon-constructed string merely
because a trusted agent is the one that might run it." The agent in this
scenario is not misbehaving; it is doing exactly what a well-behaved agent
does with an instruction captioned "the command that pushes a fix" —
running it. The unsafe artifact is the string Ariadne built, not the
agent's judgment.

**Remote interpolation, assessed separately:** not separately reproduced
with a crafted `remote` value in this pass (would need a crafted forge
response feeding `keeper_remote`, which is FORGE-adjacent setup), but the
same `format!` call interpolates it with the same lack of quoting; whatever
shape of value can reach `remote` (a forge-sourced URL, per EXEC-002)
carries the identical risk via the identical mechanism. Recorded as
reasoned-but-not-independently-reproduced, distinct from the branch-name
half which is fully reproduced above.

**Severity:** Medium. Confirmed mechanism (branch-name half), ordinary
forge usage suffices as the prerequisite, but the final step (whether an
agent actually executes unmodified text it's given, rather than composing
its own command or asking for confirmation) remains agent-behavior-
dependent and was not — and under the agreed scope, could not be —
verified against a real agent.

**Recommendation, with acceptance criteria:** this is EXEC's own string-
construction choice, fixable unilaterally regardless of what FORGE
validates upstream (keeping FORGE's metadata-ownership and EXEC's command-
construction concerns separate, per the cross-task contract): either (a)
describe the push in prose ("push your branch to `<remote>`'s copy of
`<head branch>`") and let the agent compose its own command, removing the
ready-made shell string entirely, or (b) if a literal command is kept,
single-quote both `remote` and `pull.head_branch` with embedded-quote
escaping (POSIX shell quoting: wrap in `'...'`, replacing any `'` with
`'\''`) before interpolation. *Acceptance:* a branch name containing `;`,
`$()`, backticks, or spaces, when the generated string is parsed by a
POSIX shell, is treated as a single whole argument to `git push`'s
refspec — verified by constructing exactly such a name and confirming (i)
no side-effect command executes when the generated text is run through
`sh -c`, and (ii) the push still targets the literal, unmangled branch
name including its special characters.

---

### Verified-safe controls (re-checked this revision; EXEC-POS-3 corrected)

1. **Task branch names cannot carry an injection-shaped prefix.**
   `crates/ariadne-store/src/tasks.rs:67-119` reduces a task's title to
   `[a-z0-9-]+`, cannot begin with `-`. Unchanged, still holds.
2. **`has_commit` already defends itself** with `--end-of-options`
   (`gitwt.rs:250-260`) — now cited as the in-tree precedent for EXEC-001's
   fix rather than only as a standalone positive.
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
  vector.** Traced this revision (see Coverage); the URL, client, and
  validation are all daemon-configured and TLS-protected; nothing
  repository- or forge-sourced reaches this path. Rejected as in-scope for
  this audit's question, though noted as a general (non-repo-content)
  hygiene observation (no signature/checksum beyond TLS + JSON-shape
  validation).

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
an external, unauthenticated party through the PR-review path. Fix
EXEC-001 with `--end-of-options` at each cited sink, verified per-command
exactly as shown rather than applied as a blind find-and-replace. Route
EXEC-002 and EXEC-004 to a joint EXEC/FORGE follow-up per the compatibility
decisions named in each. Ask the orchestrator whether EXEC-003 is worth a
dedicated reproduction task before scheduling a fix.

## Confidence

High on EXEC-005 (direct reproduction of the exact primitive, no
abstraction gap) and on EXEC-001's per-sink table (each cell independently
executed). Medium on EXEC-001's "no worse accepted option exists" claim
(not exhaustive). Medium on EXEC-002 (depends on FORGE guarantees not
independently verified beyond reading cited source). Medium on EXEC-004's
mechanism (fully reproduced) but Low on whether a real agent would actually
execute the generated text verbatim (not testable under the agreed scope).
Low-Medium on EXEC-003 (code-read only).

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

# EXEC — Audit repository inputs through Git worktrees and agent execution

## Question

Can hostile repository metadata or paths cause unintended commands, file
access, or process behavior?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (branch
`audit-repository-inputs-through-git-ahcj9j`, base `main`), the same commit
named as the planning reference. The worktree was clean throughout; nothing
in `crates/`, `specs/`, or elsewhere was modified for this audit — only the
`security/audit/2026-10-12/04-repository-execution/` directory is new.

Git used for every reproduction below: `git version 2.55.0` (this machine's
installed `git`, run in disposable `mktemp -d` repositories — never against
the user's own repositories, daemon, or accounts).

## Threat assumptions (as given)

- Repository content, forge content, browser/network content and other
  users' files are potentially hostile; the daemon's own code is the thing
  under review, not the user.
- Installed coding-agent *programs* are trusted once the user has explicitly
  selected/installed them. We are not trying to contain a deliberately
  malicious agent *process* (e.g. constrain its shell access). We *are*
  trying to find places where Ariadne's own code — independent of any
  agent's choices — builds a command, path, or process from hostile
  repository/forge content without validating it first.
- The report must distinguish repository-controlled execution from
  user-configured commands and explicitly selected trusted programs (see
  [Additional acceptance criterion](#additional-acceptance-criterion),
  below).

## Coverage

### Reviewed in full
`crates/ariadne-daemon/src/gitwt.rs`, `branch.rs`, `checkpoint.rs`,
`launcher.rs`, `probe.rs` (ariadne-core), `acp_registry.rs`
(ariadne-store), `crates/ariadne-store/src/repositories.rs`,
`crates/ariadne-store/src/tasks.rs` (branch-name slugging),
`crates/ariadne-daemon/src/http/repositories.rs`, `http/channel.rs`
(diff endpoint), `http/steps.rs` (gates), `http/pull_requests.rs` (diff
endpoint + `is_sha`), `scheduler/pull_requests.rs` (merge-landing),
`agents/mod.rs` (`link_skills`), `forge/mod.rs` (`Remote::parse`/`rewrite`),
`acp_index.rs` (`searchable`, `installs`), `specs/002`, `specs/007`.

A background sub-agent, briefed with the same threat model, independently
read and reported on: `acp.rs` (3637 lines), `acp_calls.rs`,
`acp_transport.rs`, `agents/{acp,handoff,prompts,mod}.rs`,
`ariadne-core/src/workflow.rs`, plus follow-ups it made itself into
`acp_discovery.rs`, `forge/github/pulls.rs`, `forge/gitlab/pulls.rs`,
`learned_key.rs`, `http/skills.rs`, `ariadne-store/src/skills.rs`. Every
claim from that sub-agent cited in this report was independently
spot-checked by me against the source or reproduced before being included
(see each finding's "Verification" line); two of its framings were narrowed
or corrected by my own reproductions (see EXEC-001).

### Tested boundaries (live reproductions, disposable repos only)
- Whether a git branch name can carry a leading `-`/`--` (via `update-ref`,
  then via an actual `git clone` of a bare repo whose `HEAD` points at such a
  branch) — confirmed yes, end to end.
- Whether `git diff`, `git merge-base`, `git rev-list --count`,
  `git worktree add`, `git rev-parse`, `git branch -D`, and `git ls-remote`
  treat a `-`/`--`-prefixed argument in the exact argv shapes
  `crates/ariadne-daemon/src/gitwt.rs` uses as an option, a literal ref
  name, or an error, for each of the call shapes actually present in this
  codebase.
- Whether `git`'s `ext::` transport is reachable through
  `GitManager::git` as deployed here (no `GIT_ALLOW_PROTOCOL` override is
  set by this codebase) — confirmed blocked by this git's own default
  (`fatal: transporte 'ext' no permitido`).
- Whether the daemon's ACP `initialize` call advertises `fs/*` or
  `terminal/*` capabilities, and whether any handler for them exists in
  `acp.rs` — confirmed neither.
- Whether a sibling directory name (e.g. `/repo` vs `/repo-evil`) can be
  mistaken for a registered repository when resolving a loose session's
  permission mode (`launcher.rs:300-306`) — confirmed not, `Path::starts_with`
  is component-wise.

### Excluded, with reason
- **Containing a malicious *installed* agent binary's own behavior** (tool
  calls it makes once running, what it does with a shell it already has) —
  explicitly out of scope per the task's agreed scope.
- **FORGE's own metadata ingestion/validation** (`forge/github/pulls.rs`,
  `forge/gitlab/pulls.rs` field-level parsing, webhook authenticity) — owned
  by the FORGE task. Where EXEC's sinks in `gitwt.rs`/`launcher.rs` consume
  forge-sourced strings with no defense of their own, that is reported here
  as cross-boundary evidence (EXEC-002), not re-litigated as a FORGE defect.
- **PERM's permission-request answering** (`acp.rs`'s
  `session/request_permission` handler, `learned_key.rs`'s matching logic
  beyond the one `..`-escape check reused from the sub-agent's pass) — owned
  by the PERM task.
- **DATA's persisted resume records** (stored-conversation readers under
  `stored_conversations.rs`) — owned by the DATA task; noted only where the
  sub-agent's pass surfaced something crossing into EXEC's execution path,
  which it did not.
- **A live SSH or `git://` remote.** The sandbox had no such remote
  available; every reproduction used a local filesystem path as the
  "remote". This leaves one sub-question in EXEC-001 genuinely open rather
  than closed by testing (noted there).
- **`agents/prompts.rs`'s full template-substitution logic** beyond the one
  skill-path join the sub-agent traced, and **`acp.rs` lines ~1600–2330**
  (`handle_update`/`handle_permission`) beyond a `Command::new`/`join(`
  grep sweep — read partially, not line-by-line; flagged as a coverage gap
  rather than silently passed over.
- **The ACP registry index's own download/integrity path** (whatever fetches
  the JSON before `acp_registry.rs` stores it) — only the storage half was
  in scope's file list; the fetch path lives outside the files named in
  this task.

## Additional acceptance criterion

**Repository-controlled execution vs. user-configured/trusted programs:**
the *program* a session runs is never chosen by repository or forge
content. `Launcher::launch` (`launcher.rs:263-349`) resolves the spawned
program through `AgentRegistry::command_of` (`acp_discovery.rs:486-488`),
which returns either (a) a command discovered by searching the daemon's own
`PATH` — filtered to absolute entries only, and for npx/uvx-style agents,
containment-checked against the actual package directory the resolved file
or its symlink chain sits under (`acp_index.rs:65-137`, see EXEC-POS-3
below) — or (b) a command from the daemon's own `[[acp_agents]]`
configuration. Repository/forge content influences only: the process's
*cwd* (an internally-derived worktree path, never a repo-supplied string —
see EXEC-POS-1), its *environment* (session/task/launch ids, all minted by
the daemon), and the *prompt text* it is briefed with (natural language,
interpreted by the trusted agent, not executed by Ariadne — see EXEC-004 for
the one place this line gets blurry). No file or path taken from a
repository or a forge response ever becomes the executable path or argv\[0\]
of a spawned process in the code reviewed.

## Findings

### Confirmed

#### EXEC-001 — Hostile repository branch names reach several `git` invocations as unguarded, option-parseable arguments (CWE-88 argument injection); confirmed harmless in the current git version, but a real and inconsistently-defended gap

**Affected files/lines:**
- `crates/ariadne-daemon/src/gitwt.rs:70-89` (`add_worktree`, the `base`
  argument at line 82)
- `crates/ariadne-daemon/src/gitwt.rs:341-344` (`delete_branch`)
- `crates/ariadne-daemon/src/gitwt.rs:350-365` (`remote_has_branch_tip`,
  both the `rev-parse branch` call and the `ls-remote --heads remote
  branch` call)
- `crates/ariadne-daemon/src/gitwt.rs:370-384` (`remote_branch_tip`, same
  shape)
- `crates/ariadne-daemon/src/gitwt.rs:26-40` (`committed_clean`, the
  `rev-list --count {base}..{branch}` range)
- `crates/ariadne-daemon/src/gitwt.rs:414-420` (`diff`, the preliminary
  `merge-base base branch` call and the `diff {base}...{branch}` call)
- Reached via `repo.base_branch` from
  `crates/ariadne-daemon/src/http/channel.rs:454`,
  `crates/ariadne-daemon/src/http/steps.rs:75-80`,
  `crates/ariadne-daemon/src/launcher.rs:846-851` (`task_worktree` →
  `add_worktree`)
- Reached via `pull.head_branch`/`pull.base_branch` (forge-sourced) from
  `crates/ariadne-daemon/src/launcher.rs:1609` (`remote_branch_tip`, inside
  `keeper_branch_is_safe_to_discard`) and
  `crates/ariadne-daemon/src/http/pull_requests.rs:790-805` (the PR diff
  endpoint's fallback `base`)

**Attacker-controlled input:** the *current branch name* of a registered
checkout. `resolve_base_branch`
(`crates/ariadne-daemon/src/http/repositories.rs:193-210`) stores whatever
`git symbolic-ref --short HEAD` answers when a repository is registered
without an explicit branch, after only checking the branch *exists*
(`git.branch_exists`/`git.current_branch`) — never that its *name* is safe
to use as a bare CLI argument. The same lack of format validation applies on
every later edit (`update`, same file, lines 95-152). A second, narrower
path is `pull.head_branch`/`pull.base_branch`, the forge-reported PR
head/base ref names, used the same unguarded way at the two sites above.

**Transformation:** none. The branch-name string is interpolated into a
larger literal (`refs/heads/{branch}`, `{base}..{branch}`,
`{base}...{branch}`) or passed as a bare `&str` into a `Vec<&str>` argv for
`tokio::process::Command::new("git")` — never through a shell, so there is
no shell-metacharacter risk, but several of these argv positions are ones
git's own `parse-options` will still interpret as a flag when the string
starts with `-`.

**Reproduction — the end-to-end input is real, not hypothetical:**
```sh
d=$(mktemp -d); cd "$d"
git init -q -b main evil.git --bare
git init -q -b tmp seed && cd seed
git -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
git push -q "$d/evil.git" "HEAD:refs/heads/--upload-pack=exploit"
cd "$d/evil.git" && git symbolic-ref HEAD 'refs/heads/--upload-pack=exploit'
cd "$d" && git clone -q "$d/evil.git" clone1 && cd clone1
git branch --show-current   # => --upload-pack=exploit
```
A normal `git clone` of a repository whose default branch is named this way
reproduces a checkout whose *current branch itself* is the dash-prefixed
string — no non-standard tooling needed, only a `HEAD` symref set this way
on the origin side (any self-hosted git server, or anyone who can push
directly to a bare repo, can do this; GitHub/GitLab's own branch-name UI
validation is irrelevant since this bypasses it via `update-ref`, not
`git branch`). If the user then registers this checkout with Ariadne
without typing an explicit base branch, `resolve_base_branch` stores
`--upload-pack=exploit` as `repository.base_branch` verbatim.

**What happens at each sink (all tested against real git, with a harmless
marker payload; no destructive operation attempted):**

| Sink | Call shape | Result |
|---|---|---|
| `add_worktree`'s `base` (`-b branch wt base`) | standalone, after a prior positional (`wt`) | `error: opción … desconocida` — worktree creation fails (task cannot start in that repo) |
| `committed_clean`'s range (`rev-list --count base..branch`) | one token, option-shaped | `rev-list` has no matching option, prints its own usage and exits non-zero — gate check fails closed |
| `diff`'s preliminary `merge-base base branch` | standalone | `error: opción … desconocida`, exit 129 — `is_err()` is true, so `diff()` takes its `EMPTY_TREE` fallback and the *later*, genuinely dangerous `diff {base}...{branch}` line (which does accept `--output=<path>`, confirmed to silently write an attacker-chosen file — see counter-evidence below) is **never reached** for a dash-prefixed `base` |
| `delete_branch`'s `branch -D branch` | standalone | `error: switch desconocido 'o'` (git parses `-foo` as bundled short options `-f -o`) — branch deletion fails, nothing deleted (also checked: a crafted `-fother` does **not** accidentally force-delete the unrelated branch `other`) |
| `remote_has_branch_tip`'s `rev-parse branch` | standalone, first arg | `rev-parse` does not recognize `--output=…` as an option at all; it echoes the string back literally to stdout and exits 0 — no file written, the subsequent SHA comparison just fails to match |
| `remote_has_branch_tip`/`remote_branch_tip`'s `ls-remote --heads remote branch` | second positional, *after* `remote` | confirmed via `GIT_TRACE=1` that git invokes its normal `git-upload-pack <path>` regardless of the injected string — args *after* the first non-option positional are treated as literal ref-pattern filters here, not re-parsed as options |

**Counter-evidence / what limits severity:** the one sink that *would* have
been dangerous — `diff {base}...{branch}` accepting `--output=<path>` as a
single combined token (independently confirmed: `git diff
"--output=/tmp/x...branch"` silently creates `/tmp/x...branch` with no
terminal output) — is, in this codebase, always preceded by the
`merge-base base branch` call on the same two strings, and that call's own
option parser rejects the same dash-prefixed value before the vulnerable
line runs. This is not a deliberate guard (there is no comment or test
suggesting anyone noticed), but it is accidental defense in depth that
holds for every payload I tried. Every other sink's own limited option set
(`worktree add`, `rev-list`, `branch -D`, `rev-parse`, `ls-remote`) contains
nothing that writes a file, executes an external program, or targets the
wrong object; the concrete, reproducible impact today is **command/gate
failure for that repository** (the task's worktree, diff, push-gate, or
keeper-teardown check breaks), not file write or code execution.

**What I could not rule out:** I did not have a real SSH or `git://` remote
to test against. `git fetch`/`ls-remote`/`clone` *do* support an
`--upload-pack=<command>` option that, for the `ssh://` and local-spawn
transports, runs `<command>` as the "remote" program — this is the
mechanism behind several historical git-wrapper CVEs. I confirmed the
*reachable call shapes in this codebase* don't let that option land where
it would be honored (branch-name arguments always come after a `remote`
positional that flips git into literal-pattern mode, as shown above), but I
did not enumerate every git subcommand/version combination this could run
under, and a different installed git version could parse some of these
calls differently (e.g. older gits are sometimes more permissive about
where options can appear).

**Inconsistency that makes this look like an oversight, not a choice:**
`has_commit` (`gitwt.rs:250-260`) already defends the identical class of
input with `--end-of-options` before its revision argument, and
`branch_exists`/`branch_tip`/`fetch_head_of`'s refspec always prefix the
branch with the literal `refs/heads/` (which can never start with `-`
regardless of content). `add_worktree`, `delete_branch`,
`remote_has_branch_tip`, `remote_branch_tip`, and the `merge-base`/`diff`
pair do not follow either pattern.

**Severity:** Medium. Confirmed, reproducible, reachable with ordinary user
action (register an untrusted repo without naming a branch) and no special
privilege — but, after deliberate attempts against every reachable sink in
the installed git version, the demonstrated impact is denial of service for
the affected repository (failed worktrees, failed gates, failed keeper
teardown checks), not data exfiltration, file write, or code execution.

**Confidence:** High on the defect and its reachability; Medium on "no
version/transport of git ever turns this into something worse" (see what I
could not rule out, above).

**Recommended remediation (smallest fix, not applied — read-only audit):**
insert `--` (or, where the command supports it, `--end-of-options`) before
every bare branch/ref/sha argument in `gitwt.rs` that is not already
prefixed by a literal like `refs/heads/`: `add_worktree`'s `base`,
`add_worktree_at`'s `at`, `add_detached_worktree`'s `reference`,
`checkout_detached`'s `reference`, `delete_branch`'s `branch`,
`remote_has_branch_tip`/`remote_branch_tip`'s `branch` (both the
`rev-parse` and `ls-remote` calls), `diff`'s `merge-base`/`diff` calls, and
`committed_clean`'s range. This closes the gap uniformly regardless of
which specific option any given git version happens to support, rather than
relying on today's accidental guards.
**Regression criterion:** a repository whose base branch is a crafted
dash-prefixed name (reproducible exactly as shown above) can still have a
task worktree created, diffed, and gated normally against a same-shaped,
non-malicious branch name of equal length, and `git -C <repo> <subcommand>
-- <that name>` is observed in the process's argv for each guarded call.

---

#### EXEC-POS-1 through EXEC-POS-6 — Controls checked and found to already hold (reported so they are not re-discovered or redundantly "fixed")

1. **Task branch names cannot carry an injection-shaped prefix.**
   `crates/ariadne-store/src/tasks.rs:67-119` (`branch_name`/`slug`) reduces
   a task's title to `[a-z0-9-]+`, cannot begin with `-` (the slugger only
   ever starts the string with an alphanumeric character), and is the value
   used for `task.branch` everywhere in `gitwt.rs`. This is the one
   genuinely internally-generated "branch" argument in the whole surface,
   and it is safe.
2. **`has_commit` already defends itself.** `gitwt.rs:250-260` passes
   `--end-of-options` before the SHA/reference argument, which is exactly
   the fix recommended for EXEC-001's other sinks.
3. **PATH search for agent discovery rejects relative/empty entries and
   checks package containment.** `acp_index.rs:65-76` (`searchable`)
   filters `PATH` to absolute entries only, matching spec 007's acceptance
   criteria; `acp_index.rs:121-137` (`installs`) follows a resolved
   binary's symlink chain (bounded to 16 hops) and requires it to sit under
   the claimed package's own directories before trusting a name-based match
   — this defeats a same-named decoy binary placed earlier on `PATH`.
   Independently spot-checked by me: `acp_discovery.rs:486-488`
   (`command_of`) always returns the one absolute path discovery already
   resolved; nothing re-searches `PATH` by name at spawn time, so there is
   no discovery-to-launch TOCTOU window.
4. **ACP declares no filesystem or terminal capability, and implements no
   handler for either.** `acp.rs:2620`:
   `v1::ClientCapabilities::new().terminal(false)` with no `.fs(...)` call
   (so `read_text_file`/`write_text_file` default `false`); independently
   confirmed with `grep` that `acp.rs` registers exactly two inbound
   handlers (`session/update`, `session/request_permission`) and none for
   `fs/*` or `terminal/*`. A compliant agent cannot reach a file or spawn a
   terminal through the daemon's ACP side at all — there is no
   worktree-containment logic to defeat because there is no handler to
   call. (A trusted agent's *own* shell/tool access is unaffected and out
   of scope by design.)
5. **Repository-path resolution for a loose session cannot be confused by a
   sibling directory name.** `launcher.rs:300-306` picks the registered
   repository a loose session's cwd lies under with `Path::starts_with`
   (component-wise), not a string prefix — `/home/user/repo-evil` cannot be
   mistaken for `/home/user/repo`.
6. **The skills symlink is created and torn down safely.**
   `agents/mod.rs:96-104` (`link_skills`) canonicalizes the trusted,
   daemon-configured `skills_dir` before linking, never anything
   repository-controlled; `clear_skills_entry`
   (`agents/mod.rs:125-136`) checks `symlink_metadata().is_symlink()` and
   removes the link itself (`remove_file`), never following it with a
   recursive delete.

### Unverified candidates / cross-boundary evidence

#### EXEC-002 — `git fetch`'s remote argument is taken from forge data with no scheme/shape gate at the point of use

**Affected files/lines:** `launcher.rs:1456-1463` (`review_worktree`) passes
`pull.head_repo.clone().unwrap_or_else(|| … forge.remote …)` directly as
`from` into `self.git.fetch_head_of(&repo_path, &from, &pull.head_branch)`
with **no call to `forge::Remote::parse`** at all.
`launcher.rs:1546-1574` (`keeper_remote`) *does* call
`crate::forge::Remote::parse(head_repo)` (`forge/mod.rs:52-77`, which
allow-lists `https|http|ssh|git|git+ssh` schemes and rejects a `file://`/
local-path/`ext::`-shaped string), but on every path where that parse
fails — `repo.forge.is_none()`, or `Remote::parse` returns `None` — the
function still returns `head_repo.to_string()` unchanged rather than
refusing it (`launcher.rs:1554-1558`). The allow-list exists but does not
function as a gate in either caller.

**Why this is reported as cross-boundary rather than owned here:** the
*value* (`pull.head_repo`) is forge-sourced; per the cross-task contract,
FORGE owns validating incoming forge metadata. What EXEC owns is that
`gitwt.rs::fetch_head_of` (the sink) applies no restriction of its own
(no `GIT_ALLOW_PROTOCOL` env override, no scheme check) — it trusts
whatever string reaches it as `from`, the same way every other `git()` call
in that file does.

**Why I did not confirm exploitability:** `GitManager::git`
(`gitwt.rs:42-63`) sets no protocol allow-list, and I confirmed this git's
own default already refuses the `ext::` transport
(`fatal: transporte 'ext' no permitido`) regardless. GitHub's and GitLab's
own API fields that `head_repo` is built from
(`forge/github/pulls.rs:86-96`, `forge/gitlab/pulls.rs:80,221`, per the
sub-agent's reading, independently plausible from how both platforms
construct clone URLs from owner/repo names they themselves constrain) make
a `-`/`ext::`-shaped value implausible through normal use of either forge.
I did not test a non-GitHub/GitLab-compatible or misconfigured forge
endpoint, and did not test whether `file://` (always-allowed by git)
pointed at an attacker-chosen local path could be used as a confused-deputy
read (fetching refs/objects from a path the daemon's user can read but the
registered repository shouldn't see) — this remains open.

**Severity:** Low-to-Medium, contingent entirely on what FORGE's own
validation guarantees (or doesn't) for `head_repo`; EXEC's sink adds no
independent protection either way.

**Recommendation:** set `GIT_ALLOW_PROTOCOL=file:http:https:ssh:git` (or
narrower) on every `GitManager::git` invocation that takes a
forge-or-repository-sourced `from`/`remote` argument, as defense in depth
regardless of what FORGE validates upstream; and make `keeper_remote`
actually refuse (not pass through) a `head_repo` that fails
`Remote::parse`, so the allow-list it already computes is load-bearing.
This is a design question for the orchestrator to route to FORGE and EXEC
jointly, since the fix likely wants a change on both sides of the
boundary.

---

#### EXEC-003 — A resumed column-agent session can fall back to the bare repository checkout instead of its task worktree

**Affected file/lines:** `launcher.rs:898-970` (`resume_in_place`, shared by
`revive_session` and `relaunch_session`), specifically lines 917-945: when
`previous.worktree_path` is `None` or the directory no longer exists on
disk, and the session has a `task_id`, `cwd` falls back to
`PathBuf::from(self.store.get_repository(&task.repo_id).await?.path)` — the
repository's own primary checkout, not a per-task worktree. This is the
intended behavior for an **orchestrator** (spec 002 item 9 says the
orchestrator deliberately works in the primary checkout) but the same
branch of this `match` is taken whenever `task` is `Some(_)`, i.e. for a
column agent's session too.

**Why this matters:** every other path that puts a column agent into a
worktree (`task_worktree`, `launcher.rs:829-860`) recreates the worktree on
disk if it is missing before handing it to the agent. `resume_in_place`
does not call `task_worktree` — it only checks `path.is_dir()` and, failing
that, substitutes the bare repo path. If that substitution is actually
reachable for a non-terminal task whose worktree directory happens to be
gone (deleted out from under Ariadne, a disk issue, or any state the
"idempotent" `cleanup_task` comment at `launcher.rs:1317-1320` doesn't fully
anticipate), a column agent would be revived with its cwd at the shared
repository checkout rather than its own branch's isolated worktree — i.e. a
repository/task-identity mismatch between what was validated (this task,
this branch) and what is actually handed to the process.

**Why this is "unverified" rather than confirmed:** I read the code path
and traced it by hand but did not build a live repro (it requires getting a
non-terminal task's worktree directory removed by some other means, then
triggering a revive/relaunch on that exact session, which needs the full
daemon running rather than a disposable git-only reproduction). I also did
not verify whether some caller upstream of `resume_in_place` already
filters this case out for task-seated sessions before reaching it.

**Severity:** Medium if reachable (it would mean an agent edits/commits
against the wrong tree — the shared checkout other tasks and the
orchestrator also use — rather than its own task branch), Low likelihood
given it requires an already-unusual state (worktree missing while the task
is still active).

**Recommendation / design question for the orchestrator:** either make
`resume_in_place` call the same `task_worktree`-style
recreate-if-missing logic `start_step_agent`/`agent_switch` already use for
a task-seated session, or make the fallback explicit about *which* seats
are allowed to land on the bare repository path (orchestrator only) and
fail loudly for any other seat whose worktree is gone, rather than silently
substituting it.

---

#### EXEC-004 — A forge-controlled branch name is embedded verbatim into a literal "run this command" instruction handed to a trusted agent

**Affected file/lines:** `launcher.rs:1579-1585` (`keeper_push_command`)
builds `format!("git push {} HEAD:{}", remote, pull.head_branch)` and
`launcher.rs:1876-1897` (`keeper_briefing_of`) hands that exact string to
the keeper agent's briefing as "the command that pushes a fix to it."

**Why this is noted, not scored as a vulnerability:** Ariadne itself never
executes this string — it is natural-language briefing text, and the
keeper agent (an explicitly trusted, user-selected program) decides what to
actually run. This sits squarely in the excluded "contain a malicious
installed agent" territory for the *agent's* side of the interaction.

**Why it is worth a design question anyway:** Ariadne, not the agent,
*constructed* this exact command line out of forge-sourced
`pull.head_branch`, and presented it as the authoritative answer to "what
command pushes this fix" rather than describing the intent in prose and
letting the agent compose its own command. A forge-reported branch name
containing shell metacharacters (plausible for a non-GitHub/GitLab forge
with looser branch-name rules, or a compromised/misconfigured one) would
make Ariadne's own briefing text the thing that turns hostile metadata into
an executable-looking instruction, even though the agent remains the one
that chooses to run it. This is the same root cause as EXEC-001/EXEC-002
(nothing validates the *shape* of a forge-reported branch name before
Ariadne's own code treats it as safe to interpolate), reaching a different
kind of sink (prompt text instead of argv).

**Recommendation:** describe the push in prose ("push your branch to
`<remote>`'s copy of `<head branch>`") and let the agent build its own
command, rather than handing it a ready-made shell command line built from
forge content — or, if the ready-made form is kept, quote/validate
`pull.head_branch` the same way `is_sha` gates a commit hash at
`http/pull_requests.rs:756-758` before it is embedded.

### Rejected hypotheses

- **`git`'s `ext::` transport as an RCE vector through `fetch_head_of`'s
  `from` argument.** Tested directly: `git fetch 'ext::sh -c "touch
  <marker>"' <refspec>` against a disposable local repo fails outright
  (`fatal: transporte 'ext' no permitido`) under this git's default
  configuration, and nothing in `GitManager::git` changes that default.
  Rejected for this git version/configuration; not retested against every
  possible installed git build (noted as a residual gap in EXEC-002).
- **`git ls-remote --heads <remote> <branch>` as an injection point via a
  trailing `--upload-pack=<cmd>`-shaped `branch`.** Tested directly with
  `GIT_TRACE=1`: git invokes its normal `git-upload-pack <path>` regardless
  of the crafted trailing argument. Rejected — args after the first
  non-option positional (the remote) are treated as literal ref-pattern
  filters by this git, not re-parsed as options.
- **The `diff {base}...{branch}` sink in `channel.rs:454`/`steps.rs:75`
  being reachable with a dangerous payload.** Initially looked like a
  confirmed arbitrary-file-write (and `git diff --output=<path>` alone
  *does* silently write an attacker-chosen file, confirmed directly) — but
  rejected as reachable *through these call sites* specifically, because
  the preceding `merge-base base branch` call on the same input fails first
  and the code takes its safe fallback. See EXEC-001 for the full chain and
  why this matters as a latent rather than live defect.
- **A sibling directory name letting a loose session borrow another
  repository's permission mode.** `launcher.rs:300-306` uses
  `Path::starts_with`, confirmed component-wise, not string-prefix.
  Rejected.
- **The skills-folder symlink being poisoned by repository content.**
  `link_skills`'s `skills_dir` argument is always the daemon's own
  configured path, canonicalized before use; no repository or forge content
  reaches this call. Rejected as a repository-content vector (see
  EXEC-POS-6) — though see the adjacent, *not* repository-content-sourced
  path-traversal note below, included for completeness since it sits
  one call away from the mechanism this audit was asked to check.

### Hardening suggestions (not forge/repo-content vectors, flagged because they sit directly on the mechanism this task was asked to inspect)

- **Skill-name path traversal via the daemon's own HTTP API, not repository
  content.** The background sub-agent traced `agents/mod.rs`'s
  `sync_skills` (`dir = skills_dir.join(&skill.name)`, around lines
  181-210) and `agents/prompts.rs:110` (`dir.join(&skill.name).join(
  "SKILL.md")`) and found `skill.name` is never validated against `/`, `\`,
  or `..` anywhere in the stack (`http/skills.rs`, `ariadne-store/src/
  skills.rs::create_skill`, the DTO, the migration's schema comment). A name
  like `../../../etc` would write/read outside `skills_dir`. I independently
  confirmed the symlink-creation half is safe (EXEC-POS-6); I did not
  independently re-verify the `sync_skills` write path myself, so this
  carries the sub-agent's confidence level, not mine. **This is explicitly
  not a "hostile repository" finding** — skills are created through the
  daemon's own API, not scanned out of a repository or a forge response —
  but it is exactly the kind of symlink/path mechanism item 3 of this task
  ("path traversal, symlink handling, worktree containment") asks about, so
  it is recorded here rather than silently dropped, with that scope caveat
  explicit.
- **The `ariadne doctor` git-version floor (2.42, for orphan-worktree
  support) does not track git's own security-patch floor.** The May 2024
  git security release (patched versions were released per-minor-line, e.g.
  2.39.4/2.40.4/2.41.1-class and later) fixed several clone/checkout
  symlink- and submodule-hook-related CVEs. Ariadne's own `git worktree add`
  calls never recurse into submodules and never clone, so the most direct
  exploitation path for that CVE class does not obviously apply through
  Ariadne's own invocations — I did not find a concrete reachable path, so
  this is recorded as a coverage/hygiene note rather than a finding: a doctor
  check that also flagged a pre-patch git point release (not just the
  2.42 feature floor) would be cheap insurance, not a response to a proven
  gap.

## Recommendation

Fix EXEC-001 uniformly (insert `--`/`--end-of-options` at every bare
branch/ref/sha argument in `gitwt.rs` named above) as a single, mechanical,
low-risk change — it closes a real defect class even though today's git
behavior happens to make it non-exploitable, and "today's git behavior"
is not a property this codebase currently asserts or tests. Route
EXEC-002 and EXEC-004 to a joint EXEC/FORGE follow-up, since both need a
decision about where forge-sourced branch/URL validation should live
(FORGE's parsing, EXEC's sink, or both — I recommend both, per the
"recommended remediation" under EXEC-002, but that is the orchestrator's
call on which task owns the change). Ask the orchestrator whether EXEC-003
is worth a dedicated reproduction task before scheduling a fix, since I
could not confirm it is reachable in practice.

## Confidence

High on EXEC-001's mechanism and reachability, Medium on its severity
ceiling (bounded by what this one git version's option sets allow — a
different git build was not tested). High on EXEC-POS-1 through
EXEC-POS-6 and both rejected `ext::`/`ls-remote` hypotheses (each
independently reproduced). Medium on EXEC-002 (depends on FORGE guarantees
I did not independently verify beyond reading the sub-agent's citations).
Low-Medium on EXEC-003 (code-read only, no live reproduction). Low on the
skill-name hardening note (sub-agent's finding, not independently
re-verified by me).

## What would change this

- A concrete demonstration that some installed/available git version
  parses one of EXEC-001's sinks differently (e.g. accepts `--upload-pack`
  or `--output` in a position I found safe here) would raise EXEC-001 to
  Confirmed/High severity.
- Confirmation from FORGE about whether `pull.head_repo`/`head_branch` can
  ever carry a non-platform-constrained value (a non-GitHub/GitLab forge
  kind, or a parsing bug on FORGE's side) would raise or lower EXEC-002.
- A live reproduction of a non-terminal task losing its worktree directory
  while still being revivable would confirm EXEC-003.

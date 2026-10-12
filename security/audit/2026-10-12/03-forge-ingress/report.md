# FORGE — Forge ingress audit: webhooks, forge fetch, and forge content into agent prompts/actions

## Question

Can hostile forge traffic or content cause unauthorized local work, secret
exposure, or unintended forge actions?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (branch `main`, matching the
task's planning reference), inspected in the worktree at
`/Users/malvarez/.ariadne/worktrees/q4r9v3t3/k6n3kcgn-eng`. The worktree was
clean at the start and end of this audit; the only transient change was a
temporary, uncommitted unit test added and reverted for FORGE-001's
reproduction (see that finding).

## Threat assumptions (per the task's agreed scope)

- Installed coding-agent programs (`gh`, `glab`, the ACP agent) are trusted.
- Repository content, forge content (titles, bodies, comments, branch
  names, file paths, head-repo URLs), browser/network responses, and other
  users' files are potentially hostile.
- Documented permission guarantees (who may comment/review/resolve/push/
  merge) are checked even though the agents that exercise them are trusted.
- No requirement to contain a deliberately malicious *installed* agent
  process (that is a different audit's concern); the question here is
  whether hostile *forge-side* input reaches a sensitive operation.
- No attack was run against the user's live daemon, repositories, accounts
  or external services. Reproductions used isolated local code (a unit
  test run against pure in-process functions) and read-only checks of a
  locally installed `gh` binary and the RustSec advisory database.

## Coverage

### Reviewed in full
- `crates/ariadne-daemon/src/webhooks.rs`
- `crates/ariadne-daemon/src/forge/hooks.rs`, `forge/mod.rs` (incl. tests)
- `crates/ariadne-daemon/src/forge/pulls.rs`, `forge/live.rs`, `forge/news.rs`
- `crates/ariadne-daemon/src/forge/tunnel.rs` (via a dedicated sub-review,
  cross-checked against the spec)
- `crates/ariadne-daemon/src/forge/github/{comments,details,hooks,issues,open,pulls,reviews}.rs`
- `crates/ariadne-daemon/src/forge/gitlab/{comments,details,hooks,issues,open,pulls,reviews}.rs`
  (`reviews.rs` for both forges re-read directly by this report's author to
  verify a sub-review finding against primary sources; see FORGE-H1 below)
- `crates/ariadne-daemon/src/agents/prompts.rs` (incl. tests)
- `crates/ariadne-daemon/src/http/pull_requests.rs`, `http/channel.rs`,
  `http/caller.rs`, `http/forge.rs`
- `crates/ariadne-daemon/src/gitwt.rs`
- `crates/ariadne-store/src/webhooks.rs`, `src/forge.rs`,
  `src/pull_request_comments.rs`
- `crates/ariadne-store/skills/pr-babysit/SKILL.md`,
  `skills/pr-reviewer/SKILL.md`
- `specs/025-forge-integration.md` through `specs/029-reviewing-a-request.md`,
  `specs/032-babysitting-a-request-by-hand.md`

### Reviewed in part
- `crates/ariadne-daemon/src/launcher.rs` (lines ~1420–1650: review/keeper
  worktree, keeper remote/push-command construction; the rest of the file —
  task worktree spawning, ACP session launch — is EXEC's territory)
- `crates/ariadne-daemon/src/http/repositories.rs` (the `forge_asked` enable/
  disable path only; registration/branch-detection plumbing not forge-trust
  relevant)
- `crates/ariadne-daemon/src/config.rs` (webhook/tunnel keys only)
- `crates/ariadne-store/src/defaults.rs` (the four seat system-prompt
  constants only)

### Excluded, with reason
- `ui/src/*` — displayed forge content is VIEW's assigned territory per the
  cross-task contract.
- EXEC's worktree-execution internals (`branch.rs`, `checkpoint.rs`, agent
  process launch/sandboxing) — this report traces forge content up to the
  point it becomes a prompt or a forge-facing API call, not what a
  general-purpose coding agent can do once it decides to run a shell
  command; that boundary is EXEC's to own (see "Cross-boundary evidence"
  below).
- `specs/030-workflows.md`, `specs/031-attention-and-notifications.md` —
  read only as citations from 026/029/032; their own gate/attention logic
  consumes already-computed `pull_requests` row state and carries no
  forge-trust decision of its own.
- `crates/ariadne-store/src/pull_requests.rs` (the plain row CRUD) — the
  specific writes it exposes (`set_pull_request_ready`,
  `set_pull_request_reviewed`, `set_pull_request_summary`,
  `upsert_pull_request`) are all exercised, and their call-site validation
  reviewed, through `http/pull_requests.rs` and `forge/pulls.rs`; the SQL
  layer itself adds no additional trust boundary.
- The `glab` binary — not installed in this sandbox (only `gh` 2.97.0 was
  available). One claim below (FORGE-H1, rejected) was verified against
  `gh api --help` as a primary source and extended to `glab` only by
  documented design parity, not by running `glab` itself. Recorded as a
  coverage gap, not folded into that item's confidence.
- No live daemon + stub-forge + stub-tunnel integration run (e.g.
  `cargo nextest run -p ariadne-daemon -E 'test(/^webhooks::/)'`) was
  executed; the existing acceptance criteria in specs/027 already name the
  tests that exercise this end-to-end, and static reading plus one isolated
  unit-level reproduction (FORGE-001) was judged sufficient within the time
  and no-app-changes constraints of this task.

## Tracer walkthrough: what is actually trusted, and where

`Webhook or forge response -> verification and fetch -> stored request or
issue -> agent context -> requested forge action`

1. **Webhook delivery is a trigger, never a data source.** The listener
   (`webhooks.rs:89-154`) verifies GitHub's `X-Hub-Signature-256`
   (HMAC-SHA256 over the raw body, `hmac`/`subtle` constant-time compare,
   `webhooks.rs:120-131`) or GitLab's `X-Gitlab-Token`
   (`ct_eq`, `webhooks.rs:133-136`) before anything else happens, and on
   success it only calls `ForgePoll::wake(id)` (`webhooks.rs:151`) — it never
   parses or trusts the delivery body's *content*. The actual pull request/
   comment data that later reaches an agent always comes from a fresh,
   separately-authenticated `gh`/`glab` API call (`forge/poll.rs` via
   `ForgeClient::details`), over the user's own forge credentials. This
   means an attacker who does not know the per-integration secret cannot
   inject *any* fabricated forge content, however they craft the webhook
   body — confirmed by code reading, matching spec 027 rule 4 ("The
   delivery triggers one fetch instead of applying its payload").
2. A **replayed, previously-valid** signed delivery (e.g. captured in
   transit before TLS, or from a log) is accepted again, but its only
   effect is one more coalesced `wake()` (`ForgePoll` dedupes concurrent
   wakes per spec 026 rule 6) — harmless by design, matching spec 027 rule 4
   ("A missed delivery costs nothing more than the next one"). No freshness/
   nonce check exists, and none is needed given this effect.
3. The listener binds a **separate port** from the main API
   (`webhooks.rs:60-72`, loopback-only unless the operator sets
   `webhook_listen` explicitly in `config.toml`) and serves exactly the two
   signed POST routes (confirmed: `Router::new().route(...).route(...)`
   with no further routes, no middleware beyond the 1 MiB body cap).
4. **Disabled integrations and changed forge identity** are both covered
   correctly: `deliver()` requires `row.enabled && row.kind == kind`
   (`webhooks.rs:112-116`) so a disabled integration's deliveries get 401;
   and `write_forge`'s `ON CONFLICT` clause
   (`crates/ariadne-store/src/forge.rs:66-84`) clears `webhook_id`,
   `webhook_secret`, `webhook_url`, resets `webhook_state` to `polling`, and
   clears `webhook_error`/`webhook_last_delivery_at` atomically the instant
   `kind`/`host`/`owner`/`name` change — so a replaced remote's old secret
   can never authorize a delivery against the new identity, and
   `webhook_delivered`'s own `AND webhook_secret = ?` guard
   (`crates/ariadne-store/src/webhooks.rs:85-92`) is redundant-but-correct
   belt-and-suspenders on top of that. This holds identically for GitHub and
   GitLab, since the SQL and the listener dispatch are forge-agnostic; only
   the HTTP calls GitHub/GitLab hook reconciliation makes differ
   (`forge/github/hooks.rs`, `forge/gitlab/hooks.rs`), and neither builds a
   request from anything but static event-name constants and the
   already-redacted secret (confirmed by sub-review; see "Rejected
   hypotheses").
5. The fetch's **comments, titles, bodies, and branch metadata** are merged
   into `forge::live::LivePulls` (`forge/live.rs`) with no sanitization
   beyond stripping Ariadne's own HTML-comment signature markers
   (`forge/pulls.rs::signature`/`unsigned`), and from there flow verbatim
   into agent prompts (`agents/prompts.rs::pull_request_briefing`,
   `pull_request_news`, `pull_request_keeper_briefing` — `pull.title`,
   comment bodies truncated to 200 chars via `news.rs::first_line`, branch
   names, etc., are all interpolated as plain text with no escaping,
   because they are natural-language prompt content, not code or markup
   context). This is expected and, by itself, not a finding — see FORGE-001
   for where the *consequence* of this (unfiltered authorship) becomes a
   control gap, and "Rejected hypotheses" for why this does **not** also
   enable CLI argument/shell/GraphQL/JSON injection.
6. The **sensitive forge actions** an agent can request are narrow and
   enforced at the HTTP layer, not by the agent's own judgment or its
   skill's prose:
   - No route approves anything (`http/pull_requests.rs::submit_review`
     line 828-836 refuses any `event` but `request_changes`/`comment` with
     400, before the forge ever sees a call) and no route merges, on either
     forge (confirmed: no merge/approve call exists anywhere in
     `forge/github/*`, `forge/gitlab/*`).
   - A conversation reply answers, once, a comment of the other side, from
     the request's own keeping agent or keeper seat alone
     (`http/pull_requests.rs::reply`, lines 540-552); a reviewer session
     resolves only a thread it itself opened (`resolve`, lines 644-653).
   - `report_pull_request`'s `ready` claim is read from the *correct* seat
     only (`Seat::Agent` for a task-kept row, `Seat::Keeper` for a
     hand-kept one, never a co-existing reviewer session on the same row —
     `report`, lines 697-707) and never itself notifies the user; a
     dedicated, forge-evidence-confirmed attention producer (031, out of
     this report's scope) is the only thing that ever tells the human a
     request is actually ready.
   - Every route that posts on the user's behalf strips an agent's trailer
     (`Co-Authored-By`, `Generated with…`, `Signed-off-by`) first
     (`forge/pulls.rs::strip_trailers`, used at `http/pull_requests.rs:519,
     849, 866` and `http/channel.rs:117`).
   This combination means that even a fully successful prompt injection
   inside a comment or issue body **cannot** make an agent approve or merge
   a request, post as a different identity, resolve someone else's thread,
   or report readiness through the wrong seat: those are structural,
   code-level refusals independent of what the LLM decides to attempt, and
   are reinforced a second time in the `pr-reviewer`/`pr-babysit` skill
   prose itself ("Never give an approval. The user gives it", "Never merge
   the request"). This is the strongest piece of **confirmed-safe**
   evidence in this audit, and it holds identically for GitHub and GitLab
   since the refusals live in the shared HTTP layer, not per-forge code.

## Findings

### FORGE-001 — Comments/replies that drive agent-authored commits carry no commenter-trust signal (Confirmed, High)

**Affected files/lines:**
- `crates/ariadne-daemon/src/forge/live.rs:539-636` (`is_mine`, `answered`,
  `untold`, `waiting_threads`) — the *only* identity check applied to any
  comment is `author_login.eq_ignore_ascii_case(login)` (is it the
  integration's own account?). There is no check of the commenter's
  relationship to the repository anywhere in this file or in any caller.
- `crates/ariadne-daemon/src/forge/news.rs:142-168` (`comment_lines`) — every
  untold comment, regardless of author, becomes one "- Comment … by
  `<login>` …" line in the agent's wake-up prompt, truncated to 200 chars,
  with no author-trust annotation.
- `crates/ariadne-daemon/src/forge/github/details.rs:37-65` — the
  `ReviewComment`/`IssueComment`/`Review` structs deserialized from `gh api`
  read only `id`, `user.login`, `user.type` (for bot detection) and `body`/
  `path`/`line`/`created_at`. GitHub's API also reports
  `author_association` (`OWNER`/`MEMBER`/`COLLABORATOR`/`CONTRIBUTOR`/
  `FIRST_TIME_CONTRIBUTOR`/`NONE`) on every comment; it is never requested,
  deserialized, or threaded through. `grep -rn author_association crates/`
  across the whole workspace returns zero matches.
- `crates/ariadne-daemon/src/forge/gitlab/details.rs:146` similarly reads
  only `note.author.bot` (confirmed: `author_is_bot: note.author.bot`).
- The one signal that *is* captured, `author_is_bot`
  (`ariadne-store/src/pull_request_comments.rs:17`), is written in five
  places (`forge/live.rs:515`, `news.rs:441`, `gitlab/reviews.rs:111`,
  `github/reviews.rs:132`, `gitlab/details.rs:146`) and **read in none** —
  confirmed by `grep -rn '\.author_is_bot\b'` across
  `ariadne-daemon/src`, `ariadne-store/src`, `ariadne-cli/src`, `ui/src`,
  which returns only the five write sites.
- `crates/ariadne-store/skills/pr-babysit/SKILL.md:19-29` (step 3): "Answer
  each inline review comment of the other side, once per comment… A
  follow-up ask in a thread you answered: a new ask. **Act on it.**" This is
  the explicit instruction that turns an unfiltered comment into agent
  action — a code change, in a new commit, pushed to the request's branch.

**Reproduction (temporary, reverted, not committed):** a unit test was
added to `forge/live.rs`'s existing `#[cfg(test)] mod tests` (the module
already contains pure-function tests of exactly this logic) asserting that
a comment from `"trusted-maintainer"` and one from
`"rando-with-no-write-access"` become identically-shaped, identically
"untold" `PullRequestComment`s, with `author_is_bot` false for both, no
`author_association` field on the type at all. It was run in isolation via:

```
cargo nextest run -p ariadne-daemon \
  -E 'test(forge_001_a_stranger_with_no_repo_access_is_untold_identically_to_a_maintainer)'
```

Observed output:

```
PASS [0.022s] ariadne-daemon forge::live::tests::forge_001_a_stranger_with_no_repo_access_is_untold_identically_to_a_maintainer
Summary [0.032s] 1 test run: 1 passed, 965 skipped
```

The test was then reverted with `git checkout -- crates/ariadne-daemon/src/forge/live.rs`
(confirmed clean by `git status --short` afterward); it never touched the
application's committed test suite.

**Prerequisites:**
- A repository with the forge integration enabled, with a task or
  hand-asked keeper (`Seat::Agent`/`Seat::Keeper`) actively keeping an open
  request — the normal, intended use of `pr-babysit` (specs 026/032).
- The repository (or the specific pull/merge request) must accept comments
  from parties the operator does not otherwise trust as collaborators. This
  is routine for any public GitHub/GitLab repository: GitHub allows any
  signed-in user to leave review comments and issue comments on a public
  repository's pull request regardless of `author_association`, and GitLab
  likewise allows any user with at least Guest/Reporter-level comment
  access (frequently open on public projects) to comment on a merge
  request.
- No prompt-injection "jailbreak" is required for the base case: the
  pr-babysit skill's own documented behavior is to act on "a follow-up ask
  in a thread you answered," which is exactly what an untrusted commenter's
  reply is instructed to produce. A more adversarial version of the same
  gap could additionally embed instructions designed to steer the agent's
  interpretation of "the ask," but that is not required to demonstrate the
  missing authorization boundary.

**Impact:** on a public (or otherwise comment-open) repository, any
stranger who can leave a PR/MR comment can get the keeping/babysitting
agent — which has real commit/push capability in the task's own worktree —
to treat their comment as legitimate reviewer feedback and act on it (new
commits, pushes to the request's branch, replies posted in the project's
name). This is "unauthorized local work" caused by hostile forge content,
the exact failure mode named in the assigned question. It is bounded by two
mitigating controls already in place (§Tracer point 6): no route this
content can reach ever merges or approves, so a human still reviews the
diff before it lands; and the `reviewer` seat (read-only worktree, no
commit/push tool at all) is unaffected — the gap is specific to the
`Agent`/`Keeper` seats, which already carry full coding-agent capability
over the branch as part of their normal job.

**Counter-evidence considered:** one could argue this is simply "prompts
can be influenced by hostile text," which the task asks to distinguish from
a demonstrated control failure. The distinguishing fact here is narrower
and more concrete: the system already fetches a trust-relevant signal
(`author_is_bot`) from both forges and stores it on every comment, but
never consults it, and never requests the richer signal
(`author_association`/project access level) GitHub's and GitLab's own APIs
expose for exactly this purpose. The gap is a specific, nameable missing
filter in `forge/live.rs`'s identity logic, not a general claim about LLM
steerability — and the skill text ("Act on it") is a second-order
amplifier on top of that gap, not the root cause.

**Recommendation (smallest remediation):** before a comment or reply
reaches `untold`/`comment_lines` (i.e. before it can become agent-visible
"news" that the pr-babysit/pr skill is told to act on), read
`author_association` (GitHub: already present on every `issues/comments`,
`pulls/comments`, and `reviews` payload `gh api` fetches; GitLab: derive
from the note author's project `access_level` via one additional `members`
lookup, or from `note.author` membership) and thread an `author_trusted:
bool` (or a small enum of levels) onto `PullRequestComment`. Two
compatibly-scoped options, left for the user to decide:
(a) withhold "Act on it"-type instructions for an untrusted author's
comment and surface it to the keeping agent as "unanswered, but from an
untrusted account — do not act without the user's own direction," while
still showing it as news (keeps visibility, removes the auto-action path);
or (b) only change the `pr-babysit`/`pr` skill text to require an explicit
human confirmation step (e.g. via `wait_for_user`) before applying a
suggestion or committing a fix sourced from an untrusted-author's comment.
**Regression criterion:** a unit test on `forge/live.rs` asserting that a
`PullRequestComment` built from a `NONE`/no-access author is tagged
untrusted, and a `pr-babysit`/`pr` workflow test (in the style of
`defaults.rs::tests::the_pr_babysit_skill_answers_each_comment_once_in_its_shape`)
asserting the skill does not commit code sourced from an untrusted
comment's suggestion without the added confirmation step.
**Scope/exclusions:** this is a daemon + skill-text change
(`ariadne-daemon`, `ariadne-store/skills`), not a UI change; it does not
need to touch `reviewer`-seat logic, which already has no write capability
and is unaffected.

### FORGE-002 — A hostile issue's title/body reaches the orchestrator's briefing verbatim when a human imports it (Unverified candidate / hardening suggestion, Low)

**Affected files/lines:** `crates/ariadne-daemon/src/agents/prompts.rs:136-169`
(`orchestrator_briefing` interpolates `goal.description`, which
`ariadne goal create --from-issue <url>` sets verbatim from the forge
issue's body per specs/028 rule 4/5) feeding straight into the
orchestrator's first prompt, which plans and stages every subsequent task.

**Why this is a candidate, not confirmed:** turning an issue into a goal is
an explicit, human-initiated action (`ariadne goal create --from-issue` or
the desktop's "Create goal on an issue" dialog, specs/028 rule 9) — the
human sees the issue's title/body in the dialog before confirming, which is
a real (if easy-to-skim) point of human review, unlike FORGE-001's fully
automatic path. The orchestrator's resulting actions also still pass
through the ordinary develop/review/merge workflow and the same
no-merge/no-approve structural refusals described in the Tracer section.

**Why it's still worth recording:** a long or formatted issue body is
exactly the kind of content a human glances at rather than reads
critically for embedded instructions ("indirect prompt injection via
content a human nominally approved"), and the orchestrator is a
high-privilege seat (it can direct task creation, staffing, and which
skills/models run). This report did not find or attempt a concrete
jailbreak scenario — no code path lets the orchestrator bypass the same
structural refusals already covering the rest of the system — so this is
recorded as a hardening suggestion rather than a confirmed vulnerability.

**Recommendation:** none required for this task; if the user wants
follow-up, the smallest change would be visually fencing the quoted
issue body in the "Create goal" dialog (VIEW's territory) as foreign/quoted
text, distinct from the human's own title/description fields — a UI
affordance, not a daemon change, so flagged to the orchestrator rather than
scoped here.

### FORGE-H1 (hardening observation, not a defect) — Localtunnel subdomain entropy

Reported by the tunnel sub-review (file:line citations in that review):
the random subdomain (`forge/tunnel.rs` word+6-digit scheme) has roughly
2^24 combinations. On a public tunnel provider with first-come-first-served
subdomain claiming, this is small enough that an attacker could plausibly
pre-register the name an instance will later request, racing it for the
webhook's effective public hostname before first use. This does **not**
bypass the webhook's own HMAC/token authentication (an attacker squatting
the subdomain still cannot forge a valid signature), so the worst case is
denial of the tunnel's own registration (the real instance gets a
different URL, visibly failing to go `live`), not unauthorized access.
Recorded as an open design question for the orchestrator, not a finding
requiring remediation in this task.

## Rejected hypotheses (with reasoning)

- **GitLab review-comment `-f position[new_path]=<attacker-chosen
  filename>` / `-f position[old_path]=…` triggers `glab`'s `@file`
  file-read magic value, exfiltrating an arbitrary local file into the MR
  discussion.** (`crates/ariadne-daemon/src/forge/gitlab/reviews.rs:83-96`,
  `130-178`.) This was a sub-review's initial finding; I verified it
  directly against the installed `gh` 2.97.0's own `--help` text (a primary
  source, since `glab`'s `api` command is explicitly modeled on `gh api`'s
  flag conventions and neither binary nor its docs were locally available
  to check directly — see "Excluded" above): **the `@filename`
  magic-value behavior is documented exclusively for `-F`/`--field`**
  ("The `-F/--field` flag has magic type conversion… if the value starts
  with `@`, the rest of the value is interpreted as a filename to read the
  value from"); `-f`/`--raw-field` is documented only as "Add a string
  parameter in key=value format," with no such behavior. Every
  attacker-influenced string in these calls (`comment.path`/`old_path`/
  `body`) is passed with `-f` (`gitlab/reviews.rs:83,85,89,91,93,95,158,177`
  and `gitlab/comments.rs:41`, `github/comments.rs:47`,
  `github/reviews.rs:160`); the one `-F` usage in this code
  (`gitlab/reviews.rs:97-98`, `position[new_line]`) carries only
  `comment.line`, an `i64` formatted with `{}`, which can never start with
  `@`. I grepped every `-F` usage across both forge clients
  (`github/details.rs:280-285`, `github/hooks.rs:34`, `gitlab/hooks.rs:37-41`,
  `gitlab/pulls.rs:126-127,199-200`, `gitlab/reviews.rs:97-98`) and confirmed
  none carries attacker-influenced string content — the code's choice of
  `-f` for every string field is the *correct*, safe convention per `gh`'s
  own documentation, not an oversight. **Rejected**, with one residual
  caveat recorded above: not independently confirmed against the actual
  `glab` binary, since it was not installed in this sandbox.
- **Git refspec injection via a malicious PR head branch name** (e.g.
  `main:refs/heads/main` used to redirect `git fetch` away from
  `FETCH_HEAD` and overwrite an arbitrary local ref) —
  `crates/ariadne-daemon/src/gitwt.rs::fetch_head_of:265-269` formats
  `refs/heads/{branch}` as a single refspec argument. Git's own
  `check-ref-format` rules forbid a colon in any ref name, so no branch a
  forge would ever let a user create can contain the `:` a two-part refspec
  needs; the forge itself is the enforcement point, and it cannot be
  bypassed from the PR side. **Rejected.**
- **SSRF via a fork's `head_repo` URL rewritten for the keeper's push
  target** — `crates/ariadne-daemon/src/forge/mod.rs::Remote::rewrite:97-119`
  and `launcher.rs::keeper_remote:1546-1574`. The scheme/authority (and
  thus host, and thus which credentials apply) always comes from the
  already-configured, trusted remote's own push URL; only the owner/name
  path segments are swapped in from the fork's `Remote`, which is itself
  constrained to the same forge host by `PullRequestRef::parse`'s
  host-matching check upstream. No path can retarget the daemon's
  authenticated git transport at an attacker-chosen host. **Rejected.**
- **Tunnel relay as an SSRF/route-bypass primitive** reaching the main API
  port instead of the webhook listener, or reaching the loopback relay from
  a non-loopback address — covered by a dedicated sub-review with file:line
  citations (`forge/tunnel.rs:443-473`, `webhooks.rs:60-72`); the relay
  binds `127.0.0.1` only and forwards exclusively to the fixed
  `SocketAddr` captured once from `WebhookListen::address()`, never
  re-read from config, and never wired to the main API's listener.
  **Rejected** (confirmed-safe).
- **CLI argument injection via a forge-controlled string starting with
  `-`** (title, branch, path, login) being reinterpreted as a flag by
  `gh`/`glab` — covered by sub-review with file:line citations across all
  14 forge-client files; every such value is always the fixed argv slot
  immediately following its named flag (`--title`, `--body`, etc.) or is a
  numeric id formatted from an `i64`/validated hex-sha before
  interpolation, and `Cli::run`/`call_with_input`
  (`forge/mod.rs:627-662,673-716`) never goes through a shell. **Rejected.**
- **GraphQL/JSON injection via forge-controlled comment bodies or paths** —
  `github/comments.rs:103-114` and `github/details.rs`'s GraphQL calls pass
  `owner`/`name`/`number` as separate `-F` *variables*, never interpolated
  into query text; `github/reviews.rs:70-96` builds review bodies with
  `serde_json::json!` through `--input -`, never manual string formatting.
  **Rejected** (confirmed-safe).

## Dependency advisories

`cargo audit` (RustSec advisory DB, 1307 advisories, fetched live) against
the full workspace `Cargo.lock` (557 crates) at the examined commit found
**0 vulnerabilities**. Two unrelated warnings: `bincode` 1.3.3 unmaintained
(RUSTSEC-2025-0141, pulled in by `syntect`, the console's markdown/code
highlighter — unrelated to forge/webhook ingestion) and `chacha20` 0.10.1
yanked (pulled in via `rand` → `ulid`, used for session/task ID generation,
not for the webhook secret, which uses `getrandom::fill` directly per
`forge/hooks.rs:121-123`). Neither is reachable through the webhook/forge/
tunnel code path (`hmac`, `sha2`, `subtle`, `axum`, `reqwest`,
`localtunnel-client` and their transitive dependencies all came back clean).

## Confidence and recommendation

**High confidence** in the confirmed-safe architecture described in the
Tracer section (webhook delivery carries no trusted payload; no
approve/merge route exists on either forge; replies/resolves/ready-reports
are seat- and ownership-scoped; changed forge identity and disabled
integrations both correctly invalidate stale secrets) — this is backed by
direct code reading with file:line citations, cross-checked by an
independent sub-review for the tunnel/relay surface, and is consistent with
the acceptance-criteria tests already named in specs/026/027/029/032.

**High confidence** in FORGE-001 as a real, demonstrated gap: it is backed
by a passing isolated reproduction, an exhaustive grep showing the one
captured trust signal (`author_is_bot`) is never read, and a textual
citation of the skill instruction that turns the gap into agent action.

**Lower confidence, flagged explicitly:** the `-f`/`-F` rejection (the
single most severe-looking candidate this audit encountered) rests on
`gh`'s own documented behavior plus design-parity reasoning for `glab`,
not on running `glab` itself. **What would change this conclusion:**
installing `glab` and confirming its `api --help` text matches `gh`'s
`-f`/`-F` distinction (expected), or finding any GitLab-specific
documentation suggesting `-f` also honors `@file` (not found in this
audit). I recommend the user or a follow-up task verify this once `glab`
is available, before fully closing out this line of inquiry — it is cheap
to check and the consequence of being wrong (arbitrary local file
exfiltration via a crafted PR file name) would be severe.

**What would change my overall recommendation:** evidence that a public
repository is a realistic deployment target for the `pr`/`keeper` seats in
practice (if Ariadne is only ever pointed at private, collaborator-only
repositories by convention, FORGE-001's severity drops substantially, since
every commenter would already be a trusted collaborator) — this is a
product/usage question, not something this audit can settle from the code
alone, and is one of the "compatibility decisions" below.

## Cross-boundary evidence for the orchestrator

- FORGE-001's root cause (no commenter-authorization filtering) is entirely
  within FORGE's own ingestion layer (`forge/live.rs`, `forge/news.rs`,
  the GitHub/GitLab detail readers); FORGE owns this. The *consequence* —
  an agent with shell/commit access inside a worktree acting on the
  resulting instruction — is EXEC's territory (worktree execution
  capability). I am not proposing any change to EXEC's sandboxing; the
  remediation proposed here is entirely upstream of that boundary (don't
  hand the agent the instruction as actionable in the first place, or
  require confirmation first).
- VIEW owns how forge content (including an untrusted stranger's comment,
  or an imported issue body) is *displayed*; FORGE-002's suggested
  mitigation (visually fencing quoted issue text in the goal-creation
  dialog) is a VIEW-side change if the user wants to pursue it.

## Compatibility decisions for the user

1. Is FORGE-001 an accepted risk for private/collaborator-only repositories
   (where every commenter is already trusted), with remediation scoped
   only to public repositories or repositories where Ariadne cannot
   determine commenter trust level? Or should the author-trust filter and
   confirmation gate apply universally, accepting the extra `gh api`/
   `glab api` round-trip (membership/association lookup) on every fetch?
2. For FORGE-001's option (a) vs (b) (above): should an untrusted comment
   still reach the agent as visible news with a "do not act without
   confirmation" annotation, or should it be withheld from the agent
   entirely until a human re-surfaces it? The former preserves
   transparency; the latter is more conservative but could hide legitimate
   first-time-contributor feedback from the agent.
3. Should a follow-up task install `glab` locally and empirically confirm
   or refute the `-f`/`-F` rejection above before it is treated as fully
   closed?

## Proposed follow-up tasks

- A remediation task scoped to `ariadne-daemon`/`ariadne-store` implementing
  FORGE-001's chosen option, once the user answers compatibility question 1
  and 2 above, with the regression tests named in that finding.
- (Optional, low priority) A VIEW-scoped task to visually fence quoted
  issue content in the "Create goal from issue" dialog, addressing
  FORGE-002.
- (Optional) A short verification task to install `glab` and confirm the
  `-f`/`-F` primary-source finding against the real binary, closing the one
  coverage gap in this report's otherwise-rejected highest-severity
  candidate.
- No application fix is included in this change; this report is evidence
  and recommendation only, per the task's constraints.

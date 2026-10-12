# FORGE — Forge ingress audit: webhooks, forge fetch, and forge content into agent prompts/actions

## Revision note

This is a revised report after a review verdict of "changes required" on commit
`31bc279647d317ad56ccc550e2e510a262e1470d`. That review found one factual
error (a protection I attributed to the wrong function), several claims
that were broader than the code actually supports, an incomplete evidence
inventory, and one reproduction too shallow to support its own severity
rating. Every must-fix item was addressed in the next revision: findings
were renumbered, rescoped, and in two cases newly discovered following the
review's own leads.

**Second revision note.** The orchestrator relayed evidence from two
parallel audit tasks working the same commit: EXEC (Git worktree
execution) and PERM (permission policy). EXEC independently reproduced,
in its own isolated harness, that a detached reviewer worktree still
accepts a commit and an explicit-refspec push, and separately found that a
branch name containing shell metacharacters survives into a generated
command shape. That revision cited EXEC's execution-capability evidence
by attribution and traced, as FORGE's own ingestion question, where that
command shape is built and whether forge content reaches it (finding
FORGE-007, below). The division of ownership adopted from there on is
three-way: **FORGE owns ingestion and influence; EXEC owns execution
capability; PERM owns permission policy.**

**Third revision note.** This review verdict found five defects, two of
them the most consequential of either prior round:

1. **An unsupported conclusion, corrected.** FORGE-007 had claimed the
   branch-name hazard was "not externally attacker-reachable" because the
   keeper seat is gated to `role == "author"` requests. The review cited
   an existing, maintained test —
   `a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository` — that
   is direct counter-evidence: it sets `author: "me"` (satisfying the role
   gate) while `head_repository` is `alice/widgets`, a different account's
   repository entirely. `role == "author"` establishes who *opened* the
   request; it does not establish who *named the branch* living in the
   head repository, which GitHub's and GitLab's own documented permission
   models allow to be a different party under ordinary, supported
   collaboration patterns. The conclusion below is corrected to an
   explicit unresolved prerequisite, not a confirmed exploit and not a
   rejection.
2. **A broken reproduction, fixed.** FORGE-007's published command sequence
   omitted the initial commit `git branch` needs; replayed verbatim, it
   fails with `fatal: not a valid object name: 'main'`. The sequence below
   is the one actually run, with the commit step present, re-verified
   before being published this time.
3. **Tunnel payload confidentiality, completed.** The previous revisions
   covered registration TLS and stale-hook secret reuse but never traced
   the forwarded tunnel connection's own transport. This revision reads
   the vendored `localtunnel-client` crate's actual forwarding code
   (`src/lib.rs:319-375`) directly and finds it carries the full webhook
   payload — on both forges — over a bare, unencrypted `TcpStream`, not
   merely a stale or reused one. This is folded into an expanded
   FORGE-005.
4. **Reproduction-isolation instructions, corrected.** `repro/README.md`
   instructed applying a patch to the task's own worktree and reverting
   it. The task requires a disposable copy. This revision redid both
   preserved reproductions inside an actual disposable local clone of the
   examined commit (`/tmp/forge-repro-a1728348`, deleted after use, never
   part of this worktree) and rewrote the instructions to match exactly
   what was done.
5. **Finding-identifier stability, restored.** The previous revision had
   reassigned "FORGE-002" from its original meaning (hostile issue
   import) to a new finding (the permission-mode gap), pushing the
   original issue-import finding to "FORGE-006." This breaks the
   contract that an identifier's meaning does not change across
   revisions. **FORGE-002 is restored to the issue-import finding below.
   The permission-mode finding is reassigned to FORGE-008**, a number no
   prior revision used. Every cross-reference in this document — the
   Tracer, the Findings headers, Confidence, Cross-boundary,
   Compatibility, and Proposed follow-up sections — was checked and
   updated to this renumbering; none of the other findings' numbers moved.

Two smaller corrections, also from this review, are folded into their
findings directly: FORGE-004's recommendation and compatibility question
had incorrectly credited `keeper_remote` with falling back to "the
configured remote" on a parse failure — it falls back to the raw,
unvalidated `head_repo` string, the same unsafe behavior FORGE-004 is
about, not a protection. FORGE-003's claimed consequence — that the
`pr-babysit` skill "treats Ariadne findings with elevated trust" and
"tracks and resolves" them — is retracted: the skill's own text treats
every comment identically regardless of its apparent source, and its own
Rules section says "Leave every thread open," the opposite of resolving
anything. The reproduced defect (the false attribution itself) stands;
the overclaimed behavioral consequence does not.

## Question

Can hostile forge traffic or content cause unauthorized local work, secret
exposure, or unintended forge actions?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (branch `main`). The task
worktree itself (`/Users/malvarez/.ariadne/worktrees/q4r9v3t3/k6n3kcgn-eng`)
was clean at the start and end of this revision; no application file in
it was ever touched. Every reproduction needing to run application code
against the examined commit ran inside a disposable local clone created
for that purpose and deleted immediately after
(`/tmp/forge-repro-a1728348`, created via `git clone` of this worktree and
`git checkout a17283486d5fe89591edfff08832712936d73030`, confirmed with
`git rev-parse HEAD` before use and removed with `rm -rf` after). The one
exception is FORGE-007's git-ref-name check, which demonstrates a general
fact about git itself (not about Ariadne's own source) and so needed no
copy of the application at all — it ran in its own empty, throwaway
repository, also deleted after use.

## Threat assumptions (per the task's agreed scope)

- Installed coding-agent programs (`gh`, `glab`, the ACP agent) are trusted
  *as programs* — running them is not itself a violation. What is audited
  is whether hostile forge content can get them invoked, or get an agent's
  own decision steered, toward an action nobody authorized.
- Repository content, forge content (titles, bodies, comments, branch
  names, file paths, head-repo URLs), browser/network responses, and other
  users' files are potentially hostile.
- Documented permission guarantees (who may comment/review/resolve/push/
  merge) are checked even though the agents that exercise them are trusted.
- No requirement to contain a deliberately malicious *installed* agent
  process; the question is whether hostile forge-side input reaches a
  sensitive operation, not whether a rogue binary could misbehave on its
  own.
- No attack was run against the user's live daemon, repositories, accounts,
  or external services. Every reproduction below is either a pure
  in-process unit test run inside a disposable local clone, a local
  git-only stand-in in its own throwaway directory, or a read of this
  sandbox's own installed `gh` binary, the vendored `localtunnel-client`
  crate's source, and public GitHub/GitLab/RustSec documentation.

## Coverage

### Reviewed in full
- `crates/ariadne-daemon/src/webhooks.rs`
- `crates/ariadne-daemon/src/forge/hooks.rs`, `forge/mod.rs` (incl. tests),
  `forge/pulls.rs`, `forge/live.rs`, `forge/news.rs`, `forge/poll.rs`
- `crates/ariadne-daemon/src/forge/tunnel.rs`, cross-checked against the
  spec, and this revision's own direct read of the vendored
  `localtunnel-client` 0.1.8 source
  (`~/.cargo/registry/src/.../localtunnel-client-0.1.8/src/lib.rs`), in
  full for the registration (`get_tunnel_endpoint`) and forwarding
  (`tunnel_one_connection`, `proxy_through`) functions specifically —
  grepped for any TLS-related symbol (`tls`, `rustls`, `native_tls`,
  `ssl`, `TlsConnector`, `TlsStream`) across the whole crate, with zero
  matches outside the `reqwest`-based registration call.
- `crates/ariadne-daemon/src/forge/github/{comments,details,hooks,issues,open,pulls,reviews}.rs`
- `crates/ariadne-daemon/src/forge/gitlab/{comments,details,hooks,issues,open,pulls,reviews}.rs`
- `crates/ariadne-daemon/src/scheduler/pull_requests.rs` (the news-delivery
  boundary: `tell_pull_request` -> `news::untold` -> `prompts::pull_request_news`
  -> `acp.send_news`; and the keeper-gating boundary: `pull_request_pass`'s
  `role != "author"` early return and `babysit_pass`)
- `crates/ariadne-daemon/src/agents/prompts.rs` (incl. tests)
- `crates/ariadne-daemon/src/http/pull_requests.rs`, `http/channel.rs`,
  `http/caller.rs`, `http/forge.rs`
- `crates/ariadne-daemon/src/gitwt.rs`
- `crates/ariadne-daemon/src/launcher.rs`, in particular lines 263-349
  (`Launcher::launch`, the permission-mode assignment), 1449-1492
  (`review_worktree`), 1546-1585 (`keeper_remote`, `keeper_push_command`),
  1835-1897 (`resume_keeper_session`, `keeper_briefing_of`), 2032-2047
  (`pull_request_switch`)
- `crates/ariadne-store/src/webhooks.rs`, `src/forge.rs`,
  `src/pull_request_comments.rs`
- `crates/ariadne-store/skills/pr-babysit/SKILL.md` (every step and rule,
  specifically re-read this revision line by line against FORGE-003's
  claimed consequence), `skills/pr-reviewer/SKILL.md`
- `crates/ariadne-cli/src/commands/goal.rs` (the `--from-issue` CLI path,
  lines ~196-248)
- `specs/021-acp-runtime.md`, `specs/022-ai-permission-mode.md`,
  `specs/002-repositories-branches-and-worktrees.md`
- `specs/025-forge-integration.md` through `specs/029-reviewing-a-request.md`,
  `specs/032-babysitting-a-request-by-hand.md`
- `crates/ariadne-daemon/tests/it/pull_request_babysit.rs`, in full this
  revision (not only the two tests previously cited), to find and read
  `a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository` and its
  neighboring `Shown` fixture precisely.

### Reviewed in part
- `crates/ariadne-daemon/src/http/repositories.rs` (the `forge_asked`
  enable/disable path only)
- `crates/ariadne-daemon/src/config.rs` (webhook/tunnel keys only)
- `crates/ariadne-store/src/defaults.rs` (the four seat system-prompt
  constants, and the pull-request briefing templates)
- `crates/ariadne-daemon/src/acp.rs`, `acp_calls.rs` — grepped for any
  mechanism that restricts an agent's own native tool set per seat (none
  found; see FORGE-008).

### Excluded, with reason
- `ui/src/*` — displayed forge content is VIEW's assigned territory.
- EXEC's worktree-execution internals beyond what FORGE-008 and FORGE-004/
  FORGE-007 directly implicate (sandboxing of the agent process itself,
  the ACP permission-decision pipeline's own code in `ai_permissions/`).
- `specs/030-workflows.md`, `specs/031-needs-attention.md` — read only as
  citations from 026/029/032.
- `crates/ariadne-store/src/pull_requests.rs` (the plain row CRUD) — the
  specific writes it exposes are exercised through `http/pull_requests.rs`
  and `forge/pulls.rs`, already reviewed in full.
- The `glab` binary — not installed in this sandbox. GitLab-specific
  claims are backed by GitLab's own published documentation (fetched live
  via `WebFetch`) or by reading Ariadne's own `gitlab/*.rs` source, not by
  running the real `glab` CLI.
- A live daemon + stub-forge + stub-tunnel run of the full acceptance
  suite named in specs/027 was not re-executed beyond the bounded subset
  cited within each finding below.

### Platform/tooling coverage gaps (explicit)

- **No `glab` binary** — GitLab CLI claims rest on GitLab's own
  documentation, not an installed binary.
- **No equivalent GitLab integration test for comment delivery** —
  FORGE-001's reproduction cites a GitHub-backed integration test; the
  code path it exercises is forge-agnostic, but no committed GitLab-backed
  test exercises this exact scenario end to end.
- **No live network access to a real GitHub/GitLab instance, or to a real
  `localtunnel.me` registration**, per the task's constraints. Every
  forge- or tunnel-facing claim is a code-reading claim, a reproduction
  against a local stand-in, or a citation of the forge's or the tunnel
  crate's own published documentation/source.
- **macOS, single-user sandbox.** No cross-platform check was performed;
  none of this report's findings are believed to be platform-specific.

## Tracer walkthrough: what is actually trusted, and where

`Webhook or forge response -> verification and fetch -> stored request or
issue -> agent context -> requested forge action`

1. **Webhook delivery is a trigger, never a data source.** The listener
   verifies GitHub's `X-Hub-Signature-256` or GitLab's `X-Gitlab-Token`
   before anything else, and on success only calls `ForgePoll::wake(id)`.
   All pull request/comment content an agent ever sees comes from a
   fresh, separately-authenticated `gh`/`glab` API call. **Confirmed.**
2. A replayed, previously-valid signed delivery only triggers one more
   coalesced `wake()` — harmless by design. **Confirmed.**
3. The listener binds a separate port, loopback by default, serving
   exactly the two signed POST routes and nothing else. **Confirmed.**
4. **Changed forge identity** is handled safely: an atomic SQL clause
   clears the webhook secret the instant the forge identity changes, so a
   replaced remote's old secret can never authorize a delivery against the
   new identity. **Confirmed — but see FORGE-005**, which covers two
   distinct, related gaps that identity-change protection does not reach:
   a stale hook kept registered (and its never-rotated secret) when only
   the *public URL* changes with the identity held constant, and, more
   fundamentally, the forwarded tunnel connection's own lack of
   encryption regardless of whether anything is stale at all.
5. Fetched comments, titles, bodies, and branch metadata flow into agent
   prompts as plain text, with no sanitization beyond stripping Ariadne's
   own HTML-comment signature markers. **Confirmed as a mechanism — but
   see FORGE-003**: that stripping step is part of a (now narrower, see
   below) provenance-labeling defect.
6. **Ariadne's own "no merge/approve" API guarantee does not bound the
   underlying agent.** Pull-request sessions (reviewer, keeper, and task
   agents alike) take the repository's own ACP permission mode
   (`launcher.rs:263-349`), which defaults to `auto` (spec 002) and which
   grants every permission request with no human gate at all in that mode
   (spec 021 rule 9). Ariadne's own `submit_review` route genuinely
   refuses any approve/merge call, but that only constrains Ariadne's
   *own* tools, not the coding agent's general-purpose Bash tool, which
   every seat retains and which is gated solely by the permission mode.
   This is now finding **FORGE-008**, below.
7. **Resolve-thread scoping, corrected (unchanged from the prior
   revision):** a reviewer session resolves a thread whose *first comment*
   is attributed to the integration's own login — not necessarily a
   thread that specific review session itself opened. An external
   attacker still cannot trigger this, since it still requires the
   thread's opener to be the integration's own authenticated identity.

## Findings

Ordered by severity. Each states what is confirmed by code and/or
reproduction, and what remains inferred or unresolved.

### FORGE-008 — Ariadne's own "no merge/approve" guarantee does not bound the underlying agent; the backstop is the repository's permission mode, which defaults to no human gate at all (Confirmed code-level gap; exploitation unverified; High)

*(This finding was numbered FORGE-002 in the previous revision. It is
renumbered here because FORGE-002 is restored to its original meaning,
the issue-import finding below — see the Revision note.)*

**Affected files/lines:**
- `crates/ariadne-daemon/src/launcher.rs:263-349`, specifically lines
  275-289 (pull-request sessions take the repository's mode exactly as a
  task session does) and line 313 (`let mode = repo.permission_mode();`).
- `specs/002-repositories-branches-and-worktrees.md` acceptance criteria:
  "A repository is `auto` until told otherwise."
- `specs/021-acp-runtime.md` rule 9: `auto` "selects the first allowing
  option, then the first option, and cancels only an empty list."
- `crates/ariadne-daemon/src/http/pull_requests.rs:828-836` (the one
  guarantee that *does* hold: Ariadne's own `submit_review` route refuses
  `approve`).
- `crates/ariadne-store/src/defaults.rs:383` (the reviewer's "commit
  nothing, push nothing" instruction — prose only).

**Corroborating evidence from EXEC's parallel audit** (relayed by the
orchestrator; not re-run here, to avoid duplicating EXEC's own work): an
isolated reproduction found `launcher.rs:1648-1675` creates reviewer
sessions through the ordinary launch path, `acp.rs:2052-2270` routes
permission decisions such that `auto` allows a request while
`ask`/`learn`/`ai` can constrain it, `acp.rs:2620-2625` advertises no
client terminal capability (a missing client handler does not disable an
agent's own native tools), and `acp.rs:965-978`/`gitwt.rs:94-105` apply no
reviewer-specific filesystem or process isolation. EXEC's own isolated
test performed tracked-file changes, a sibling marker write, and a
detached commit successfully; a plain `git push` failed only because
`HEAD` was detached, and an explicit-refspec push succeeded against a
disposable local bare remote. EXEC's own caveats apply in full: this does
not prove malicious-comment causation, and no real model or installed
agent accepted an injected instruction.

**Prerequisites:**
- The repository runs the default `auto` permission mode.
- The coding-agent CLI backing the session offers a general-purpose tool
  (Bash, or equivalent) capable of invoking `gh`/`glab`/`git` directly.
- Some input succeeds in steering the agent to decide to run such a
  command. **Not demonstrated or claimed here.**

**What is confirmed:** in the default configuration, if a reviewer,
keeper, or task agent's own tool decides to run `gh pr merge`, `glab mr
approve`, or `git push --force`, nothing in Ariadne's code stops it. The
"no route approves or merges" guarantee covers only calls through
Ariadne's own `/v1/pull-requests/...` API.

**What is not claimed:** that realistic forge content actually causes an
agent to decide to run such a command — that would require either a live
LLM-backed agent under adversarial prompting (out of scope for this
control-gap audit) or a pre-programmed stub, which proves nothing beyond
what the code reading above already establishes.

**Counter-evidence and what would change this assessment:** the actual
default a real deployment runs on matters enormously. If most
forge-integrated repositories run `ask`, `learn`, or `ai` mode rather than
`auto`, a human or a model is interposed before any such command runs,
and this finding's severity drops substantially.

**Recommendation (smallest remediation, PERM's to scope):** this
finding's root cause — a permission-mode default with no human gate,
applied identically to every seat — is the permission-policy engine's own
territory (specs 021/022). Two directions, neither scoped here: (a) give
the reviewer/keeper seats a stricter default than the repository's own
setting; or (b) require confirmation for any Bash/shell invocation
resolving to `gh`/`glab`, regardless of the general permission mode.
**Regression criterion:** an ACP runtime test asserting that a `Bash`
permission request whose command resolves to `gh`/`glab` with a merge- or
approval-shaped subcommand is never auto-approved by `auto` mode on a
pull-request session (if that asymmetry is chosen), or on any seat (if the
stricter fix is chosen).

### FORGE-005 — The tunnel's forwarded transport carries every webhook payload, and GitLab's secret, in plaintext; a stale destination can also receive them after the integration moves on (Confirmed, two independently demonstrated mechanisms; High)

**Part A — transport confidentiality (new this revision; applies the
moment the tunnel is used at all, with no staleness or squatting
prerequisite).**

**Affected files/lines:**
- `~/.cargo/registry/src/.../localtunnel-client-0.1.8/src/lib.rs:329`
  (`tunnel_one_connection`): `TcpStream::connect(&addr)` — a bare,
  unencrypted TCP connection to the tunnel server's assigned data port.
- `src/lib.rs:362-375` (`proxy_through`): `io::copy_bidirectional(&mut
  remote_stream, &mut local_stream)` — raw byte forwarding between that
  unencrypted remote socket and the local webhook listener, with no
  decryption, re-encryption, or integrity check of its own.
- `src/lib.rs:392-400` (`get_tunnel_endpoint`): the *only* TLS in the
  entire crate — a `reqwest::Client` used solely for the one-time
  registration HTTP call that learns `server_host`/`server_port`. Grepping
  the whole crate for `tls|rustls|native_tls|ssl|TlsConnector|TlsStream`
  returns matches only inside `reqwest`'s own dependency tree, none in
  this crate's own forwarding code.

**What this means, concretely:** when GitHub or GitLab delivers a webhook
to the tunnel's public HTTPS URL, TLS terminates at the tunnel
*provider's* edge (`localtunnel.me` by default), not at the daemon — that
termination point necessarily sees the decrypted HTTP request. What
happens next, from this client's own code, is a second, *unencrypted* TCP
hop carrying that decrypted request from the provider's data port to the
daemon's local relay. This is not a bug introduced by a stale or reused
destination; it is the ordinary, successful-case behavior of using this
tunnel feature at all, with no forged or squatted anything. Anyone
positioned to observe that internal hop — the tunnel provider itself, or
anyone who compromises or is positioned on its infrastructure — can read
the complete plaintext of every webhook delivery: the full request body
(PR/MR numbers, comment text, commit SHAs — whatever the forge includes)
and every header, GitHub's HMAC signature included. **HMAC and (where
chosen) secret rotation protect only authenticity/integrity of a
delivery's origin; neither is a confidentiality mechanism, and neither
stops this disclosure on either forge.** GitLab's plaintext
`X-Gitlab-Token` (Part B, carried over from the prior revision) is a
second, GitLab-specific exposure on top of this forge-agnostic one.

**Prerequisites:** the tunnel feature in use (not a fixed
`webhook_public_url`, which this audit's tracer already established opens
no tunnel at all) and a position on the tunnel provider's own
infrastructure, or control of it. This audit did not and could not verify
whether `localtunnel.me`'s own server-side implementation adds a
TLS-wrapped hop internally before this client-visible plaintext leg (no
primary source found either way, and testing the real service is out of
scope) — what is confirmed is specifically this crate's *client-side*
code, which the daemon runs as-is.

**Part B — stale destination and no secret rotation (carried over,
corrected from the prior revision's framing where noted).**

- `crates/ariadne-daemon/src/forge/hooks.rs:156-166` — when an enabled
  integration's public URL goes away (tunnel down, or `webhook_public_url`
  withdrawn) while the integration stays `enabled`, the code only clears
  its own local bookkeeping; it never calls `delete_hook` or
  `update_hook_url`, so the forge keeps sending to the old, now-unbound
  address.
- `crates/ariadne-daemon/src/forge/gitlab/hooks.rs:31-38` — the webhook is
  registered with GitLab's legacy `token` field, which GitLab's own
  documentation states is "sent as plain text in the `X-Gitlab-Token`
  HTTP header" (fetched via `WebFetch` against
  `docs.gitlab.com/user/project/integrations/webhooks/`). GitLab 19.0's
  newer HMAC `signing_token` parameter is not used.
- `crates/ariadne-daemon/src/forge/hooks.rs:119-127` — the secret is
  generated only once and never rotated on a URL change, only on an
  identity change.
- **Unresolved** (unchanged from the prior revision): whether an attacker
  can realistically gain control of a specific, previously-used
  localtunnel subdomain depends on the provider's own subdomain-reuse
  window, which this audit did not and could not test.
- **Impact ceiling, reassessed in light of Part A:** the prior revision
  bounded this to "a stolen secret enables forged wakes, not data
  injection" — true, but Part A shows the premise ("stealing the secret
  requires squatting a stale subdomain") is not actually the only way to
  observe it: the live, in-use tunnel discloses it on *every* delivery to
  anyone positioned on the provider's own infrastructure, with no
  staleness needed at all. Part B's own, additional contribution is that
  the same secret keeps working against the *current* endpoint
  indefinitely once observed, by either route.

**Recommendation (smallest remediation):** Part A has no small,
forge-ingestion-side fix — it is inherent to this third-party tunnel
protocol's own design, and the only ways to remove it are (i) not
disclosing the tunnel's existence/use to the user as risk-free, i.e.
documenting the trust dependency explicitly (a docs/UX fix, not code),
or (ii) offering a transport that does not depend on a third party for
TLS termination (a reverse-proxy-based alternative to `webhook_listen`
with the operator's own certificate, which spec 027 rule 1 already
mentions as possible: "A reverse proxy can use a fixed configured
address"). Part B: (a) have the public-URL-withdrawn branch delete or
update the forge-side hook when an enabled integration loses its public
URL; and/or (b) rotate `webhook_secret` on every public-URL change, not
only on identity change; and/or (c) adopt GitLab's `signing_token`
(HMAC) in place of the legacy plaintext `token`, which would also narrow
Part A's GitLab-specific secret exposure to the body-disclosure Part A
already covers for both forges. **Regression criterion:** a `webhooks.rs`
test asserting a tunnel-down transition on an enabled integration either
deletes the forge-side hook or issues a fresh secret, and (for Part A, if
the reverse-proxy alternative is pursued) a doc/config test asserting the
tunnel path's own documentation names the third-party-visibility trust
dependency explicitly.

### FORGE-003 — An attacker's own comment can be mislabeled "by the Ariadne review" in the agent's own news feed, with no author check (Confirmed defect, reproduced; retracted behavioral-impact claim; Low)

**What changed this revision:** the previous draft claimed the
`pr-babysit` skill "treats Ariadne findings with elevated trust" and is
"instructed to track and resolve" them, making the false attribution an
amplifier. Re-reading `pr-babysit/SKILL.md` line by line: step 3 (lines
19-29) handles every unanswered comment identically by its *content*
shape (an ask for a change, a question, thanks, a follow-up, a
conversation comment) — nothing there reads or branches on `from_review`
at all. The skill's own Rules (line 52) say **"Leave every thread open"** —
the literal opposite of "tracks and resolves." The claimed behavioral
consequence is retracted as unsupported; only the reproduced defect
itself (the false attribution text) is confirmed.

**Affected files/lines:**
- `crates/ariadne-daemon/src/forge/live.rs` (`comments()`): any comment
  body ending in the literal text `<!-- ariadne:review -->` gets
  `from_review = true`, regardless of its real `author_login`.
- `crates/ariadne-daemon/src/forge/news.rs::comment_lines`: the `by`
  attribution is computed from `comment.from_review` alone:
  ```rust
  let by = match comment.from_review {
      true => "the Ariadne review",
      false => comment.author_login.as_str(),
  };
  ```

**Reproduction** (preserved as `repro/forge-003-review-mark-spoof.patch`;
re-verified this revision inside a disposable local clone of the examined
commit — see "Reproduction workflow" below): a `NewPullRequestComment`
with `author_login: "attacker"` and a body ending in the review mark,
run through the real `live::comments()` then `news::of()`:
```
cargo nextest run -p ariadne-daemon \
  -E 'test(forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review)'
```
```
PASS [0.026s] ariadne-daemon forge::news::tests::forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review
Summary [0.159s] 2 tests run: 2 passed, 965 skipped
```
(run together with FORGE-004's test in the same disposable clone, hence
"2 tests run"). The test confirmed `from_review` became `true` for the
attacker-authored comment, the resulting news line contained "by the
Ariadne review," and did not contain the string "attacker" anywhere.

**Impact, correctly scoped:** the agent's news text misattributes an
outsider's comment to Ariadne's own review. This is a genuine
transparency/provenance defect — a human or agent reading the news later
cannot tell, from this text alone, that the comment did not come from
Ariadne's own prior review. This report found **no** skill text or code
path (beyond the already-covered `open_p0` check in
`http/pull_requests.rs::submit_review`, which has the same root cause but
a bounded, validation-only consequence) where this label changes what
action the agent takes. The severity is downgraded from the prior
revision's "Medium" to **Low** to match this narrower, correctly-bounded
impact.

**Recommendation (unchanged):** require
`comment.author_login.eq_ignore_ascii_case(login)` in addition to the
marker check wherever `from_review` is read for display, exactly the
guard `is_mine`/`is_review_summary` already apply elsewhere.
**Regression criterion:** the reproduction above, promoted into the real
suite, asserting correct attribution for a non-login author.

### FORGE-001 — Comments that drive agent-authored commits carry no commenter-trust signal (Confirmed structural gap; unauthorized-commit outcome unverified; High as a gap, not independently reproduced as an attack)

**Affected files/lines:** `forge/live.rs:539-636` (`is_mine`, `untold`;
the only identity check anywhere is `author_login.eq_ignore_ascii_case(login)`),
`forge/news.rs` (`comment_lines`), `github/details.rs:37-65` (never reads
GitHub's own `author_association` field), `gitlab/details.rs:146` (reads
only `note.author.bot`). `author_is_bot`
(`ariadne-store/src/pull_request_comments.rs:17`) is written in five
places and read in none. `pr-babysit/SKILL.md` step 3 (line 25)
instructs: "A follow-up ask in a thread you answered: a new ask. **Act on
it.**"

**Reproduction:** cites an existing, committed integration test,
`crates/ariadne-daemon/tests/it/pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`
(lines 271-287), which scripts a comment from an arbitrary login
(`alice`, carrying no trust/association designation) and asserts it
reaches the live stub agent's actual prompt text
(`h.prompts_to(&session)`):
```
cargo nextest run -p ariadne-daemon \
  -E 'test(asking_keeps_a_request_of_mine_by_hand_until_the_merge)'
```
```
PASS [4.091s] ariadne-daemon::it pull_request_babysit::asking_keeps_a_request_of_mine_by_hand_until_the_merge
Summary [4.092s] 1 test run: 1 passed, 964 skipped
```

**Measured vs. inferred, stated precisely:** measured — a comment from an
arbitrary forge login reaches a live agent process's actual prompt text,
by the real mechanism, with no distinguishing mark. Not measured, and not
claimed — that a real LLM-backed agent would act on adversarial content
the way it acts on the stub's benign comment; that is exactly the
"generic claim that prompts can influence models" this report keeps
distinct from the confirmed control gap (no authorization check exists at
all before the content is handed over as actionable).

**Commenter prerequisites, with primary sources:** GitHub's REST API
`author_association` enum lists `NONE` alongside `OWNER`/`MEMBER`/
`COLLABORATOR`/`CONTRIBUTOR` (`docs.github.com/en/rest/issues/comments`);
GitLab's Guest role — its lowest access level — suffices to "Comment and
add suggestions" on a public project's merge request
(`docs.gitlab.com/user/permissions/`).

**Recommendation and regression criterion (unchanged):** thread an
author-trust signal through before a comment becomes "news," and gate the
skill's "act on it" instruction behind it or an explicit human
confirmation. Extend the cited existing test with a second, untrusted
comment and assert differing treatment.

### FORGE-004 — A fork's `head_repo` is fetched with no host validation at all in the review worktree path (Unverified candidate; Medium)

**Affected files/lines:**
- `crates/ariadne-daemon/src/launcher.rs::review_worktree`, lines 1449-1492
  (specifically 1456-1471): fetches directly from `pull.head_repo` with no
  call to `Remote::parse` or any host check at all.
- Contrast `launcher.rs::keeper_remote`, lines 1546-1576 — **corrected
  description, per this review**: on a successful `Remote::parse` that
  matches the configured forge's own repository exactly, it reuses the
  configured remote (safe); on a genuine cross-repository match it
  rewrites the configured remote's authority onto the fork's path via
  `Remote::rewrite` (also safe, preserves the trusted host); but **on a
  parse *failure*, `launcher.rs:1556-1558` returns the raw, unvalidated
  `head_repo` string directly — not the configured remote.** The prior
  revision incorrectly described this fallback as returning to the safe
  configured remote; it does not. There is no host-mismatch *refusal*
  anywhere in `keeper_remote` either — only a parse-failure fallback that
  happens to be unsafe, and a successful-parse path that happens to be
  safe. `keeper_remote` is on balance safer than `review_worktree` (which
  has no validated path at all), but it does not "already perform" the
  guard this finding recommends adding.
- `head_repo` itself is populated verbatim from the forge API's own field
  with no host check anywhere upstream: `github/pulls.rs::normalized`
  (GraphQL `headRepository.url`) and `gitlab/pulls.rs::normalized`
  (`source_project.http_url_to_repo`).

**Reproduction** (preserved as `repro/forge-004-head-repo-no-validation.patch`;
re-verified this revision inside the same disposable clone as FORGE-003,
see "Reproduction workflow" below): two local, throwaway git repositories
stood in for a victim checkout and a fork at an unrelated filesystem path.
`GitManager::fetch_head_of` accepted the fork's bare filesystem path — not
a URL, not even a URL scheme — as `from`, and fetched its commit:
```
cargo nextest run -p ariadne-daemon \
  -E 'test(fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check)'
```
```
PASS [0.158s] ariadne-daemon gitwt::forge_004_reproduction::fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check
```
This establishes the absence of a guard conclusively; it does not
establish that a real forge response can ever actually carry a cross-host
value in this field, which remains unresolved (no primary source found
either way; see FORGE-007 below for the closely related, but distinct,
question of whether `head_repo` can differ in *ownership* — not
necessarily *host* — from what the request's author controls, which this
revision answered for GitHub and GitLab specifically).

**Recommendation (smallest remediation, corrected):** validate
`head_repo`'s host against the integration's own `forge.host` **before**
`review_worktree` ever calls `fetch_head_of` with it, with a clear
refusal on mismatch — framed here as a **new** guard, not as reusing one
`keeper_remote` already has (it does not). **Regression criterion:** a
`launcher.rs` test asserting `review_worktree` refuses a `pull.head_repo`
whose host does not match the integration's configured host.

### FORGE-007 — The keeper's briefing hands the agent a literal, unescaped shell command built from a forge-sourced branch name; whether that branch name is exclusively the authenticated user's own choice is an open question, not a settled one (Confirmed code hazard; reachability is an unresolved prerequisite, corrected from "not reachable"; Medium-High)

**What changed this revision:** the previous conclusion — that the
`role == "author"` gate makes this unreachable by a third party — is
retracted. It conflated "the request's author is the trusted user" with
"the head branch's name was chosen by the trusted user," which are not
the same fact, and an existing, maintained test proves they are not:

```rust
// crates/ariadne-daemon/tests/it/pull_request_babysit.rs:609-639
async fn a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository() {
    ...
    stub.reprogram(script(&Shown {
        head_repo: "https://github.com/alice/widgets",
        ..Shown::open(&head)   // Shown::open defaults author: "me"
    }));
    ...
    assert!(told.contains("git push https://github.com/alice/widgets.git HEAD:fix"), ...);
}
```
This test's own fixture (`Shown::open`, `pull_request_babysit.rs:41-50`)
defaults `author: "me"` — satisfying `role == "author"` exactly as the
previous revision relied on — while `head_repository` is explicitly
`alice/widgets`, a different account's repository. The keeper's
`push_command` is built from *that* repository and *that* repository's
own branch, named by whoever can push there.

**Primary-source confirmation that this is not a hypothetical edge case,
for both forges:**
- GitHub's own REST API documentation for creating a pull request
  (`docs.github.com/en/rest/pulls/pulls#create-a-pull-request`, fetched
  via `WebFetch` this revision): **"To open or update a pull request in a
  public repository, you must have write access to the head or the
  source branch."** Write access to the head repository — not
  authorship of its commits, and not ownership of that repository — is
  the actual requirement. A collaborator relationship (being added to
  someone else's fork, or a shared team fork) satisfies this without the
  PR's author having chosen, or even seen, the branch's name themselves.
- GitLab's own documentation on creating merge requests
  (`docs.gitlab.com/user/project/merge_requests/creating_merge_requests/`,
  fetched this revision): **"You must have the Developer, Maintainer, or
  Owner role, or be allowed to create merge requests in the project"** —
  the same role-based pattern: permission to *open* the MR is independent
  of who actually pushed the source branch's commits or chose its name.

**Affected files/lines:**
- `crates/ariadne-daemon/src/launcher.rs:1579-1585` (`keeper_push_command`):
  ```rust
  async fn keeper_push_command(&self, pull: &PullRequest, repo: &Repository) -> Result<String> {
      Ok(format!("git push {} HEAD:{}", self.keeper_remote(pull, repo).await?, pull.head_branch))
  }
  ```
  `pull.head_branch` is folded into a plain `format!` string with no
  escaping or quoting.
- `crates/ariadne-store/src/defaults.rs:412-423`
  (`PULL_REQUEST_KEEPER_BRIEFING`): hands the agent `"Push with:
  {push_command}"` as the literal, operationally-necessary command to
  run (the keeper's local branch name differs from the request's own
  head branch, so a bare `git push` would not work without it).

**Confirmed, by reproduction (this report's own, redone correctly this
revision — see the fixed command sequence below): git's own ref-name
rules permit shell metacharacters**, so nothing about git itself, or
about the documented permission model that lets a non-author push the
branch a PR cites, would stop such a name from existing on a real forge.

**What remains genuinely unresolved, stated as the review asked:**
1. Whether, in practice, a keeper-eligible request (the user's own,
   `role == "author"`) commonly or ever actually has a `head_repo` the
   user does not themselves exclusively control. This requires a
   collaboration arrangement (shared fork access, a team repository, a
   maintainer relationship) that this audit cannot establish is common,
   rare, or absent in real deployments — only that the system explicitly
   models and tests it as a supported scenario.
2. Whether a collaborator with write access to a shared fork, who is not
   the keeper-eligible request's "author," would realistically be in a
   position to choose a hostile branch name without the request's actual
   author noticing before asking Ariadne to babysit it. This is a human-
   process question this audit cannot settle from code alone.
3. Required human actions, for both forges: opening the pull/merge
   request itself is always an explicit, authenticated action by the
   trusted user (not automatic, not webhook-triggered) — but, per the
   permission-model citations above, the *branch's content and name*
   need not be that same action, or even reviewed by that user, before
   the PR/MR cites it.

**What is not claimed:** that this scenario is presently exploited or
easily exploitable; that a real coding agent would copy-paste and execute
the string verbatim rather than quoting the branch name itself; or that
EXEC's own command-execution mechanics are reproduced here (cited by
attribution only, per the prior revision).

**Fixed reproduction** (run in its own disposable, throwaway git
repository — no copy of the application needed, since this demonstrates
a fact about git itself, not about Ariadne's code):
```
$ mkdir /tmp/forge-branch-check && cd /tmp/forge-branch-check
$ git init -q -b main
$ git -c user.email=a@a -c user.name=a commit -q --allow-empty -m init
$ git check-ref-format --branch 'fix$(touch,pwned)'; echo "check-ref-format-exit=$?"
fix$(touch,pwned)
check-ref-format-exit=0
$ git branch 'fix$(touch,pwned)'; echo "branch-create-exit=$?"
branch-create-exit=0
$ git branch --list
  fix$(touch,pwned)
* main
```
(The previous revision's published sequence omitted the `commit`
step; replayed as published, `git branch` fails with `fatal: not a valid
object name: 'main'`, exactly as this review found. The sequence above
was re-run fresh for this revision and is the complete, exact transcript.)

**Recommendation (smallest remediation, split along the orchestrator's
line, unchanged in substance):** FORGE's part — validate `pull.head_branch`
against a conservative allow-list before it is used to build any
templated instructional text, at the point it is read off the forge.
EXEC's part — `keeper_push_command` should not compose a raw shell string
at all; give the agent the remote and the refspec as separate, structured
values. **Regression criterion:** a `launcher.rs` test asserting
`keeper_push_command` (or its replacement) refuses or safely escapes a
`head_branch` containing shell metacharacters, run alongside a
`forge/pulls.rs` test asserting such a branch name is rejected or
sanitized at ingestion, *and* a new test exercising the
`a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository` fixture
with a hazardous branch name specifically, since that is the fixture that
proves the scenario is real.

### FORGE-002 — A hostile issue's title/body reaches the orchestrator's briefing verbatim; the CLI import path has no preview step at all (Hardening suggestion; Low)

*(Restored to its original identifier this revision — see the Revision
note. This finding was mislabeled FORGE-006 in the immediately prior
revision.)*

**Affected files/lines:**
- `crates/ariadne-cli/src/commands/goal.rs:196-248` (`--from-issue <url>`
  fetches the issue and, absent an explicit `--title`/`-d` override, posts
  the goal immediately with no intermediate display or confirmation).
- `crates/ariadne-daemon/src/agents/prompts.rs:136-169`
  (`orchestrator_briefing` interpolates `goal.description` verbatim).

**Why this remains a suggestion, not a confirmed vulnerability:** the
human still explicitly chooses *which issue* to import, by URL. The
orchestrator's resulting actions still pass through the ordinary
develop/review/merge workflow, including whatever remediation is chosen
for FORGE-008. No bypass of those controls specific to the orchestrator
seat was found or attempted.

**Recommendation:** for the CLI path, print the fetched issue's title and
body and require confirmation before posting the goal. A VIEW/CLI-scoped
UX change, not a daemon change.

## Reproduction workflow (applies to FORGE-003 and FORGE-004)

Both patches were re-verified this revision inside a disposable local
clone, never against the task's own worktree, per the task's requirement
to use temporary copies for source instrumentation:
```
rm -rf /tmp/forge-repro-a1728348
git clone -q <this worktree> /tmp/forge-repro-a1728348
cd /tmp/forge-repro-a1728348
git checkout -q a17283486d5fe89591edfff08832712936d73030
git apply repro/forge-003-review-mark-spoof.patch
git apply repro/forge-004-head-repo-no-validation.patch
cargo nextest run -p ariadne-daemon \
  -E 'test(forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review) | test(fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check)'
# -> PASS, PASS; Summary: 2 tests run: 2 passed, 965 skipped
rm -rf /tmp/forge-repro-a1728348
```
The clone was deleted immediately after. See `repro/README.md` for the
exact, copy-pasteable version of this sequence.

## Rejected hypotheses (with reasoning)

- **GitLab review-comment `-f position[new_path]=<attacker-chosen
  filename>` triggers `glab`'s `@file` file-read magic value.** Verified
  against GitLab's own CLI documentation (`docs.gitlab.com/cli/api/`):
  the `@filename` magic value is documented exclusively for `-F`/`--field`;
  `-f`/`--raw-field` has no such behavior. Every attacker-influenced
  string in the forge clients uses `-f`. **Rejected.** (The installed
  `glab` binary itself was not run to confirm it matches its own
  documentation — see "Platform/tooling coverage gaps.")
- **Git refspec injection via a malicious PR head-branch name.** Git's own
  `check-ref-format` forbids a colon in any ref name. **Rejected.**
- **SSRF via the tunnel relay reaching anywhere but the fixed webhook
  listener, or the main API port.** Confirmed by sub-review with file:line
  citations. **Rejected.** (Distinct from FORGE-005's Part A, which is
  about the forwarded connection's *confidentiality*, not its
  *destination*.)
- **CLI argument injection via a forge-controlled string starting with
  `-`.** Confirmed by sub-review across all 14 forge-client files: every
  such value sits in a fixed argv slot, and `Cli::run` never goes through
  a shell. **Rejected.**
- **GraphQL/JSON injection via forge-controlled comment bodies or paths.**
  GraphQL variables are passed structurally; review bodies are built with
  `serde_json::json!`. **Rejected.**
- **SSRF via the fork `head_repo` rewritten for the keeper's *push*
  target.** Narrowed, not fully rejected: `keeper_remote`'s successful-parse
  path is reasonably protected (see FORGE-004's corrected description
  above for exactly what it does and does not guarantee); its
  parse-failure fallback is unsafe and is covered within FORGE-004, not
  here.

## Dependency advisories

Unchanged: `cargo audit` against the full workspace `Cargo.lock` (557
crates, 1307 advisories) reports **0 vulnerabilities**; two unrelated
maintenance warnings (`bincode` via `syntect`, `chacha20` via
`rand`→`ulid`), neither reachable through the webhook/forge/tunnel
dependency subtree. This includes `localtunnel-client` 0.1.8 itself —
FORGE-005's Part A is a design property this audit found by reading the
crate's source, not a flagged advisory; `cargo audit` would not surface
it, since it is not a known CVE, just an unencrypted-by-design relay.

## Confidence and recommendation

**What this third round changes:** two conclusions from the prior
revision were wrong in the same direction — both underestimated reach by
over-trusting a gate that establishes less than it seemed to (role-based
authorship, in FORGE-007's case) or by not tracing far enough down a
transport (the tunnel's data-plane encryption, in FORGE-005's case). Both
are corrected above with reproductions and primary sources rather than
narrowed prose. The finding-identifier instability (FORGE-002's meaning
changing across revisions) is also fixed, with FORGE-002 restored to its
original meaning and the permission-mode finding moved to the unused
FORGE-008.

**High confidence**, by direct code citation and EXEC's independent
corroboration: FORGE-008. **High confidence**, by this revision's own
primary-source reading of the vendored `localtunnel-client` crate (zero
TLS symbols outside the registration call): FORGE-005's Part A. **High
confidence**, by code reading: FORGE-005's Part B. **High confidence**, by
reproduction re-verified in a disposable clone: FORGE-003 (narrowed) and
FORGE-004. **High confidence**, by reproduction against the project's own
committed test: FORGE-001. **High confidence that the hazard is real, and
now explicitly unresolved rather than falsely dismissed, that its
reachability is**: FORGE-007 — the existing fork-push test and both
forges' own documentation establish the mechanism exists by design; what
is *not* established, and is named as such, is how often real deployments
actually exercise it with a hostile branch name.

**Lower confidence, named explicitly:**
- FORGE-008's practical severity depends entirely on what permission mode
  real deployments actually run. **What would change this:** evidence of
  the actual distribution of `auto` vs. `ask`/`learn`/`ai` across
  forge-integrated repositories.
- FORGE-005 Part A's severity depends on whether `localtunnel.me`'s own
  server-side implementation adds protection this client-side code does
  not. **What would change this:** the provider's own architecture
  documentation, or a primary source on how `loca.lt`-style tunnels secure
  (or do not secure) their internal relay hop.
- FORGE-007's severity depends on how common the fork-collaboration
  prerequisite actually is in real usage. **What would change this:**
  usage data, or the user's own account of whether they babysit requests
  built on branches they did not personally push.
- FORGE-004's exploitability depends on whether a forge response can ever
  carry a cross-host `head_repo`, still unresolved after this revision's
  search for a primary source.

## Cross-boundary evidence for the orchestrator

Three-way split, as the orchestrator stated it: **FORGE owns ingestion
and influence; EXEC owns execution capability; PERM owns permission
policy.**

- **FORGE-008** belongs to **PERM**: its enforcement point is the
  permission-policy engine itself (specs 021/022). EXEC's corroborating
  evidence (cited within FORGE-008) answers the adjacent
  execution-capability question.
- **FORGE-007** is a three-way case: FORGE's own tracing (this report,
  corrected this revision with the fork-test counter-evidence) establishes
  which field reaches the hazard and that reachability is unresolved, not
  settled either way; EXEC owns whatever safer command-construction
  replaces the naive `format!` string; PERM's alternative (gating
  multi-token `Bash` calls) is a third option, not scoped here.
- **FORGE-004**'s fix is scoped to FORGE's own ingestion code, not the
  git-command layer, to avoid overlapping with EXEC on the same file.
- **FORGE-005** is entirely FORGE's own territory — both the webhook/hook
  code and the tunnel client live under `crates/ariadne-daemon/src/forge/`,
  within FORGE's assigned path.
- **FORGE-001 and FORGE-003** are entirely within FORGE's own ingestion
  layer; no cross-boundary coordination is needed for either's proposed
  fix.
- VIEW owns how forge content is displayed; FORGE-002's suggested
  mitigation (a confirmation step in the CLI import path) is a CLI/UX
  change.

## Compatibility decisions for the user

1. **For FORGE-008**: should reviewer/keeper (and task) sessions get a
   stricter default permission mode than the repository's own setting, or
   should the fix instead target the one class of tool call that matters
   (a `Bash` invocation resolving to `gh`/`glab` with a merge/approve-shaped
   subcommand) regardless of seat?
2. **For FORGE-001/FORGE-003**: should an untrusted or spoofing-marked
   comment still reach the agent as visible news with an annotation, or be
   withheld entirely until a human re-surfaces it?
3. **For FORGE-005**: Part B — is rotating the webhook secret on every
   public-URL change an acceptable operational cost, or is deleting the
   stale hook on URL loss preferred? Part A — is the tunnel's
   trust-the-provider-with-plaintext property acceptable as a documented
   trade-off for the convenience of not running a reverse proxy, or should
   the reverse-proxy alternative (spec 027 rule 1) be promoted as the
   recommended path for anyone handling sensitive repository content?
4. **For FORGE-004**: should an unresolvable or cross-host `head_repo` be
   a hard refusal, or fall back to the configured remote (a genuinely new
   guard, now that this revision has corrected what `keeper_remote`
   actually does on a parse failure)?
5. **For FORGE-007**: given that a keeper-eligible request's head branch
   is not guaranteed to be the trusted user's own choice, should Ariadne
   refuse to babysit a request whose `head_repo` differs from the
   repository's own configured remote at all (closing the fork-collaboration
   scenario entirely, at the cost of the feature `a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository`
   exists to support), or should the fix be narrower (validate/escape the
   branch name, leaving cross-fork babysitting itself intact)?

## Proposed follow-up tasks

- A remediation task scoped to the ACP permission-policy subsystem,
  **owned by PERM**, addressing FORGE-008, per compatibility question 1.
- A remediation task scoped to `forge/live.rs`/`forge/news.rs` addressing
  FORGE-003 and FORGE-001 together, per compatibility question 2.
- A remediation task scoped to `forge/hooks.rs`, `forge/gitlab/hooks.rs`,
  and (for Part A) either `forge/tunnel.rs`'s own documentation or a
  reverse-proxy-path promotion, addressing FORGE-005, per compatibility
  question 3.
- A remediation task scoped to `forge/pulls.rs`/`launcher.rs::review_worktree`
  addressing FORGE-004, per compatibility question 4.
- A remediation task addressing FORGE-007, scoped per compatibility
  question 5 and split along the FORGE/EXEC/PERM line within the finding.
- (Optional, low priority) A CLI/VIEW-scoped task addressing FORGE-002.
- (Optional) A short verification task to install `glab` and confirm its
  documented behavior against the real binary.
- No application fix is included in this change; this report is evidence
  and recommendation only, per the task's constraints.

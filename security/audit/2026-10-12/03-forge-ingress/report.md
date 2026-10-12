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
   what was done. *(Superseded in the fourth revision: that sequence used
   a fixed path and applied patch paths the examined commit does not hold.
   Use `repro/run-patches.sh`.)*
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

**Fourth revision note.** The review of `da5db735` found three defects,
and a recovery brief asked for a correction matrix covering every item
raised so far. This revision:
1. Rewrites FORGE-005 around three separate channels — registration,
   provider termination, and the provider-to-daemon data connection. The
   prerequisite is now an on-path network position, not a position inside
   the provider. Documentation is described as risk acceptance, not a fix.
   The remediation options are authenticated encryption on the data
   connection, or disabling the plaintext transport in favor of a secured
   ingress. Each comes with its own verifiable regression criterion.
   Reassignment of a stale destination stays unresolved.
2. Replaces the reproduction sequence with one executable workflow,
   `repro/run-patches.sh`, replayed as committed.
3. Completes FORGE-004's account of `keeper_remote`: four paths return
   raw `head_repo`, two of them after a successful parse.

It also adds the correction matrix below, prerequisite tables for the
unproved branch-input and `head_repo` paths, and forge documentation on
branch-name characters. Local git acceptance of such a name is now kept
apart from forge acceptance, which is not tested.

## Correction matrix

Every item raised by a review verdict or by the orchestrator's recovery
brief, with where this report answers it, the evidence, and what is still
uncertain. "Review N" counts the verdicts on `31bc2796`, `af1b251a`,
`1dc58060` and `da5db735`, in order.

| Item | Report location | Evidence | Remaining uncertainty |
|---|---|---|---|
| Review 1.1 — FORGE-001 shallow reproduction | FORGE-001 | Existing test `asking_keeps_a_request_of_mine_by_hand_until_the_merge`, run unchanged | No real agent tested; no GitLab-backed delivery test |
| Review 1.2 — overclaimed structural guarantees | Tracer 6-7; FORGE-008; FORGE-002 | `launcher.rs:263-349`; spec 002; spec 021 rule 9; EXEC evidence (attributed); `goal.rs:196-248` | Real deployments' permission modes unknown |
| Review 1.3 — fork URL rejection cited the wrong check | FORGE-004; Rejected hypotheses | `launcher.rs:1449-1492, 1546-1574`; `github/pulls.rs`, `gitlab/pulls.rs`; replayed patch | Whether a forge can report a cross-host `head_repo` |
| Review 1.4 / 3.3 / 4.1 — tunnel confidentiality | FORGE-005 Channels 1-3, Part B | `localtunnel-client` `lib.rs:329, 362-375, 379-412`; `tests/it/tunnel.rs` test run unchanged | Channels 1-2 not measured; stale reassignment unresolved |
| Review 1.5 — coverage, sub-review evidence, REVIEW_MARK | Coverage; FORGE-003 | File list; replayed FORGE-003 patch | Human reading of the mislabeled line not measured |
| Review 3.1 — authorship vs branch provenance | FORGE-007 | `pull_request_babysit.rs:609-639`; GitHub and GitLab docs | How common the collaboration prerequisite is |
| Review 3.2 — transcript missing initial commit | FORGE-007 transcript | Re-run; reviewer replay passed | Forge acceptance of the name not tested |
| Review 3.4 / 4.2 — reproduction isolation and workflow | Reproduction workflow; `repro/README.md`; `repro/run-patches.sh` | Two replays: exit 0, 2 passed, no work directory left | Clone and build steps rely on the outer time limit |
| Review 3.5 — stable identifiers | FORGE-002, FORGE-008 headers | Header and reference scan | None |
| Review 3.6 / 4.3 — `keeper_remote` fallbacks | FORGE-004 | `launcher.rs:1551-1573` | None for the code; reachability as in Review 1.3 |
| Review 3.7 — FORGE-003 skill claims | FORGE-003 | `pr-babysit/SKILL.md:19-29, 52` | None |
| Recovery 3 — registration separate from transport | FORGE-005 Channels 1 and 3 | As above | Channel 1 TLS not measured |
| Recovery 4 — private body confidentiality, both forges | FORGE-005 "What crosses that connection" | Source; committed test (GitHub-shaped); GitLab docs | GitLab case not run through the daemon's test |
| Recovery 6 — authorship separate from branch control | FORGE-007 | As Review 3.1 | As Review 3.1 |
| Recovery 7 — human actions for unproved paths | FORGE-007 table; FORGE-004 prerequisites | Code and docs cited in each step | Each step is marked proved or not |
| Recovery 9 — FORGE/EXEC/PERM boundaries | Cross-boundary evidence | Orchestrator's stated split | None |
| Recovery 10 — capability vs causation | FORGE-008; FORGE-001; FORGE-007 steps 5-6 | EXEC evidence (capability only) | Causation not shown anywhere |
| Recovery 11 — policy questions open | Compatibility decisions; follow-up preamble | — | All five questions open |
| Recovery 12-13 — edits only in develop; preserve the reviewed commit | Commit history | `da5db735` unchanged and an ancestor of this revision | None |

## Question

Can hostile forge traffic or content cause unauthorized local work, secret
exposure, or unintended forge actions?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (branch `main`). The task
worktree itself (`/Users/malvarez/.ariadne/worktrees/q4r9v3t3/k6n3kcgn-eng`)
was clean at the start and end of this revision; no application file in
it was ever touched. Instrumented reproductions run only through the
published workflow `repro/run-patches.sh`, which clones this repository
into a new `mktemp -d` directory, checks out
`a17283486d5fe89591edfff08832712936d73030`, verifies `HEAD`, applies the
patches there, and removes that directory on exit (see "Reproduction
workflow"). The existing-test evidence (FORGE-001, FORGE-005) ran
unchanged against this worktree, which matches the examined commit in
every application file. FORGE-007's git-ref-name check uses no
application code and ran in its own empty, throwaway repository.

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
  or external services. Every reproduction below is one of: a patched
  unit test run by `repro/run-patches.sh` in a fresh clone; an existing
  committed test run unchanged, including the project's own stand-in
  tunnel provider; or a local git-only check in its own throwaway
  directory. Everything else is a read of this sandbox's installed `gh`
  binary, the vendored `localtunnel-client` crate's source, and public
  GitHub/GitLab/RustSec documentation. All committed evidence uses
  synthetic values.

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
- `crates/ariadne-daemon/tests/it/pull_request_babysit.rs`, in full, to
  find and read
  `a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository` and its
  `Shown` fixture precisely.
- `crates/ariadne-daemon/tests/it/tunnel.rs`: the stand-in provider
  (lines 26-136, in particular the raw forwarding at 84-92), the `deliver`
  helper (498-528), and
  `a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`
  (549-599), the committed evidence for FORGE-005 Channel 3.
- Forge documentation fetched via `WebFetch` and cited where used:
  GitHub's REST "Create a pull request", issue-comment
  `author_association`, and "Dealing with special characters in branch and
  tag names"; GitLab's webhooks, project webhooks API, permissions,
  creating merge requests, branches, and CLI `api` pages.

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
  crate's own published documentation/source. In particular, TLS on tunnel
  registration and the provider's termination of the forge's HTTPS
  (FORGE-005 Channels 1 and 2) are not measured.
- **No forge push of a branch name holding shell-special characters**
  (FORGE-007). Forge acceptance is documented in part and not tested.
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

### FORGE-005 — Tunnel deliveries cross an unencrypted provider-to-daemon connection that any on-path observer can read; a stale hook keeps a never-rotated secret in use (Confirmed transport mechanism; stale-destination reassignment unresolved; High)

*(Corrected this revision. The prior text required "a position on the
tunnel provider's own infrastructure" and suggested provider-internal TLS
might protect the hop. Both were wrong: the plaintext socket is the
daemon's own outbound connection, it crosses whatever network separates
the provider from the daemon's host, and nothing inside the provider can
encrypt it. The prior remediation also listed documentation as a way to
"remove" the disclosure; documentation only accepts the risk.)*

The tunnel involves three distinct channels. Each has its own question.

**Channel 1 — Registration (daemon to `tunnel_host`).**
- `localtunnel-client-0.1.8/src/lib.rs:379-400` (`get_tunnel_endpoint`)
  builds `GET <tunnel_host>/<subdomain>` (lib.rs:385-386) and sends it with
  a `reqwest` client. The workspace pins the `rustls-tls` feature
  (`Cargo.toml:30`), and the default `tunnel_host` is
  `https://localtunnel.me` (`crates/ariadne-daemon/src/config.rs:120`,
  asserted at `:245`), so registration with the default host uses TLS.
- It carries the requested subdomain only: the daemon sets
  `credential: None` (`forge/tunnel.rs:382`), and no webhook secret or
  payload is ever sent on this channel.
- The answer chooses where the data connection goes: the client takes the
  host from the answer's URL minus its first label, and the port from the
  answer (`lib.rs:403-412`).
- No explicit redirect policy is set (`lib.rs:392-394`), so `reqwest`'s
  default applies. `tunnel_host` is operator configuration, not forge
  content.
- Limit: TLS on this channel follows from configuration and code. It was
  not measured against the real host, which this audit does not contact.

**Channel 2 — Provider termination of the forge's HTTPS (a trust question,
not a transmission question).** The forge connects to the public tunnel
URL. That HTTPS connection ends at the provider, which must read the
request to route it onto one of the client's connections. The project's
own stand-in does exactly this: it reads the request and writes it onto a
client connection (`tests/it/tunnel.rs:84-92`). That the real provider
behaves the same way is inferred from the protocol, not measured. Any fix
to Channel 3 leaves this in place: the provider is an endpoint of the
forge's TLS, so it sees every delivery in clear text. Whether to trust a
third-party relay with that is a decision for the user.

**Channel 3 — Provider-to-daemon data connection (transmission
confidentiality).**
- `lib.rs:329` (`tunnel_one_connection`): `TcpStream::connect(&addr)`, a
  bare TCP connection to the host and port the registration answer named.
- `lib.rs:362-375` (`proxy_through`): `io::copy_bidirectional` between that
  socket and the daemon's loopback relay, with no encryption, decryption,
  or integrity check.
- Grepping the whole crate for `tls|rustls|native_tls|ssl|TlsConnector|TlsStream`
  finds nothing outside the `reqwest` registration call.

This socket is the daemon's own outbound connection. It crosses the
network between the provider and the daemon's host in clear text. **An
on-path observer — anyone who can read traffic on that path, for example a
shared local network, the host's ISP, or a transit network — can read every
delivery's headers and body. The observer needs no position inside the
provider's infrastructure.** No TLS inside the provider can protect this
socket, because the encryption would have to be on the socket itself, and
this client opens it without any.

What crosses that connection, for each forge:
- GitHub: the private delivery body (for example comment text, pull
  request and commit identifiers) and `X-Hub-Signature-256`. The signature
  is an HMAC: it proves origin and integrity, but it encrypts nothing.
- GitLab: the private delivery body and `X-Gitlab-Token`, which *is* the
  webhook secret ("sent as plain text in the `X-Gitlab-Token` HTTP header",
  GitLab's own webhook documentation).

HMAC signing and secret rotation do not provide confidentiality on either
forge.

**Evidence for Channel 3.**
1. Source, as cited above.
2. An existing committed test, run unchanged this revision:
   `crates/ariadne-daemon/tests/it/tunnel.rs::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`.
   It runs the daemon's real `Tunnel`, loopback relay, `WebhookListen`, and
   `localtunnel-client`. Its stand-in provider forwards a signed GitHub
   delivery (signature header and body) as raw bytes onto the daemon's
   tunnel connection (`tunnel.rs:88`, `conn.sendall(...)`), and the daemon
   accepts it with 202 and fetches once. The daemon could not parse those
   bytes if the connection carried any encryption layer.
   ```
   cargo nextest run -p ariadne-daemon \
     -E 'test(a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once)' \
     --status-level fail --final-status-level all
   ```
   ```
   PASS [2.315s] (1/1) ariadne-daemon::it tunnel::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once
   Summary [2.316s] 1 test run: 1 passed, 964 skipped
   ```
   Log: `/tmp/forge-tunnel-existing-test.log` (outside the worktree).
3. An additional observation, not published as a reproduction: during the
   previous review, a standalone run of the same client crate with
   synthetic values only showed the data connection starting with a
   plaintext HTTP request line, and carrying a synthetic GitLab token and
   synthetic private bodies for both forges. This report relies on items 1
   and 2, which anyone can re-run from the repository.

**Limits.** The real `localtunnel.me` was not contacted. The committed test
sends a GitHub-shaped delivery only. The GitLab case rests on the same
transport code, which forwards bytes without reading headers, plus
GitLab's documentation that its token header carries the secret.

**Prerequisites for Channel 3.**
- The tunnel is in use: the tunnel switch is on, at least one integration
  is enabled, and no `webhook_public_url` is set (spec 027 rule 12; a set
  `webhook_public_url` opens no tunnel).
- A delivery is in flight.
- The observer can read packets on the network path between the provider's
  data port and the daemon's host.
- No provider compromise, stale destination, or subdomain squatting is
  needed.

**Severity and confidence.** High: the observer gets private repository
content on both forges, and on GitLab the webhook secret itself. A captured
GitLab secret then authorizes forged deliveries, whose effect is bounded to
extra fetches (Tracer point 1). Confidence is high for Channel 3 (source
plus a committed test of the real daemon components), and medium for
Channel 2 (inferred from the protocol, not measured).

**Part B — stale destination and no secret rotation (carried over;
reassignment explicitly unresolved).**

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
- **Unresolved, and not claimed:** whether another party can come to
  receive traffic for a subdomain the daemon used before. That depends on
  the provider's subdomain-reuse behavior after a disconnect. This audit
  did not test it and found no primary source. The report does not claim
  takeover of a stale destination.
- **What Part B adds to Channel 3:** Channel 3 already exposes the GitLab
  secret to an on-path observer while the tunnel works normally. Part B
  adds two things. Deliveries keep going to an address the daemon no
  longer holds, and the same secret stays valid against the current
  endpoint for as long as the forge identity is unchanged, however it was
  observed.

**Remediation options (the choice between them is the user's,
compatibility question 3):**
- *Transmission confidentiality (Channel 3)* — either of:
  1. **Authenticated encryption on the provider-to-daemon data
     connection**: for example TLS to the provider's data port, verifying
     the provider's certificate. `localtunnel-client` 0.1.8 offers no such
     option, so this needs a different tunnel client or protocol, and a
     provider that supports it. This audit did not establish whether
     `localtunnel.me`'s server supports it.
  2. **Disable the plaintext tunnel transport** and use a secured ingress
     instead: a `webhook_public_url` served by an HTTPS endpoint the
     operator controls, such as a reverse proxy in front of
     `webhook_listen` (spec 027 rules 1 and 12). This also removes the
     Channel 2 trust, because no third party terminates the forge's TLS.
- *Documentation* of the tunnel's exposure is **risk acceptance**. It tells
  the user about the disclosure; it removes none of it.
- *Part B* — any of: (a) delete or update the forge-side hook when an
  enabled integration loses its public URL; (b) rotate `webhook_secret`
  on every public-URL change, not only on an identity change; (c) use
  GitLab's HMAC `signing_token` in place of the legacy plaintext `token`.
  Option (c) stops the GitLab secret itself from crossing Channel 3. The
  private body still crosses it.

**Regression criteria (each independently verifiable):**
- *Transmission confidentiality, option 1:* extend the stand-in in
  `tests/it/tunnel.rs` so that a delivery carries a synthetic GitLab token
  and a synthetic private body, and record the bytes on the daemon's data
  connection at the socket. Assert that neither synthetic value appears in
  those bytes, and that the first bytes are not a plaintext HTTP request
  line. By the Channel 3 evidence, this assertion would fail against the
  examined commit (not run here: this audit adds no failing test), so it
  can tell a fixed transport from the current one. It belongs in the
  remediation task, not in this audit.
- *Transmission confidentiality, option 2:* with the tunnel switch on, an
  enabled integration and no `webhook_public_url`, assert that the daemon
  opens no plaintext data connection and registers no hook at a tunnel
  URL.
- *Part B:* a `webhooks.rs` test asserting that a tunnel-down transition on
  an enabled integration either deletes the forge-side hook or issues a
  fresh secret, and that a delivery signed with the earlier secret is then
  rejected.

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

**Reproduction** (patch `repro/forge-003-review-mark-spoof.patch`, run by
the published workflow `repro/run-patches.sh`; see "Reproduction
workflow"): a `NewPullRequestComment` with `author_login: "attacker"` and a
body ending in the review mark, run through the real `live::comments()`
then `news::of()`:
```
PASS [0.011s] (1/2) ariadne-daemon forge::news::tests::forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review
```
The test confirmed `from_review` became `true` for the
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
- Contrast `launcher.rs::keeper_remote`, lines 1546-1574 — **corrected
  again this revision; the prior text still called every successful-parse
  path safe, which is wrong.** It has one path that never uses
  `head_repo` and four that return it raw:
  - `head_repo` absent (1551-1553): the configured remote. Safe.
  - No forge row (1554-1556): **raw `head_repo`**.
  - `Remote::parse` fails (1557-1559): **raw `head_repo`**.
  - Parse succeeds and host, owner and name all match the configured forge
    (1560-1565): the configured remote. Safe.
  - Parse succeeds with any other repository (1566-1573): `keeper_remote`
    reads the configured remote's push URL and rewrites the fork's owner
    and name onto its authority with `Remote::rewrite`. That keeps the
    configured host only when both steps succeed. **If `remote_push_url`
    fails (1571), or `Remote::rewrite` returns `None` (1569-1570), it returns
    raw `head_repo`.**

  So a successful parse is not enough to reach a safe result. There is no
  host-mismatch *refusal* anywhere in `keeper_remote`: a host that differs
  from the forge's is either silently replaced by the configured authority
  (when both steps succeed) or passed through raw (when either fails).
  The result feeds both the keeper's fetch (`keeper_worktree`, 1513-1515)
  and its push command (`keeper_push_command`, 1579-1585). `keeper_remote`
  has more safe paths than `review_worktree`, which has none, but it does
  not already perform the guard this finding recommends.
- `head_repo` itself is populated verbatim from the forge API's own field
  with no host check anywhere upstream: `github/pulls.rs::normalized`
  (GraphQL `headRepository.url`) and `gitlab/pulls.rs::normalized`
  (`source_project.http_url_to_repo`).

**Reproduction** (patch `repro/forge-004-head-repo-no-validation.patch`,
run by the published workflow `repro/run-patches.sh`; see "Reproduction
workflow"): two local, throwaway git repositories stood in for a victim
checkout and a fork at an unrelated filesystem path.
`GitManager::fetch_head_of` accepted the fork's bare filesystem path — not
a URL, not even a URL scheme — as `from`, and fetched its commit:
```
PASS [0.157s] (2/2) ariadne-daemon gitwt::forge_004_reproduction::fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check
```
This establishes the absence of a guard conclusively; it does not
establish that a real forge response can ever actually carry a cross-host
value in this field, which remains unresolved (no primary source found
either way; see FORGE-007 below for the closely related, but distinct,
question of whether `head_repo` can differ in *ownership* — not
necessarily *host* — from what the request's author controls, which this
revision answered for GitHub and GitLab specifically).

**Prerequisites and human actions (path not proved):**
1. The user asks Ariadne to review a request (`PUT .../ariadne-review`;
   spec 029 rule 10 — a review never starts on its own), or to babysit
   one of their own (spec 032).
2. The registered checkout lacks the request's head commit, so a fetch is
   needed (`launcher.rs:1456`, `1510`).
3. The forge's response gives a `head_repo` on a host other than the
   integration's — not shown to be possible on either forge (see above) —
   or one that fails `Remote::parse`, on the keeper path.
4. The daemon then fetches from that value with no human step between. Any
   effect beyond reaching that host (credentials, agent probing) is
   unresolved, as stated above.

**Recommendation (smallest remediation):** add a **new** guard that
validates `head_repo`'s host against the integration's own `forge.host`
before `review_worktree` calls `fetch_head_of`, and at each of
`keeper_remote`'s four raw-`head_repo` returns (1555, 1558, 1570, 1571).
`keeper_remote` has no such guard today. What the guard does on a mismatch
— refuse, or fall back to the configured remote — is the user's decision
(compatibility question 4). **Regression criteria:** a `launcher.rs` test
asserting that `review_worktree` does not fetch from a `pull.head_repo`
whose host differs from the integration's configured host; and a test
asserting that `keeper_remote` never returns a `head_repo` with a foreign
host, with `remote_push_url` failing and with an unparseable `head_repo`.

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

**Confirmed, by local reproduction: git's own ref-name rules accept the
name `fix$(touch,pwned)`** (transcript below). Git stores the name; it runs
nothing.

**What the forges document about such names, and what is not
established.** Local git acceptance does not prove forge acceptance, so
this is stated separately:
- GitHub's documentation, "Dealing with special characters in branch and
  tag names" (`docs.github.com/en/get-started/using-git/dealing-with-special-characters-in-branch-and-tag-names`),
  names two restrictions beyond git's own: no 40-hex names, and no names
  beginning with `refs/`. It addresses branch names that hold characters
  special to a shell, gives `'hello-$USER'` as its example, and warns that
  unless such a name is quoted or escaped in a command, "the command may
  have unintended effects."
- GitLab's branch documentation (`docs.gitlab.com/user/project/repository/branches/`)
  enforces no spaces and no 40-hex names. It lists characters to avoid,
  among them `'`, `"`, and a leading `-`. It does not list `$`, `(`, `)`,
  `;`, `|`, or `&`. It recommends alphanumerics, hyphens and underscores.
- **Not established:** that either forge accepts a push of the exact name
  used here. No push to a real forge was made, per the task's constraints.

**Prerequisites and human actions, for each forge.** This attack path is
not proved. Each step below is a precondition, with who must act:

| Step | GitHub | GitLab | Who acts | Evidence |
|---|---|---|---|---|
| 1. A branch with a shell-special name exists in the head repository | Any account with write access to that repository pushes it | Any account with push access to the source project pushes it | A collaborator, possibly not the request's author | Git accepts the name (transcript); forge acceptance documented in part, not tested |
| 2. A request cites that branch, authored by the integration's login | The user opens the PR; needs write access to the head branch (GitHub REST docs, "Create a pull request") | The user opens the MR; needs Developer, Maintainer or Owner, or permission to create MRs (GitLab docs) | **The user** (human, authenticated) | Forge docs; `forge/pulls.rs::role` |
| 3. Ariadne is asked to babysit that request | `PUT .../ariadne-babysit` (CLI `pr babysit`, or the panel's Start babysit) | Same | **The user** (human) | spec 032 rule 7; `http/pull_requests.rs::ask_babysit` |
| 4. The keeper's briefing carries the name inside `Push with: git push <remote> HEAD:<branch>` | Built unescaped | Same | Daemon, automatically | `launcher.rs:1579-1585`, `defaults.rs:412-423` |
| 5. The keeper runs that text through a shell that interprets it | — | — | **The agent**, by its own decision | Not demonstrated; capability evidence only (EXEC, attributed) |
| 6. The permission gate lets the shell call run | `auto` runs it with no prompt; `ask` needs a human click; `learn`/`ai` may allow or ask | Same | PERM's policy; a human in `ask` mode | spec 021 rule 9; FORGE-008 |

Steps 2 and 3 always need the user. Step 1 can be another party's action.
Steps 5 and 6 are where capability (shown) and causation (not shown)
separate: EXEC's evidence shows an agent session *can* run shell commands
in its worktree, and nothing here shows that an agent *would* run this
text unquoted.

**What remains unresolved:**
1. How often a keeper-eligible request (the user's own,
   `role == "author"`) has a head repository the user does not control
   alone. The system models and tests this as supported
   (`a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository`);
   how common it is in real use is unknown.
2. Whether a collaborator who is not the request's author would choose a
   hostile branch name without the author noticing before step 3. This is
   a human-process question the code cannot settle.
3. Whether the forges accept the exact name (see above).

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

The one published workflow is the executable script
**`repro/run-patches.sh`**. It replaces the sequence the previous revision
printed here, which checked out the examined commit and then applied
`repro/*.patch` paths that commit does not hold, and which deleted a fixed
`/tmp` path. What the script does:
- It resolves the patch paths from its own directory before any clone or
  checkout, so the patches are read from the report directory.
- It creates its work directory and its log directory with `mktemp -d`, so
  each run's names are its own.
- It clones the repository into the work directory, checks out
  `a17283486d5fe89591edfff08832712936d73030`, verifies `HEAD`, and applies
  both patches there. It never touches a tracked worktree.
- It runs the two tests with a nextest configuration that ends any test
  after five minutes (`slow-timeout = { period = "60s", terminate-after = 5 }`).
  The whole script is meant to run under a limit of ten minutes or less;
  it was replayed under the runner's 600-second limit.
- It writes the test log to the log directory, outside the worktree, and
  keeps it.
- Its `EXIT` trap removes the work directory, and only that directory, on
  success, failure and interruption.

Replay of the script as committed, from the task worktree:
```
logs: /var/folders/.../T//forge-repro-logs.UbW09e
        PASS [   0.011s] (1/2) ariadne-daemon forge::news::tests::forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review
        PASS [   0.157s] (2/2) ariadne-daemon gitwt::forge_004_reproduction::fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check
     Summary [   0.157s] 2 tests run: 2 passed, 965 skipped
exit=0 elapsed=48s
```
After the run no `forge-repro.*` work directory remained, and
`git status --short` in the task worktree showed only the new script
itself, not yet committed at that moment. The run was done twice, with
the same result both times.

Not covered by the script: a time limit on the clone and build steps
themselves. Those rely on the outer limit under which the script is run.

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
  citations. **Rejected.** (Distinct from FORGE-005's Channel 3, which is
  about the provider-to-daemon connection's *confidentiality*, not where
  the loopback relay forwards to.)
- **CLI argument injection via a forge-controlled string starting with
  `-`.** Confirmed by sub-review across all 14 forge-client files: every
  such value sits in a fixed argv slot, and `Cli::run` never goes through
  a shell. **Rejected.**
- **GraphQL/JSON injection via forge-controlled comment bodies or paths.**
  GraphQL variables are passed structurally; review bodies are built with
  `serde_json::json!`. **Rejected.**
- **"The keeper's push target is protected by `Remote::rewrite`."** No
  longer rejected as a hypothesis, and not a separate finding either: it
  is folded into FORGE-004. Only two of `keeper_remote`'s paths avoid
  `head_repo` entirely (no `head_repo`, or an exact match with the
  configured forge). One more path keeps the configured host (a successful
  parse where `remote_push_url` and `Remote::rewrite` both succeed). The
  other four return raw `head_repo`, two of them after a successful parse.
  See FORGE-004 for the line-by-line account.

## Dependency advisories

Unchanged: `cargo audit` against the full workspace `Cargo.lock` (557
crates, 1307 advisories) reports **0 vulnerabilities**; two unrelated
maintenance warnings (`bincode` via `syntect`, `chacha20` via
`rand`→`ulid`), neither reachable through the webhook/forge/tunnel
dependency subtree. This includes `localtunnel-client` 0.1.8 itself.
FORGE-005's Channel 3 is a design property of that crate, found by
reading its source and confirmed by the project's own tunnel test. It is
not a known advisory, so `cargo audit` would not report it.

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

**What the fourth round changes:** FORGE-005's prerequisite was too narrow
and its remediation too soft. The plaintext socket is the daemon's own
outbound connection, so an on-path network observer reads it without any
position inside the provider. Documentation was presented as a fix when
it is only risk acceptance. Both are corrected, and the three tunnel
channels are now analyzed separately. FORGE-004's account of
`keeper_remote` now covers all four raw-`head_repo` returns. The
reproduction workflow is a single published script, replayed as
committed. See the correction matrix for every item.

**High confidence**, by direct code citation and EXEC's independent
corroboration: FORGE-008's capability facts. (Causation by forge content
is not shown.) **High confidence**, by source reading plus the project's
own committed tunnel test run unchanged against the real daemon
components: FORGE-005 Channel 3. **Medium confidence**, by inference from
the protocol, not measured against the real provider: FORGE-005 Channel 2.
**High confidence**, by code reading and GitLab's documentation: FORGE-005
Part B's stale hook and unrotated secret. Reassignment of a stale
destination to another party is **unresolved**. **High confidence**, by
the published workflow's replay: FORGE-003 (narrowed) and FORGE-004's
missing guard. **High confidence**, by an existing committed test:
FORGE-001's structural gap. **High confidence that the hazard is real,
with reachability unresolved**: FORGE-007. The existing fork test and both
forges' documentation show that authorship and branch provenance are
separate. How often real deployments meet the prerequisites is not
established.

**Lower confidence, named explicitly:**
- FORGE-008's practical severity depends entirely on what permission mode
  real deployments actually run. **What would change this:** evidence of
  the actual distribution of `auto` vs. `ask`/`learn`/`ai` across
  forge-integrated repositories.
- FORGE-005 Channel 2 (provider termination) is inferred from the
  protocol. **What would change this:** the provider's own architecture
  documentation. Note that no provider-side fact can change Channel 3: the
  plaintext socket is the daemon's own.
- FORGE-005 Part B's reassignment risk depends on the provider's
  subdomain-reuse behavior. **What would change this:** the provider's
  published reuse policy.
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
3. **For FORGE-005**:
   - Channel 3: should the data connection get authenticated encryption,
     which needs a tunnel client and provider that support it, or should
     the plaintext tunnel transport be disabled in favor of a secured
     ingress (an operator-controlled HTTPS `webhook_public_url`)? Or does
     the user accept the disclosure as documented risk, knowing that
     documentation removes none of it?
   - Channel 2: is trusting a third-party relay that terminates the forge's
     HTTPS acceptable at all?
   - Part B: is rotating the webhook secret on every public-URL change an
     acceptable operational cost, or is deleting the stale hook on URL loss
     preferred, or both? Should GitLab integrations move to the HMAC
     `signing_token`, which needs GitLab 19.0 or later?
4. **For FORGE-004**: when the new host guard finds a `head_repo` that is
   unparseable or on a foreign host, should it refuse — so no worktree is
   made and the session does not start — or fall back to the configured
   remote? Today `keeper_remote` does neither: it returns raw `head_repo`
   at four places (1555, 1558, 1570, 1571).
5. **For FORGE-007**: given that a keeper-eligible request's head branch
   is not guaranteed to be the trusted user's own choice, should Ariadne
   refuse to babysit a request whose `head_repo` differs from the
   repository's own configured remote at all (closing the fork-collaboration
   scenario entirely, at the cost of the feature `a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository`
   exists to support), or should the fix be narrower (validate/escape the
   branch name, leaving cross-fork babysitting itself intact)?

## Proposed follow-up tasks

Each policy choice below stays open until the user answers the matching
compatibility question. No task here assumes an answer.

- A remediation task for FORGE-008, **owned by PERM**, per compatibility
  question 1.
  - Scope: the ACP permission-policy decision for pull-request seats
    (specs 021/022).
  - Excluded: Ariadne's own pull-request routes, which already refuse
    approve and merge; EXEC's execution-capability boundary.
  - Acceptance: the FORGE-008 regression criterion for the option chosen.
- A remediation task for FORGE-001 and FORGE-003 together (shared files),
  per compatibility question 2.
  - Scope: `forge/live.rs`, `forge/news.rs`, the GitHub and GitLab comment
    readers, and the `pr-babysit` skill text.
  - Excluded: displayed forge content (VIEW).
  - Acceptance: the FORGE-003 attribution test promoted into the suite, and
    the extended `asking_keeps_a_request_of_mine_by_hand_until_the_merge`
    test asserting different treatment of an untrusted author.
- A remediation task for FORGE-005, per compatibility question 3.
  - Scope: `forge/tunnel.rs` (the transport choice for Channel 3),
    `forge/hooks.rs` and `forge/gitlab/hooks.rs` (Part B).
  - Excluded: the tunnel provider's own server, and the daemon's loopback
    relay, which is already loopback-only.
  - Acceptance: the Channel 3 regression criterion for the option chosen,
    and the Part B `webhooks.rs` criterion, both as stated in FORGE-005.
- A remediation task for FORGE-004, per compatibility question 4.
  - Scope: the new host guard before `review_worktree`'s fetch and at
    `keeper_remote`'s four raw returns.
  - Excluded: the git-command layer in `gitwt.rs` (EXEC).
  - Acceptance: the two regression criteria in FORGE-004.
- A remediation task for FORGE-007, per compatibility question 5.
  - Scope, FORGE's part: validate `head_branch` at ingestion
    (`forge/pulls.rs`).
  - Excluded: safer command construction in `keeper_push_command` (EXEC),
    and any gating of shell calls (PERM). Each is its owner's choice.
  - Acceptance: the FORGE-007 regression criteria, including the
    fork-fixture test with a hazardous branch name.
- (Optional, low priority) A task for FORGE-002.
  - Scope: the CLI `goal create --from-issue` path.
  - Excluded: the desktop dialog (VIEW) and the orchestrator seat.
  - Acceptance: a CLI test asserting that the fetched title and body are
    shown and confirmed before the goal is created.
- (Optional) A short verification task to install `glab` and confirm its
  documented behavior against the real binary.
- No application fix is included in this change; this report is evidence
  and recommendation only, per the task's constraints.

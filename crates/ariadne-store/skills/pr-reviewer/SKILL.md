---
name: pr-reviewer
description: Review one open forge request and post one inline finding per defect, P0 to P2. Use for a live reviewer session on a request.
---

# PR reviewer

Review the open request. Complete one round, then end your turn. Write the
verdict first: request changes, comment, or nothing new to say. No
preamble, no narration, no closing summary.

## Steps

1. Call `get_pull_request` and `get_diff`. On a later round, use `since`
   only if the old revision remains an ancestor. If `get_diff` refuses
   `since`, review the full diff and reuse no old evidence. Read
   `supervise_posts` before you post or resolve a thread.
2. On a later round, call `list_comments` first. For each thread you
   opened:
   - Verify a claimed fix at the current head.
   - Outside supervised mode, call `resolve_thread` without a reply.
   - In supervised mode, leave a fixed thread for the user.
   - When a claimed fix fails, reply once with `Still there: <why>.` Do
     not reply again in that thread.
   - On `Not changed:` or another unfixed answer, post nothing.
   - Leave every other thread open for its author.
3. Reuse evidence only when it covers the reviewed head and a required
   check. Run a check with evidence missing, outdated or insufficient.
   Record its command, result and the checked revision. Read code outside
   the diff only to judge a risk you can name.
4. Read the request description. Hunt a defect in behavior, errors, races,
   boundaries, security, data loss, tests and required behavior.
5. Give every finding a trigger, the input or state that causes it, and a
   priority:
   - P0: a critical risk to behavior, data, or security.
   - P1: a proven defect, unmet requirement, or repository-rule violation.
   - P2: an optional improvement.
   Mark a finding with no trigger you can name P2. Cap new P2 findings at
   five, the ones you have most conviction in.
6. Write every new finding as one inline comment on the line of the
   defect. The line is on the new side and shown by the diff. Give it a
   title, the trigger, the failure and the fix. Use a ```suggestion block
   for a one-line fix.
7. Call `submit_review` once with only new findings. Use `request_changes`
   for an active P0 or P1 finding where the forge permits it. Otherwise
   use `comment`. Do not call `submit_review` with no new findings.
8. Call `report_pull_request` with `reviewed_sha` set to the head you
   reviewed.

## Rules

- Give no approval. The user gives it.
- Never land the request.
- Never post a second comment for a defect.
- Resolve only a thread you opened.
- Never post a review summary, status comment, summary draft, or
  resolution draft.
- Reach the forge only through your session tools.
- A tool failure posts nothing another way. Say what failed.
- Flag a defect only where it breaks behavior, a test, or a written rule.

## Supervised repositories

A supervised repository holds replies and reviews in the user's forge
drafts. `pending: true` means held, not failed. The user publishes drafts.

When the user asks to change a pending post, call `edit_pending_comment`,
`delete_pending_comment`, or `edit_pending_review_body`. Do not post a new
reply or review instead. A 409 means the user already published it.

Read `list_comments` for a pending finding id before editing or deleting
it. `edit_pending_review_body` replaces the user's pending review body
whole.

## Do not tell yourself

- "Nobody answered, so I say it again." -> Wait for an answer.
- "It said Fixed, so the thread is done." -> Verify the current head.
- "The tool failed, so I use another forge command." -> Say what failed.
- "The round found nothing, so I note the request looks fine." -> Post
  nothing. Silence is the verdict.

## Done

Each new finding names its trigger, failure and fix, and is inline. New
P2 findings stay at five or fewer. Resolve fixed threads silently outside
supervised mode. Report the reviewed revision.

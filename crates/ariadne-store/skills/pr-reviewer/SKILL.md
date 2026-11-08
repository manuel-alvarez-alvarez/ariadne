---
name: pr-reviewer
description: Use when an open forge request needs a live reviewer session.
---

# PR reviewer

Complete the pr-reviewer work. Done when the conditions in `## Done` hold.

Review the open request. Complete one round, then end your turn. Write the
verdict first: request changes, comment, or nothing new to say. No
preamble, no narration, no closing summary.

## When not to use

Use `code-review` when its triggers apply.

## Steps

1. Call `get_pull_request` and `get_diff`. On a later round, use `since`
   only if the old revision remains an ancestor. If `get_diff` refuses
   `since`, review the full diff and reuse no old evidence. Read
   `supervise_posts` before you post or resolve a thread.

   Done when the current diff and supervision mode are known.
2. On a later round, call `list_comments` first. For each thread you
   opened:
   - Verify a claimed fix at the current head.
   - Outside supervised mode, call `resolve_thread` without a reply.
   - In supervised mode, leave a fixed thread for the user.
   - When a claimed fix fails, reply once with `Still there: <why>.` Do
     not reply again in that thread.
   - On `Not changed:` or another unfixed answer, post nothing.
   - Leave every other thread open for its author.

   Done when each owned thread has a disposition.
3. Reuse evidence only when it covers the reviewed head and a required
   check. Run a check with evidence missing, outdated or insufficient.
   Record its command, result and the checked revision. Read code outside
   the diff only to judge a risk you can name.

   Done when each required check has current evidence.
4. Read the request description. Hunt a defect in behavior, errors, races,
   boundaries, security, data loss, tests and required behavior.

   Done when the required defect classes have been examined.
5. Give every finding a trigger, the input or state that causes it, and a
   priority:
   - P0: a critical risk to behavior, data, or security.
   - P1: a proven defect, unmet requirement, or repository-rule violation.
   - P2: an optional improvement.
   A finding with no trigger you can name is P2. Mark it unverified. Cap
   new P2 findings at five, the ones you have most conviction in.

   Done when each finding has a trigger and priority.
6. Write every new finding as one inline comment on the line of the
   defect. The line is on the new side and shown by the diff. Give it a
   title, the trigger, the failure and the fix. Use a ```suggestion block
   for a one-line fix.

   Done when each new finding has one inline comment.
7. Call `submit_review` once with only new findings. Use `request_changes`
   for an active P0 or P1 finding where the forge permits it. Otherwise
   use `comment`. Do not call `submit_review` with no new findings.

   Done when the review has been submitted when needed.
8. Call `report_pull_request` with `reviewed_sha` set to the head you
   reviewed.

   Done when the reviewed revision has been reported.

## Rules

### Rules

- Never land the request.
- Never post a second comment for a defect.
- Resolve only a thread you opened.
- Never post a review summary, status comment, summary draft, or
  resolution draft.
- Reach the forge only through your session tools.
- A tool failure posts nothing another way. Say what failed.
- Flag a defect only where it breaks behavior, a test, or a written rule.

### Supervised repositories

When the user asks to change a pending post, call `edit_pending_comment`,
`delete_pending_comment` or `edit_pending_review_body` instead of
`submit_review` or `resolve_thread`. Read `list_comments` for the pending
finding's forge id first.

## Never

Give no approval. The user gives it.

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

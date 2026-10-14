---
name: pr-reviewer
description: Review one open forge request and post one inline comment per finding, by priority.
---

# PR reviewer

Review the open request. Complete one round, then end your turn.

## Steps

1. Call `get_pull_request` and `get_diff`. On a later round, use `since` only
   if `git merge-base --is-ancestor <reviewed_sha> HEAD` succeeds. Otherwise,
   review the full diff and do not reuse old evidence. Read `supervise_posts`
   before you post or resolve a thread.
2. On a later round, call `list_comments` first. For each thread you opened:
   - Verify a claimed fix at the current head.
   - Outside supervised mode, call `resolve_thread` without a reply.
   - In supervised mode, leave a fixed thread for the user.
   - When a claimed fix fails, reply once with `Still there: <why>.`.
     Do not reply again in that thread.
   - Leave unanswered and other unfixed threads open.
3. Reuse evidence only when it covers the reviewed head and required checks.
   Run the affected checks when evidence is missing, outdated, or insufficient.
   Run the required checks and record the command and result. Record the checked revision.
4. Read the request description. Hunt defects in behavior, errors, races,
   boundaries, security, data loss, tests, and required behavior.
5. Give every finding a priority:
   - P0: a critical risk to behavior, data, or security.
   - P1: a proven defect, unmet requirement, or repository-rule violation.
   - P2: an optional improvement.
6. Write every new finding as one inline comment on the line of the defect.
   Include a title, failure, and fix. Use a ```suggestion block for a one-line fix.
7. Call `submit_review` once with only new findings. Use `request_changes` for
   an active P0 or P1 finding where the forge permits it. Otherwise use `comment`.
   Do not call `submit_review` when there are no new findings.
8. Call `report_pull_request` with `reviewed_sha` set to the head you reviewed.

## Rules

- Give no approval. The user gives it.
- Never land the request.
- Never post a second comment for a defect.
- Resolve only a thread you opened.
- Never post a review summary, status comment, summary draft, or resolution draft.
- Reach the forge only through your session tools.

## Done

Each new finding is inline and includes a priority, failure, and fix. Resolve
fixed threads silently outside supervised mode. Report the reviewed revision.

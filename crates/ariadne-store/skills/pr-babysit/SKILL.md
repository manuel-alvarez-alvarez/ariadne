---
name: pr-babysit
description: Keep the request your task opened moving until a human merges or closes it. Answer comments, fix checks, keep the branch current.
---

# PR babysit

Your task is yours until a human merges or closes its request. Ariadne
reads the forge for you. It wakes you with the news: new comments with
their ids, failed checks, a base branch ahead of the head, a new review
decision, or a new state. Handle the news, then end your turn.

## Steps

1. Call `get_pull_request` for the branches and the failed checks. Call
   `list_comments` with `unanswered_only`.
2. A comment that asks for no change: call `reply_comment` once. Answer
   it, or give your reason.
3. A change a comment asks for, a failed check, or a base ahead of the
   head: fix it on your branch as a new commit. Read a check's log at its
   URL. Take in the base with `git merge --no-edit <remote>/<base>`. Run
   the tests and the lint of what changed. Then call `request_review`.
   Push nothing yet.
4. The reviewers approve: Ariadne tells you. Push the branch plainly.
   Then call `reply_comment` once for each comment you changed code for.
5. Never amend, rebase or force a push.
6. Call `report_pull_request` with `ready: true` once every required
   approval and check reads green. Call it with `ready: false` when a later
   change turns one back. Report only a change.
7. The request merged: run `git -C <repo> fetch <remote> <base>:<base>`,
   or `merge --ff-only <remote>/<base>` where `<repo>` is on `<base>`.
   Then call `finish_task` with `git -C <repo> rev-parse <base>`.
8. The request closed and did not merge: call `fail_task` with the reason.

## Rules

- Never merge the request. A human merges it.
- Leave every thread open. A human closes a thread; you reply to it.
- Reach the forge only through your session's tools: no forge CLI, no other
  forge tool. A tool fails: post nothing another way. Say what failed.
- Never wait on a timer, and never check the request on your own.
- End your turn when the news is handled.

## Do not tell yourself

- "This comment is wrong, so I can skip it." -> Reply with your reason.
- "The fix is small, so the reviewers need not see it." -> Every change
  goes through `request_review` before you push it.
- "A force push makes the history clean." -> Push only forward.
- "I will look at the request again soon." -> End your turn. Ariadne wakes
  you with the next news.

## Done

Every comment in the news has one reply. Every fix went through the
reviewers before you pushed it. The last report matches the real approvals
and checks. Your turn has ended.

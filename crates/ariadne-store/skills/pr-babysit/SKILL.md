---
name: pr-babysit
description: Open and keep the task request until it merges or closes. Answer each comment once, fix checks, and keep the branch current.
---

# PR babysit

Keep this task until the request merges or closes. On your first turn,
with no request open, do step 1. With one already open, start at step 2.
On each later turn, Ariadne sends news: do steps 2 to 8. Then end your
turn.

## Steps

1. Push the task branch. Call `open_pull_request`. Take the title from the
   repository's commit conventions. Fill its pull request template, or
   write three full sections: Why, What changed, How to test.
2. Call `get_pull_request`. Call `list_comments` with `unanswered_only`.
3. Answer each comment of the other side, once per comment:
   - An ask for a change: make it in a new commit, then run the checks of
     what you changed. Push it. Reply `Fixed in <sha>: <what changed>.`
     Or reply `Not changed: <why>.`
   - A question: give one short answer.
   - Thanks or an acknowledgement: reply nothing.
   - A follow-up ask in a thread you answered: a new ask. Act on it.
   - A `Still there:` reply from the reviewer: no second try. The user
     decides.
4. A finding with a suggestion block: apply the block. Write your own code
   only where the block does not compile or pass.
5. Fix each failed check in a new commit. Read its log at its URL. Where
   the base is ahead, run `git merge --no-edit <remote>/<base>`. Run the
   checks, then push.
6. Call `report_pull_request` only when readiness changes:
   - `ready: true` with the `head_sha` you read once every approval and check is green
   - `ready: false` when one goes back.
7. On a merge, where you sit on a task, update `<base>` in the primary
   checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is
     checked out
   - else `git -C <repo> fetch <remote> <base>:<base>`.
   Call `complete_step` with the merge as the reason. Else end your
   turn: Ariadne ends your session.
8. On a close, where you sit on a task, call `fail_task` with the reason.
   Else end your turn.

## Rules

- Never merge the request. Never amend, rebase or force a push.
- Commit only what a comment or a failed check asks for.
- Leave every thread open.
- Post no new thread, summary or top-level comment.
- Read and write the request only with your session's tools. When a tool
  fails, report it and post nothing another way.

## Do not tell yourself

- "I will check the request myself, to be sure." -> Ariadne wakes you
  with its news. A check of your own finds nothing it did not say.
- "A second reply gets the user's attention faster." -> `Still there:`
  means the user decides next. A retry is not your call.
- "My reply fixed it, so the thread is done." -> Closing a thread out is
  the user's to do. Leave every thread open.
- "This is close to the suggestion, let me write it my way." -> Apply the
  block. Write your own code only where it fails.
- "A thanks deserves a reply." -> A reply to a reply with nothing to
  answer starts a loop neither side ends.

## Done

Each comment that waits on you has one answer, in its shape. A thanks or
a `Still there:` gets none. Each fix is pushed after its checks pass. The
report matches the current approvals and checks. The merge or the close
ends the task; else your turn ends.

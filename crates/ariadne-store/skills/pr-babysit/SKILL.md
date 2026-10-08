---
name: pr-babysit
description: Keep one open pull or merge request of the user moving. Answer its comments, fix its checks and keep its branch current.
---

# PR babysit

Keep one open request of the user moving until a human merges or closes
it. Ariadne reads the forge for you. It wakes you with a prompt that lists
the news: new comments with their ids, failed checks, a base branch ahead
of the head, a new review decision, or a new state. Handle the news, then
end your turn.

## Steps

1. Call `get_pull_request` for the worktree, the branches and the failed
   checks. Call `list_comments` with `unanswered_only`.
2. Read each unanswered comment. Apply the fix it asks for, or reject it
   with a reason. Then call `reply_comment` once for it: say what you
   changed, or why the code stays.
3. A failed check: read its log at the URL `get_pull_request` gives. Fix
   the cause in the worktree. Run the tests and the lint of what changed,
   in the foreground. Commit, then `git push`.
4. A base branch ahead of the head: fetch the base, then run
   `git merge --no-edit <remote>/<base>`. Fix the conflicts, commit, and
   push plainly.
5. Grow the branch only forward. Write each fix as a new commit. Never
   amend, rebase or force a push.
6. Call `report_pull_request` with `ready: true` once every required
   approval and check reads green. Call it with `ready: false` when a later
   change turns one back. Report only a change.
7. The request merged or closed: call `report_pull_request` with that
   `state`. Your work ends there.

## Rules

- Never merge the request. A human merges it.
- Leave every thread open. A human closes a thread; you reply to it.
- Reach the forge only through the tools of your session. Run no forge CLI.
- Never wait on a timer, and never check the request on your own. Ariadne
  wakes you with the next news.
- End your turn when the news is handled.

## Do not tell yourself

- "This comment is wrong, so I can skip it." -> Reply to it with your
  reason.
- "The thread is answered, so I can close it." -> A human closes a thread.
- "A force push makes the history clean." -> The readers of the request
  read the commits you pushed. Push only forward.
- "I will look at the request again soon." -> End your turn. Ariadne wakes
  you with the next news.

## Done

Every comment in the news has one reply. Every failed check has a fix
pushed. The branch holds the base. The last report matches the real
approvals and checks. Your turn has ended.

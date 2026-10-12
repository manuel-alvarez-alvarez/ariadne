---
name: pr-babysit
description: Open and keep the task request until it merges or closes. Answer comments, fix checks, and keep the branch current.
---

# PR babysit

Keep this task until the request merges or closes. On your first turn, do
step 1. On each later turn, Ariadne sends news: do steps 2 to 7. Then end
your turn.

## Steps

1. Push the task branch. Call `open_pull_request`. Take the title from the
   repository's commit conventions. Fill its pull request template, or
   write three full sections: Why, What changed, How to test.
2. Call `get_pull_request`. Call `list_comments` with `unanswered_only`.
3. For each thread that asks for a change:
   - make the change in a new commit, run the checks of what you changed, and push;
   - reply once in the thread with the change and the commit.
   Or reply once in the thread with why you do not make it.
4. Fix each failed check in a new commit. Read its log at its URL. Where
   the base is ahead, run `git merge --no-edit <remote>/<base>`. Run the
   checks, then push.
5. Call `report_pull_request` only when readiness changes:
   - `ready: true` with the `head_sha` you read once every approval and check is green
   - `ready: false` when one goes back.
6. When the request merges, update `<base>` in the primary checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is checked out
   - else `git -C <repo> fetch <remote> <base>:<base>`.
   Call `complete_step` with the merge as the reason.
7. When the request closes without a merge, call `fail_task` with the
   reason.

## Rules

- Never merge the request. Never amend, rebase or force a push.
- Post only one reply in a thread that asks for a change. Post no new
  thread, summary, top-level comment or thanks.
- Commit only what a comment or a failed check asks for.
- Leave every thread open.
- Read and write the request only with your session's tools. When a tool
  fails, report it and post nothing another way.
- Do not wait on a timer or check the request on your own.

## Done

Each thread that asks for a change has one reply. Each fix is pushed after
its checks pass. The report matches the current approvals and checks.

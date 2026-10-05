---
name: pull-request
description: Open, defend and land a pull or merge request. Use for a pull_request task, or the final task of a feature_branch goal.
---

# Pull request

Own one request from its first push to a human's merge. The landing
briefing names your branch, the base branch and the repository; read it
first.

## Steps

1. Call `get_task`. A `pr_url` already on it is yours: skip to step 5 and
   resume polling. Never open a second request for one task.
2. Read the `origin` remote with `git remote -v`. github.com takes `gh`,
   GitLab `glab`. Neither, or `auth status` shows no account: `fail_task`
   with the failed check.
3. An ordinary task rebases onto the base branch once, before the push.
   The final task of a `feature_branch` goal works on the goal branch
   itself and skips the rebase: every other task already landed on it.
4. Push your branch. Open the request against the base branch with
   `gh pr create` or `glab mr create`. Title it by the repository's commit
   conventions. Fill its template. Call `record_pull_request` with the URL.
5. Poll the request and its comments with `gh pr view` or `glab mr view`.
   Sleep up to 300 seconds between polls, never longer in one call. Never
   end your turn while it is open.
6. Answer every comment you have not answered yet. Keep a note of which
   one you answered, so a comment you already covered waits for nothing
   more from you.
7. Fix a failed check on your branch. Run the tests and the lint of what
   you changed, once, before you push the fix.
8. Commit a requested change on your branch. Call `request_review`. Push
   it only once Ariadne approves it.
9. After the push, grow the branch only forward. Write a later fix as a
   new commit. Never rewrite one already pushed, and never rebase or
   force the push again. A merge that stops going cleanly: merge the base
   branch in with `git merge --no-edit` and push plainly.
10. Read the request's current approvals and checks on every poll. Call
    `record_pull_request` again with `ready: true` once every required
    approval and check reads green.
11. A later poll that finds a failed check, or a new commit that reopened
    one: call `record_pull_request` again with `ready: false`.
12. Never merge it. Wait for a human. A request closed unmerged ends the
    task: `fail_task` with that.
13. Merged: fetch and fast-forward the base branch in the primary
    checkout, `git merge --ff-only`.
14. The final task only: detach your worktree from the goal branch, then
    delete it, local and remote.
15. `finish_task` with the base branch's sha.

## Rules

- Resume an existing request by its recorded URL; never open a second one.
- Report the URL once. Report readiness only on a change from the last
  report.
- A human merges every request. Never run `gh pr merge` or `glab mr merge`.
- Answer each comment once. A comment your last reply already covers is
  not asked again.
- Push a revision only once Ariadne approved it.

## Do not tell yourself

- "The thread is quiet, so I can sleep longer." -> Cap every sleep at 300
  seconds, or the daemon reads the session as stalled.
- "It is still green, so last poll's answer still holds." -> Read the
  current approvals and checks before every ready report.
- "One more push, then I will ask for review." -> Push only what Ariadne
  already approved.
- "It merged, so the task is done." -> Fast-forward the base branch and
  report its sha before `finish_task`.

## Done

The request is open under the recorded URL. Every comment is answered.
Every revision passed Ariadne review before it was pushed. The last report
matches the request's real approvals and checks. A human merged it, the
base branch is fast-forwarded, and `finish_task` carries its sha.

---
name: pr-babysit
description: Use when a task request needs upkeep until merge or close.
---

# PR babysit

Complete the pr-babysit work. Done when the conditions in `## Done` hold.

On your first turn, with no request open, do step 1. With one already open,
start at step 2. On each later turn, Ariadne sends news: do steps 2 to 8.
Then end your turn.

A base-only linear-history rule does not require a linear task branch.
For Ariadne, merge main into PR branches and push normally. Never rebase published commits.
Never force-push main. Use the lease path below only where the repository explicitly permits it.
For Ariadne, require GitHub CI on the current head before a GitHub squash merge.

## When not to use

Use `merge` when its triggers apply.

## Steps

1. Push the task branch. Call `open_pull_request`. Take the title from the
   repository's commit conventions. Fill its pull request template, or
   write three full sections: Why, What changed, How to test.

   Done when the request is open.
2. Call `get_pull_request`. Call `list_comments` with `unanswered_only`.

   Done when the request and unanswered comments are read.
3. Answer each inline review comment of the other side, once per comment:
   - An ask for a change: accept it, or reply `Not changed: <why>.`
   - A question: give one short answer.
   - Thanks or an acknowledgement: reply nothing.
   - A follow-up ask in a thread you answered: a new ask. Act on it.
   - A reviewer reply that keeps a finding open: no second try. The user
     decides.
   - A comment on the conversation: answer it the same way, once per comment.

   Done when each comment has its reply or disposition.
4. A suggestion block you accept: apply it. Write your own code only where it
   does not compile or pass. Else reply `Not changed: <why>.`

   Done when each accepted suggestion has a disposition.
5. Group related accepted changes and failed-check fixes in one
   coherent commit per batch; keep unrelated changes separate.

   Done when each batch has one commit.
6. The repository's history rules choose the branch update. Fetch the
   base and task branch, then integrate before the checks. Where
   merges are allowed, run `git merge --no-edit <remote>/<base>`. A task-branch
   linear-history rule requires `git rebase <remote>/<base>`. Keep every
   commit others added. Run the checks the batch affects, then push once.
   After a merge, push only a fast-forward. After a rebase, run
   `git push --force-with-lease=<branch>:<sha> <remote> HEAD:<branch>`, with
   `<sha>` the fetched tip. Report a refused push; do not push again.
   Force-push no other way or branch. Never amend a pushed commit outside
   this rebase. A rejected request or a question keeps its step 3 reply.
   Reply once to each comment whose accepted change or suggestion the batch
   includes: `Fixed in <sha>: <what changed>.`

   Done when the batch is checked and pushed.
7. Call `report_pull_request` only when readiness changes:
   - `ready: true` with the `head_sha` you read once every approval and
     check is green
   - `ready: false` when one goes back.

   Done when the report matches readiness.
8. On a merge, where you sit on a task, update `<base>` in the primary
   checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is
     checked out
   - else `git -C <repo> fetch <remote> <base>:<base>`.
   Call `complete_step` with the column report. Else end your turn:
   Ariadne ends your session. On a close, where you sit on a task, call
   `fail_task` with the reason. Else end your turn.

   Done when the merge or close has its disposition.

## Rules

### Rules

- Commit only what a comment or a failed check asks for.
- Post no new thread, summary or top-level comment, except steps 3 and 6's
  replies.
- Use only your session's request tools. On failure, report it; post
  nothing another way.

### Supervised repositories

A supervised repository holds your reply in the user's forge drafts.
`pending: true` means waiting, not failed. Go on.

Call `edit_pending_comment`, `delete_pending_comment` or
`edit_pending_review_body` for a user-asked change. Never post a new reply
instead.

## Never

Never merge the request. Leave every thread open.

## Do not tell yourself

- "My reply closes it." -> The user closes it. Leave it open.
- "A thanks needs an answer." -> Do not start a loop.

## Done

Each actionable comment has one answer; every fix reply names its final pushed
commit. A thanks or kept-open finding gets none. Each batch is checked and
pushed once. The report matches approvals and checks. The merge or close ends
the task; else your turn ends.

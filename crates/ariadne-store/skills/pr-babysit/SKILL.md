---
name: pr-babysit
description: Open and keep the task request until it merges or closes. Answer comments, fix checks, and keep the branch current.
---

# PR babysit

Keep this task until the request merges or closes. Ariadne reads the
request and sends you its news. Handle the news, then end your turn.

## Steps

1. Push the task branch plainly. Call `open_pull_request`. Write the title
   from the repository's commit conventions. Fill the repository's pull
   request template if it has one, else write the body with three
   sections: Why, What changed, How to test. Write each section from the
   task and its diff, not one line each.
2. Call `get_pull_request` for the branches and checks. Call `list_comments`
   with `unanswered_only`.
3. Handle each existing thread that asks for a change.
   - Make the requested change in a new commit. Run the tests and lint of
     what changed. Push the branch plainly. Reply once in that thread.
     State the change and the commit.
   - Reply once in the thread when you do not make the change. State why you
     did not make the change.
4. Fix each failed check on your branch as a new commit.
   Read a failed check's log at its URL. If the base is ahead, run
   `git merge --no-edit <remote>/<base>`. Run the tests and lint of what
   changed. Push the branch plainly.
5. Never amend, rebase, or force a push.
6. Call `report_pull_request` with `ready: true` and the read `head_sha`
   when every approval and check is green on a new head. Call it with
   `ready: false` on a change back. Report only a change.
7. When the request merges, fetch the base in the repository checkout.
   Run `git -C <repo> fetch <remote> <base>:<base>`, or run
   `merge --ff-only <remote>/<base>` when the checkout is on the base.
   Then call `complete_step` with the merge as the reason.
8. When the request closes without a merge, call `fail_task` with the
   reason.

## Rules

- Never merge the request yourself.
- Leave every thread open.
- Reply only in the thread of the defect.
- Do not reply to a comment that asks for no change.
- Post no thanks, praise, or acknowledgement.
- Open no new thread.
- Post no summary, no top-level comment, and no comment outside a thread.
- Commit only the changes that a comment or a failed check asks for.
- Add no other change.
- Write no trailer in a pull request body, a reply, or a commit: no
  "Generated with", no `Co-Authored-By`, no tool name.
- Use only your session's tools to read or write the request. Report a tool
  failure and do not use another path to post.
- Do not wait on a timer or check the request on your own.
- End your turn when you handle the news.

## Done

Reply only in the thread of the defect. Push each fix after its tests and
lint pass. Report the current approvals and checks. End your turn.

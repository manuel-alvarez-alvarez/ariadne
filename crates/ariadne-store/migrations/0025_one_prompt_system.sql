-- One prompt system: every text an agent receives is a template.
--
-- Until now a profile owned seven briefings and the daemon carried fifteen
-- more texts as string literals: the stall nudges of all four roles, the
-- engineer's revival, the integrator's two wake instructions, and the notice
-- an addressed agent is woken with. They are prompt kinds now, editable per
-- profile like the briefings beside them, so this migration has two halves.
--
-- Half one rewrites the three briefings whose wording moved: `changes_requested`
-- now carries a round written by the people on a published request as readily
-- as one written by the reviewers, `reviewer_resume` reads as the nudge it
-- doubles as, and `integration_resume` covers the published revision the
-- daemon used to compose itself. As in migrations 0009, 0012, 0016, 0017,
-- 0018, 0019, 0020, 0021 and 0022, only where the row still holds the default
-- it was seeded with, so a prompt its user rewrote survives the upgrade. The
-- old texts are the ones migrations 0017 and 0019 last wrote: 0021 rewrote the
-- system prompts and 0022 the integration instructions, and neither touched
-- these three.
--
-- Half two seeds the four new kinds into every profile that owns one. A
-- profile with a row of its own is left alone: reruns are not possible, but a
-- profile created between this release and the upgrade already has them.

-- 1. The engineer's round of requested changes: whoever asked for them.
UPDATE profile_prompts
SET content = 'Changes were requested on your task.

{feedback}

Apply them on the same branch and commit, then `request_review` again, answering every point above — where you disagree with one, say why the code stays as it is instead of changing it.',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'changes_requested'
  AND content = 'Reviewers requested changes on your task.

{feedback}

Apply them on the same branch, commit, and call `request_review` again, saying how each point was addressed.';

-- 2. The reviewer's resume, which is also what an idle reviewer is nudged
--    with: the verdict it owes, and the diff it has to fetch again.
UPDATE profile_prompts
SET content = 'Your verdict is what review round {review_round} of "{task_title}" is waiting on.

Your worktree is on the tip of {branch}, which has moved if the engineer revised the change: fetch the diff again with `get_diff`, review the change as it stands — checking whether your feedback was addressed — and submit exactly one verdict for review round {review_round}: `approve` or `request_changes`.

## The engineer''s summary of what it last did
{summary}',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'reviewer_resume'
  AND content = 'The engineer revised the change: this is review round {review_round} of "{task_title}".

Your worktree has moved to the new tip of {branch}: last round''s diff is stale. Fetch it again with `get_diff`, review the change as it stands — checking whether your feedback was addressed — and submit exactly one verdict for round {review_round}: `approve` or `request_changes`.

## Engineer''s summary of this revision
{summary}';

-- 3. The integrator's resume, which now covers the published revision too:
--    the request Ariadne has recorded, and the engineer's replies to the
--    people reading it.
UPDATE profile_prompts
SET content = 'Pick the integration of "{task_title}" up again: it is approved and yours to land, in {repo_path}. Your worktree is on {branch}, which has moved if the engineer revised the change. Ariadne has recorded {request}.

Check what is open before you touch anything — `gh pr list --head {branch} --state all` on GitHub, `glab mr list --source-branch {branch} --all` on GitLab — then go on from your integration instructions: an open {noun} is the one to update, never a second one, and with none open the task is landed the way they say. Then end your turn.

The summary below is the engineer''s own account of this revision. Where a {noun} is open it is its replies to the people reading it, and the one message you `post_message` to "user" carries it verbatim, so they can answer on the {noun} themselves.

## The engineer''s summary

{summary}',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'integration_resume'
  AND content = 'Pick the integration of "{task_title}" up again: it is approved and yours to land, in {repo_path}. Your worktree is on {branch}, which has moved if the engineer revised the change.

Check first whether it was already published — `gh pr list --head {branch} --state all` on GitHub, `glab mr list --source-branch {branch} --all` on GitLab.

- If a pull or merge request exists, update that one and never open a second, exactly as your integration instructions say a published request is updated: `git fetch <remote> {base_branch} && git merge --no-edit <remote>/{base_branch}`, then a plain `git push <remote> {branch}`, never forced and never rewriting a commit it already shows. Then `post_message` to "user" that it is updated and ready to look at again.
- If none does, land the task as your integration instructions say, from the forge check (`gh auth status` / `glab auth status`) onward: publish it and `record_pull_request` the URL, or, with no forge to publish to, rebase, squash by the repository''s commit conventions, fast-forward the base from the primary checkout and `mark_merged` with the resulting sha.

Whatever you write for the forge or for the commit that lands reads as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer and no mention of Ariadne, agents, models or tooling.

End your turn afterwards: Ariadne watches a published request and wakes you when a human merges it; what they write on it in the meantime goes to the engineer, not to you. If the rebase or the merge conflicts, abort it and `return_to_engineer` with the conflicting files and what to reconcile.';

-- 4. The planner's nudge, for a goal it has stopped planning.
INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
SELECT p.id, 'planner_resume', 'Keep planning "{goal_title}": create the tasks it still needs with `create_task`, or `finalize_plan` once the user agrees the plan is complete. If you are waiting on the user, `post_message` to "user" asks them rather than sitting idle.', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM profiles p
WHERE p.role = 'planner'
  AND NOT EXISTS (
    SELECT 1 FROM profile_prompts pp WHERE pp.profile_id = p.id AND pp.kind = 'planner_resume'
  );

-- 5. The engineer's: the session that ended and the one sitting idle
--    are picked up with the same words.
INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
SELECT p.id, 'engineer_resume', 'Pick "{task_title}" up again: your worktree is on {branch}, and `git status` and `git log` say where the last session left it. Carry on from there until the work is complete and verified, then `request_review`. If something is blocking you, `post_message` says so instead of stalling.', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM profiles p
WHERE p.role = 'engineer'
  AND NOT EXISTS (
    SELECT 1 FROM profile_prompts pp WHERE pp.profile_id = p.id AND pp.kind = 'engineer_resume'
  );

-- 6. The integrator's last wake: a request a human has merged.
INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
SELECT p.id, 'integration_merged', '{request} was merged on {forge}. Finish "{task_title}" off {base_branch} in {repo_path}, the way your integration instructions say a merged request is finished, and `mark_merged` with the sha it landed as. The daemon verifies the merge against {forge}, so report it truthfully.', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM profiles p
WHERE p.role = 'integrator'
  AND NOT EXISTS (
    SELECT 1 FROM profile_prompts pp WHERE pp.profile_id = p.id AND pp.kind = 'integration_merged'
  );

-- 7. And the notice every role is woken with when a message addresses
--    it, which is why this one is seeded into profiles of all four roles.
INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
SELECT p.id, 'message_delivery', 'New message from the {author} in {thread}:

{body}

Read the rest with `list_messages`, answer with `post_message` — both MCP tools.', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM profiles p
WHERE NOT EXISTS (
    SELECT 1 FROM profile_prompts pp WHERE pp.profile_id = p.id AND pp.kind = 'message_delivery'
  );

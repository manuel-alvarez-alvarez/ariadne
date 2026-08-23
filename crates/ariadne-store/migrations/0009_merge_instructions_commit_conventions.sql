-- The merge instructions gained three rules the squash commit must follow:
-- a Conventional Commits message, no authorship or tool trailers, and signing
-- left to the repository's git configuration.
--
-- Defaults are only seeded into an empty database, so an existing install
-- would never see the new text. Rewriting it here reaches those installs
-- without overwriting anyone's work: only rows still holding the old default
-- byte for byte are touched, and an edited prompt stays edited.

UPDATE profile_prompts
SET content = 'Your task has been approved. Merge it now, keeping the base branch''s history linear — one commit per task, no merge commits:

1. In your worktree, rebase onto the latest base: `git fetch . && git rebase {base_branch}` (resolve conflicts if any).
2. Squash the branch into a single commit on top of the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That squash commit is the only one landing on {base_branch}, so its message must:
   - follow Conventional Commits: a `type(scope): summary` subject line derived from the task — the task title, "{task_title}", is not necessarily one already — and a body explaining what changed and why;
   - carry no `Co-Authored-By`, `Generated with` or any other authorship or tool trailer;
   - leave signing to the repository''s git configuration: sign if git is configured to sign, do not pass `--no-gpg-sign` or otherwise disable it, and do not force `-S` either.
3. Fast-forward the base branch from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 1.
4. Call `mark_merged` with the resulting commit sha (`git -C {repo_path} rev-parse {base_branch}`).',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'merge_instructions'
  AND content = 'Your task has been approved. Merge it now, keeping the base branch''s history linear — one commit per task, no merge commits:

1. In your worktree, rebase onto the latest base: `git fetch . && git rebase {base_branch}` (resolve conflicts if any).
2. Squash the branch into a single commit on top of the base: `git reset --soft {base_branch} && git commit -m "{task_title}" -m "<what changed and why>"`.
3. Fast-forward the base branch from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 1.
4. Call `mark_merged` with the resulting commit sha (`git -C {repo_path} rev-parse {base_branch}`).';

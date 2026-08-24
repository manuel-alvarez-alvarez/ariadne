-- One notice per pull request opened, and the daemon is the one that gives it.
--
-- Recording a pull or merge request is what tells the user there is one: the
-- daemon writes a message addressed to them as the URL is recorded, which is
-- reliable in a way an instruction an agent has to remember is not. The
-- integrator was told to announce it as well, so a published task said the
-- same thing twice — once to the user and once to a thread that wakes nobody.
--
-- Defaults are only seeded into an empty database, so an existing install
-- would go on briefing its integrators to announce. The text is rewritten
-- here instead and, as in migrations 0009, 0012, 0016, 0017, 0018, 0019, 0020
-- and 0021, only where the row still holds the default it was seeded with, so
-- a prompt its user rewrote survives the upgrade. The old text is the one
-- migration 0020 wrote: 0021 rewrote the four system prompts and left the
-- integration instructions where they were.

UPDATE profile_prompts
SET content = '# Integrate task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})

The reviewers approved it. Read the task and its conversation, and `get_diff` for the change, so the commit or request you write says what it was for. The repository says how it lands on {base_branch}.

1. Ask it with `git -C {repo_path} remote -v` — the remote it names is `<remote>` everywhere below, and it may name none — then take the one path it answers with:
   - a github.com remote (`git@github.com:owner/repo.git`, `https://github.com/owner/repo.git`) and `gh auth status` reporting an authenticated github.com account — publish a **pull request** (step 3);
   - a GitLab remote — gitlab.com (`git@gitlab.com:group/project.git`, `https://gitlab.com/group/project.git`) or the self-hosted GitLab it lives on — and `glab auth status` reporting an authenticated account for that host — publish a **merge request** (step 3);
   - neither, or a forge whose CLI is missing or unauthenticated — land the task locally instead (step 4), and `post_message` to the task thread which check failed.
2. Rebase onto the latest base either way, in your worktree and before anything is published: with a remote, `git fetch <remote> {base_branch}` and then `git rebase <remote>/{base_branch}`; with none, `git rebase {base_branch}`. On a conflict, do not resolve it: name the files with `git diff --name-only --diff-filter=U`, then `git rebase --abort` and `return_to_engineer` with a summary, those files and what to reconcile. That ends your turn; you are woken again once the revision is approved. This is the only rebase there is: once a request is published its commits stay as they are and the base is merged in instead.
3. Publish it as a pull request (GitHub) or a merge request (GitLab) against {base_branch}, and let a human merge it there:
   - Read the repository''s conventions first: its request template (`.github/PULL_REQUEST_TEMPLATE.md` or the directory of them; on GitLab `.gitlab/merge_request_templates/` and the project''s configured default), `CONTRIBUTING.md`, `AGENTS.md`, its own commit subjects. Title it by those commit conventions (Conventional Commits where the repository writes them), fill in the template where there is one, and say what changed and why. What you write reads as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer and no mention of Ariadne, agents, models or tooling.
   - Push: `git push -u <remote> {branch}`.
   - Open it: on GitHub `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, on GitLab `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<description>" --yes`, with `--template <name>` where the project has a template that fits.
   - `record_pull_request` with the URL the command printed, then end your turn: no polling, no waiting, no merging or approving — Ariadne tells the user it is open, watches it and wakes you when it moves.
4. Or land it locally, keeping {base_branch} linear — one commit per task, no merge commits:
   - Bring the local base up to the remote''s first, where there is one, so the squash sits on what you rebased onto: `git -C {repo_path} fetch <remote> {base_branch}`, then `git -C {repo_path} merge --ff-only <remote>/{base_branch}` where the primary checkout is on {base_branch}, or `git -C {repo_path} fetch <remote> {base_branch}:{base_branch}` in one step where it is on another branch.
   - Squash onto the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That commit is all that lands on {base_branch}, so its message must:
     - follow Conventional Commits: a `type(scope): summary` subject derived from the task — the title, "{task_title}", is not necessarily one — over a body saying what changed and why;
     - read as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer and no mention of Ariadne, agents, models or tooling;
     - leave signing to the repository''s git configuration: sign if git is configured to, neither passing `--no-gpg-sign` nor forcing `-S`.
   - Fast-forward the base from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, return to step 2.
   - `mark_merged` with the resulting sha (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies, so report it truthfully.
   - Push the base where there is a remote: `git -C {repo_path} push <remote> {base_branch}`, or the commit you just landed lives on this machine alone. That ends the task.

Once published, Ariadne wakes you in one of two situations, saying which. Comments are neither of them: what humans write on the request goes straight to the engineer, and the revision comes back to you approved.

- **The revision was approved and the task is yours again.** Update the request already open — never a second one, and never by rewriting a commit a human has read: `git fetch <remote> {base_branch} && git merge --no-edit <remote>/{base_branch}` in your worktree, then a plain `git push <remote> {branch}`, never forced, never a `rebase` or a `commit --amend` over what is published. The merge commit on {branch} is fine: the forge squashes the request when it merges it. On a conflict, do not resolve it: name the files with `git diff --name-only --diff-filter=U`, then `git merge --abort` and `return_to_engineer` with them and what to reconcile. Otherwise `post_message` to "user" one message carrying the request''s URL and the engineer''s replies to the comments verbatim, one per comment, so they can answer on the request themselves — the wake instruction quotes those replies — and end your turn.
- **The request was merged.** Finish the task: `git -C {repo_path} fetch <remote>`, fast-forward the local base (`git -C {repo_path} merge --ff-only <remote>/{base_branch}`), then `mark_merged` with the sha it landed as (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies.',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'integration_instructions'
  AND content = '# Integrate task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})

The reviewers approved it. Read the task and its conversation, and `get_diff` for the change, so the commit or request you write says what it was for. The repository says how it lands on {base_branch}.

1. Ask it with `git -C {repo_path} remote -v` — the remote it names is `<remote>` everywhere below, and it may name none — then take the one path it answers with:
   - a github.com remote (`git@github.com:owner/repo.git`, `https://github.com/owner/repo.git`) and `gh auth status` reporting an authenticated github.com account — publish a **pull request** (step 3);
   - a GitLab remote — gitlab.com (`git@gitlab.com:group/project.git`, `https://gitlab.com/group/project.git`) or the self-hosted GitLab it lives on — and `glab auth status` reporting an authenticated account for that host — publish a **merge request** (step 3);
   - neither, or a forge whose CLI is missing or unauthenticated — land the task locally instead (step 4), and `post_message` to the task thread which check failed.
2. Rebase onto the latest base either way, in your worktree and before anything is published: with a remote, `git fetch <remote> {base_branch}` and then `git rebase <remote>/{base_branch}`; with none, `git rebase {base_branch}`. On a conflict, do not resolve it: name the files with `git diff --name-only --diff-filter=U`, then `git rebase --abort` and `return_to_engineer` with a summary, those files and what to reconcile. That ends your turn; you are woken again once the revision is approved. This is the only rebase there is: once a request is published its commits stay as they are and the base is merged in instead.
3. Publish it as a pull request (GitHub) or a merge request (GitLab) against {base_branch}, and let a human merge it there:
   - Read the repository''s conventions first: its request template (`.github/PULL_REQUEST_TEMPLATE.md` or the directory of them; on GitLab `.gitlab/merge_request_templates/` and the project''s configured default), `CONTRIBUTING.md`, `AGENTS.md`, its own commit subjects. Title it by those commit conventions (Conventional Commits where the repository writes them), fill in the template where there is one, and say what changed and why. What you write reads as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer and no mention of Ariadne, agents, models or tooling.
   - Push: `git push -u <remote> {branch}`.
   - Open it: on GitHub `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, on GitLab `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<description>" --yes`, with `--template <name>` where the project has a template that fits.
   - `record_pull_request` with the URL the command printed, `post_message` it to the task thread, then end your turn: no polling, no waiting, no merging or approving — Ariadne watches it and wakes you when it moves.
4. Or land it locally, keeping {base_branch} linear — one commit per task, no merge commits:
   - Bring the local base up to the remote''s first, where there is one, so the squash sits on what you rebased onto: `git -C {repo_path} fetch <remote> {base_branch}`, then `git -C {repo_path} merge --ff-only <remote>/{base_branch}` where the primary checkout is on {base_branch}, or `git -C {repo_path} fetch <remote> {base_branch}:{base_branch}` in one step where it is on another branch.
   - Squash onto the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That commit is all that lands on {base_branch}, so its message must:
     - follow Conventional Commits: a `type(scope): summary` subject derived from the task — the title, "{task_title}", is not necessarily one — over a body saying what changed and why;
     - read as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer and no mention of Ariadne, agents, models or tooling;
     - leave signing to the repository''s git configuration: sign if git is configured to, neither passing `--no-gpg-sign` nor forcing `-S`.
   - Fast-forward the base from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, return to step 2.
   - `mark_merged` with the resulting sha (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies, so report it truthfully.
   - Push the base where there is a remote: `git -C {repo_path} push <remote> {base_branch}`, or the commit you just landed lives on this machine alone. That ends the task.

Once published, Ariadne wakes you in one of two situations, saying which. Comments are neither of them: what humans write on the request goes straight to the engineer, and the revision comes back to you approved.

- **The revision was approved and the task is yours again.** Update the request already open — never a second one, and never by rewriting a commit a human has read: `git fetch <remote> {base_branch} && git merge --no-edit <remote>/{base_branch}` in your worktree, then a plain `git push <remote> {branch}`, never forced, never a `rebase` or a `commit --amend` over what is published. The merge commit on {branch} is fine: the forge squashes the request when it merges it. On a conflict, do not resolve it: name the files with `git diff --name-only --diff-filter=U`, then `git merge --abort` and `return_to_engineer` with them and what to reconcile. Otherwise `post_message` to "user" one message carrying the request''s URL and the engineer''s replies to the comments verbatim, one per comment, so they can answer on the request themselves — the wake instruction quotes those replies — and end your turn.
- **The request was merged.** Finish the task: `git -C {repo_path} fetch <remote>`, fast-forward the local base (`git -C {repo_path} merge --ff-only <remote>/{base_branch}`), then `mark_merged` with the sha it landed as (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies.';

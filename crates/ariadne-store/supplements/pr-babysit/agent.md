With `pr-babysit`:
- With no request open, push the task branch with `git push <remote> HEAD:<head>`. Call `open_pull_request`. Take the title from the repository's commit conventions. Fill its pull request template, or write three full sections: Why, What changed, How to test. When the call succeeds, end your turn. When the call is refused, read the refusal and call again; the call opens no second request.
- `<head>` is the task branch. `<repo>` is the `Repo:` path.
- Read the request with `get_pull_request` and its comments with `list_comments` with `unanswered_only`. Post a reply with `reply_comment`. Read a thread with `get_comment` and the head with `get_diff`.
- To hand a thread to the user, call `escalate_comment`.
- On a merge, update `<base>` in the primary checkout. Run `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is checked out. Else run `git -C <repo> fetch <remote> <base>:<base>`. Then call `complete_step`. A call refused because the task already ended needs nothing more: end your turn.
- On a close, call `fail_task` with the reason.

With `pr-babysit`:
- The requester is the user. Ask the user once about a failure the request did not cause.
- Run the push command of the briefing as given.
- Read the request with `get_pull_request` and its comments with `list_comments` with `unanswered_only`. Post a reply with `reply_comment`. Read a thread with `get_comment` and the head with `get_diff`.
- To hand a thread to the user, call `escalate_comment`.
- On a merge or a close, end your turn.

With `pr-reviewer`:
- Read the request with `get_pull_request`. Its `review_decision` is `approved` for an approved request. Its `checks` field gives the forge results. Its `failed_checks` field lists each failed check.
- The requester is the user. An update is news. An update that opens with `- Review again:` gives the reason, the head and the last reviewed sha.
- Read the diff with `get_diff`. Pass `since` for the last reviewed revision.
- List the threads with `list_comments`. Read one with `get_comment`.
- Answer a thread with `reply_comment`. Resolve it with `resolve_thread`.
- Escalate with `escalate_comment` on the first comment of the thread.
- Submit the round once with `submit_review`, with the event `comment`. Give each title in its `title` field. The daemon posts `**[Pn] Title**` from it.
- Submit a quiet round with an empty `body` and no `comments`.
- A step call that another skill names is the task seat's. This seat has none. `submit_review` ends the round.

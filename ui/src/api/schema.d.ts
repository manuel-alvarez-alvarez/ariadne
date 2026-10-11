/**
 * Types generated from the ariadned OpenAPI document — DO NOT EDIT BY HAND.
 * Regenerate with `npm run gen:api` (see ui/README.md).
 */

export interface paths {
    "/v1/acp-agents": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Every discovered and configured ACP agent with its cached probe result. */
        get: operations["acp-agents_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/acp-agents/refresh": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Probe every registry entry and replace the cached discovery snapshot. */
        post: operations["acp-agents_refresh"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/agents": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Every registry agent's flags, in registry order. An agent nobody set
         *     flags for is listed with none.
         */
        get: operations["agents_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/agents/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Replace a registry agent's flags.
         * @description The list is replaced whole, and an empty one is a legitimate answer.
         *     Restoring the defaults is this same call with the `default_flags` the
         *     GET hands out — nothing else to learn, and nothing that can drift from
         *     them.
         */
        put: operations["agents_update"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/attention": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Every item a human has something to do about right now: read off every
         *     registered producer (`crate::attention`), not inferred here from a task
         *     or a session's bare status.
         */
        get: operations["attention_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/doctor": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * What the daemon sees: its PATH, the binaries on it, and the state of the
         *     directories it works in.
         */
        get: operations["system_report"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/events": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List agent events (poll with `after` for tailing, or read the newest with
         *     `order=desc` and walk back with `before`).
         */
        get: operations["events_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/events/stream": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Subscribe to the live domain-event stream.
         * @description Every state change in the daemon — from HTTP calls, from the scheduler and
         *     from agent activity alike — is published here. Each message carries a fresh
         *     ULID `id`, the event kind as its `event` name, and the full updated DTO as
         *     `data`, so clients patch their state without refetching.
         *
         *     Not every event is a database write. `task_branch_updated` is published by
         *     the daemon's watch on each live task's branch ref, so a commit an agent
         *     makes in its worktree — which changes nothing in the store — still says
         *     that the task's diff (`GET /v1/tasks/{id}/diff`) is no longer the one you
         *     hold. It carries the branch and the full sha of its new head.
         *
         *     There is **no replay or backfill**: the `id` is informational and
         *     `Last-Event-ID` is ignored. On (re)connect, refetch the REST state you care
         *     about and then follow the stream.
         *
         *     Besides the domain events there is a `heartbeat` event (a `HeartbeatDto`:
         *     the daemon's `version` and its `started_at`), sent as the connection opens
         *     and every 15 s an idle connection goes without one. It is a named event
         *     rather than the SSE comment other streams keep alive with, because a
         *     browser's `EventSource` never surfaces a comment: a client watches it to
         *     tell a live daemon from a dead one, and a changed `started_at` to tell a
         *     restarted daemon from the one it was talking to.
         *
         *     A client that falls too far behind loses events. It is never left silently
         *     stale: the daemon sends a final `resync` event (`{"missed": n}`) and closes
         *     the connection, so an `EventSource` reconnects and takes the refetch path
         *     above.
         */
        get: operations["events_stream"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/forge/tunnel": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The tunnel switch, the tunnel state and the bound listener address. */
        get: operations["repositories_get_tunnel"];
        /**
         * Turn the tunnel on or off. Off closes it, and every integration fetches on
         *     its timer; the hooks stay registered.
         */
        put: operations["repositories_set_tunnel"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List goals. */
        get: operations["goals_list"];
        put?: never;
        /**
         * Create a goal on registered repositories; the orchestrator session is
         *     spawned by the scheduler once agent execution lands.
         * @description The repos are referenced, not copied: whatever `POST /v1/repositories`
         *     validated about a checkout holds for every goal that names it, and an edit
         *     there moves this goal too. The workflow is the one the request names, or
         *     the first repository's default, and its columns are copied onto the goal.
         */
        post: operations["goals_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{goal_id}/tasks": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create a task in a goal (orchestrator via MCP, or the user). */
        post: operations["tasks_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Inspect a goal. */
        get: operations["goals_get"];
        put?: never;
        post?: never;
        /** Delete a finished goal and everything under it. */
        delete: operations["goals_delete"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{id}/cancel": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Cancel a goal. */
        post: operations["goals_cancel"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{id}/complete": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Complete the goal: it moves active -> completed and every session of it
         *     is torn down.
         * @description The orchestrator's call, because it is the only agent that knows whether
         *     the plan did what the goal asked for — the daemon can see that every task
         *     ended, and not whether the goal is met. The user's too: it is their goal,
         *     and a goal whose orchestrator will not start is otherwise one nothing can
         *     close.
         *
         *     What the daemon checks is the part it can see. A goal with a task still
         *     going is not one anybody may declare finished, however sure they are.
         */
        post: operations["goals_complete"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{id}/finalize": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Finalize the plan: goal moves planning -> active and its tasks start. The
         *     orchestrator's call alone, and there is nothing left for the user to
         *     approve. A task with a column nobody staffs is refused by the column's
         *     name: every task runs through every column, and one with nobody on it
         *     would stop there.
         */
        post: operations["goals_finalize"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/goals/{id}/messages": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The messages of a goal: what its agents said that was not about one task.
         * @description A goal's channel is the orchestrator's inbox. Everything an agent says to
         *     it about a task is on that task instead, and this is where the rest goes.
         */
        get: operations["goals_list_goal_messages"];
        put?: never;
        /** Send a message about the goal itself. */
        post: operations["goals_post_goal_message"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/health": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Daemon liveness probe. */
        get: operations["system_health"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/logs": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Recent daemon log lines from the in-memory ring buffer, oldest first. */
        get: operations["logs_snapshot"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/logs/stream": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Follow the daemon log.
         * @description The stream opens with a `snapshot` event carrying the current ring buffer
         *     (a `LogSnapshotResponse`, what `GET /v1/logs` would have returned), then
         *     sends a `delta` event per new line (a `LogLineDto`). Payloads are compact
         *     JSON, so log content cannot break SSE framing. There is no replay on
         *     reconnect: every connection starts over from a fresh snapshot, which is
         *     also the resync path for a follower that fell behind and was dropped.
         */
        get: operations["logs_stream"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/models": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Everything an agent can be pinned to, `<agent>:<model>` apiece: the
         *     models discovery found each registry agent offering, grouped by agent.
         *     No bare-agent entry: a model is required wherever an agent is pinned,
         *     so there is nothing an agent on its own could be staffed as.
         * @description Each entry carries the efforts it can be run at, as the agent offered
         *     them, and which of them it runs by default. Every entry says whether
         *     an agent can be staffed on it. A model the user turned off stays in
         *     the list, off: a catalog that hid it would leave nothing to turn back
         *     on, and nothing to say why a pin naming it is refused.
         */
        get: operations["models_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/models/enabled": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Turn one entry of the catalog on or off.
         * @description The catalog is discovery, so this writes only the exception:
         *     an id nothing in the catalog carries is a 404, and the last entry left
         *     on cannot be turned off — a plan needs something to be staffed on, and
         *     a daemon that can staff nothing is not a state to leave a user in.
         */
        put: operations["models_set_enabled"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/models/rank": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /** Set or clear a catalog entry's rank without changing its enabled flag. */
        put: operations["models_set_rank"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/outside-sessions/resume": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Resume an outside conversation without creating a goal or task. */
        post: operations["sessions_resume_outside"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/permissions/ai": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The AI permission settings, the interpreter probed afresh, and where the install
         *     has got to.
         */
        get: operations["permissions_get"];
        /**
         * Change the AI permission settings. An absent field stays as it was.
         * @description Turning the model on is refused while the daemon has no Python 3.12 or 3.13 to
         *     install into: the download is minutes and gigabytes, and it would fail at
         *     the end of them. Turning it off keeps every file on disk, so turning it
         *     back on repairs its pinned package and weights.
         */
        put: operations["permissions_update"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/permissions/ai/refresh": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Run the install again: the pinned package, adapter and base. */
        post: operations["permissions_refresh"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/permissions/ai/test": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Score one request with the AI permission model without selecting an option
         *     or changing any permission state.
         */
        post: operations["permissions_test"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/permissions/learned": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["permissions_list_learned"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/permissions/learned/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["permissions_get_learned"];
        /** Widen or narrow a learned row: the only write a person makes on one. */
        put: operations["permissions_update_learned"];
        post?: never;
        delete: operations["permissions_delete_learned"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The open requests of the enabled repositories, read live off the forge
         *     (026): every one, or with `role` and `requested` the user's own or the
         *     ones that ask for their review, each with what Ariadne keeps of it where
         *     it works on it. With `task`, the request a task opened, which its author
         *     keeps (005).
         */
        get: operations["pull-requests_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/refresh": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["pull-requests_refresh"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** A request Ariadne works on, read off the forge now. */
        get: operations["pull-requests_get"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/comments": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The comments of a request Ariadne works on, read off the forge now, with
         *     the marks beside them; with `unanswered_only`, the threads that wait on
         *     the integration login.
         */
        get: operations["pull-requests_comments"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/comments/{comment_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** One comment of a request Ariadne works on, read off the forge now. */
        get: operations["pull-requests_comment"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/comments/{comment_id}/reply": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Reply to one comment. The daemon posts the reply through the forge CLI,
         *     marks it the review's where a review session posts it (029), and reads
         *     the request again.
         */
        post: operations["pull-requests_reply"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/comments/{comment_id}/resolve": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Resolve the thread of one comment, once a push fixed what it found
         *     (029). Only a review session resolves, and only a thread it opened under
         *     the integration login: the threads of anyone else stay open for the
         *     person who wrote them.
         */
        post: operations["pull-requests_resolve"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/diff": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The change under review, read in the reviewer session's worktree (029):
         *     `git diff <base>...HEAD`, or `git diff <since>..HEAD` where `since` is
         *     given. The base is the remote's copy of the base branch where the
         *     checkout holds one, else the local branch.
         */
        get: operations["pull-requests_diff"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/report": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * What the request's session says of it: `ready` once the babysitting task
         *     believes every required approval and check reads green, and the head a
         *     review session posted its review on. Neither raises `waiting_user` on
         *     the session by itself (029): the claim alone is not confirmed evidence,
         *     and a pr-reviewer session finishing its own review must notify nobody —
         *     the `pull_request` attention producer reads the forge's own evidence
         *     against this claim and raises its own item once it actually backs the
         *     claim up.
         */
        post: operations["pull-requests_report"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/pull-requests/{id}/reviews": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Post one round of a review of a request, in the user's name (029): its
         *     new findings as one review of inline comments, each led by its priority,
         *     and its summary, written into the one summary comment the review keeps
         *     on the request — posted on the first round, edited on every later one. A
         *     round with no new finding posts no review. `request_changes` stands only
         *     on a P0 finding, new or still open. Any other event is refused, an
         *     approval above all: the user gives every approval. What it posted is
         *     marked as the review's.
         */
        post: operations["pull-requests_submit_review"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List repositories. */
        get: operations["repositories_list"];
        put?: never;
        /** Create a repository. */
        post: operations["repositories_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a repository. */
        get: operations["repositories_get"];
        /** Update a repository. */
        put: operations["repositories_update"];
        post?: never;
        /** Delete a repository. */
        delete: operations["repositories_delete"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}/issues": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["issues_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}/issues/{number}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["issues_get"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}/pull-requests/search": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["pull-requests_search"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}/pull-requests/{number}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * One open request of a repository, read off the forge now (026): its
         *     own read, its checks and its comments, with what Ariadne keeps of it
         *     where it works on it. What the desktop's panel shows.
         */
        get: operations["pull-requests_get_by_number"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{id}/pull-requests/{number}/ariadne-review": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Ask Ariadne to review a request on the model the user picks, or stop
         *     asking (029): a review session runs on that pin while the request is
         *     open and out of draft. A request of the user's own posts each round as
         *     a comment in the user's name; one that asks for the user's review on a
         *     repository with no review pin of its own gets the manual start the
         *     `pull_request` attention producer offers when nobody is assigned to it,
         *     and posts as any other review Ariadne runs on it would (029). Asking
         *     starts Ariadne's work on the request; stopping ends it, where no task
         *     keeps the request. A request that asks for the user's review on a
         *     repository that already pins one has a review already, and takes no
         *     asking.
         */
        put: operations["pull-requests_ask_review"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List sessions: one page of the sessions Ariadne runs and the
         *     conversations the ACP agents stored themselves, newest activity first.
         */
        get: operations["sessions_list"];
        put?: never;
        /**
         * Start a loose session: a new conversation with an agent, in a directory,
         *     with no goal, task or seat behind it.
         * @description The model is checked as a goal's pin is — a registry agent and a model of
         *     it, not one the user turned off — and the effort against that model. The
         *     directory must be an absolute path to one that exists. The session comes
         *     back live, waiting for the first prompt typed into its console, which
         *     becomes its title.
         */
        post: operations["sessions_create_session"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Inspect a session. */
        get: operations["sessions_get"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/console": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The session's newest events, in order: the transcript a console opens on.
         *     It is a page of at most two hundred, the recent past rather than every
         *     turn a session ever ran; `GET /v1/events` walks back from it. While a
         *     turn runs, one `agent_thought_chunk` or `agent_message_chunk` holding the
         *     text so far follows the stored events.
         */
        get: operations["sessions_snapshot"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/console/cancel": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Cancel the turn a session is running.
         * @description Sends ACP `session/cancel` to the agent while its `session/prompt` is
         *     still in flight. The turn then ends as any other does — the text so far
         *     stored, then a `stop` whose `stop_reason` is `cancelled`. A session that
         *     is not live, one whose agent process is gone, and one between turns all
         *     have nothing to cancel, and say so with `409`.
         */
        post: operations["sessions_console_cancel"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/console/input": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Type into a session.
         * @description While a permission request is pending, the text selects that request's
         *     option; otherwise it becomes a fresh `session/prompt` — sent at once if
         *     the agent is between turns, or queued, in order, behind whichever one is
         *     running and sent the moment it ends.
         *
         *     Both halves of "live" matter: the row's status, because a finished
         *     session takes no more input, and the runtime itself, which has no agent to
         *     hand a prompt to for a session whose process is gone.
         *
         *     And it is the user acting on the session, so whatever it was flagged for
         *     comes down with the input: a permission answered, a question typed back,
         *     a message read. An agent still blocked raises its own again with its next
         *     event. The scheduler hears about it as it does about an ingested event, so
         *     the quiet clock and the stream follow.
         */
        post: operations["sessions_console_input"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/console/stream": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Follow a session's console.
         * @description Opens with a `snapshot` event carrying what `GET /console` would return —
         *     the newest page of stored events, oldest first, then the running turn's
         *     text so far — then an `event` per later one, each an `AgentEventDto`.
         *     Subscribing happens before the snapshot is read and every later stored
         *     event is compared against the snapshot's last id, so nothing committed in
         *     between is ever missed or delivered twice; the live events are read under
         *     the runtime's own turn lock for the same guarantee.
         *
         *     There is no replay and no `Last-Event-ID`: reconnecting starts again from a
         *     fresh snapshot. A client that falls too far behind gets a final `resync`
         *     event and the connection closes, exactly as `/v1/events/stream` does.
         */
        get: operations["sessions_stream"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/console/terminal": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * A session's console as a terminal.
         * @description Upgrades to a WebSocket. The client sends JSON text frames — a `resize`
         *     with the terminal's columns and rows first and on every change, a `key`
         *     per key press and a `paste` per pasted text (`TerminalClientMessage`) —
         *     and reads binary frames of terminal bytes that draw the console, plus a
         *     text frame carrying the session's status as the socket opens and as it
         *     ends (`TerminalServerMessage`). Closing the socket ends the console and
         *     leaves the session running; the session ending, Ctrl-C twice or Ctrl-D
         *     close the socket.
         */
        get: operations["sessions_console_terminal"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/kill": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Kill a session's agent process. */
        post: operations["sessions_kill"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/resume": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Revive an ended session: a new agent process, same agent conversation
         *     (resumed via the stored internal session id). Returns the session to
         *     attach to, which is this one either way — relaunched under its own id, or
         *     untouched when its agent turned out to be alive already.
         * @description A missing worktree falls back to the repository checkout. A completed
         *     goal stays completed while its session runs again.
         */
        post: operations["sessions_resume"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/switch": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Switch a session to another model or agent. A same-agent switch keeps the
         *     session and conversation; another agent starts a new session on the seat.
         * @description The pin is checked as `POST /v1/sessions` checks it, a model turned off
         *     included. A session of a cancelled goal is refused.
         */
        post: operations["sessions_switch_session"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/skills": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List every skill, shipped and written, by name. */
        get: operations["skills_list"];
        put?: never;
        /**
         * Create a skill of the user's own.
         * @description It carries its own document: nothing Ariadne ships answers to its name, so
         *     there is nothing behind it to fall back to.
         */
        post: operations["skills_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/skills/{name}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get one skill by name. */
        get: operations["skills_get"];
        /** Write a new document over a skill's. */
        put: operations["skills_update"];
        post?: never;
        /** Delete a skill of the user's own (409 for a built-in, or while loaded). */
        delete: operations["skills_delete"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/skills/{name}/document/reset": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Put a built-in skill back on the document Ariadne ships. */
        post: operations["skills_reset_document"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/stats/attention": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["stats_attention"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/stats/models": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["stats_models"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/stats/spend": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["stats_spend"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/stats/time": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["stats_time"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/stats/work": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["stats_work"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List tasks. */
        get: operations["tasks_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Inspect a task. */
        get: operations["tasks_get"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Edit a task that is not running (orchestrator or user): pending, ready,
         *     or failed and waiting for a retry.
         */
        patch: operations["tasks_update"];
        trace?: never;
    };
    "/v1/tasks/{id}/cancel": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Cancel a task: the user's, or the orchestrator's, which holds the plan
         *     the task belongs to.
         * @description Who called it is read from the session header rather than assumed, so the
         *     transition log says which of the two it was — and so the state machine
         *     refuses a column's agent reaching for it.
         */
        post: operations["tasks_cancel"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/diff": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Diff of the task branch against its base (`git diff base...branch`), or,
         *     once the task is merged, the diff its merge commit brought into the base —
         *     after the merge the branch is contained in the base, so the three-dot diff
         *     would be forever empty.
         */
        get: operations["tasks_diff"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/messages": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The messages of a task: what its agents have said to each other. */
        get: operations["tasks_list_task_messages"];
        put?: never;
        /**
         * Send a message about a task.
         * @description Who it is from is the session header's, never the body's: an agent cannot
         *     write as somebody else, and a call with no session behind it is the user
         *     speaking.
         */
        post: operations["tasks_post_task_message"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/pull-request": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Open the pull or merge request a task lands by, through the repository's
         *     own forge CLI, and answer its URL.
         * @description The daemon runs the forge call, never the agent: `gh` or `glab` only ever
         *     run here, with the authentication the user already set up for them
         *     ([`crate::forge`]). The current column's agent supplies only what the
         *     daemon cannot read off the task or the repository — the title and the
         *     body — and gets the URL back to show the user.
         *
         *     One request per task: a task that already has a `pr_url` answers it again
         *     and opens nothing, so a retried call never opens a second request.
         */
        post: operations["tasks_open_pull_request"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/retry": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Retry a failed task: failed -> ready, which starts it on its first column
         *     again. The user's call, and the orchestrator's — the daemon wakes it when
         *     a task fails, and retrying is one of the three answers it has. A task
         *     with a column nobody staffs is refused by that column's name.
         */
        post: operations["tasks_retry"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/step/complete": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["tasks_complete"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/step/fail": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["tasks_fail"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/transitions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Transition audit log of a task. */
        get: operations["tasks_list_transitions"];
        put?: never;
        /** Request a status transition. The actor is derived from the call context. */
        post: operations["tasks_transition"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/version": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Daemon name and version. */
        get: operations["system_version"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workflows": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List every workflow, shipped and written, by name. */
        get: operations["workflows_list"];
        put?: never;
        /**
         * Create a workflow of the user's own. It carries its own document: nothing
         *     Ariadne ships answers to its name, so there is nothing behind it to fall
         *     back to.
         */
        post: operations["workflows_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workflows/parse": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Parse a document without saving it anywhere: the picker a workflow editor
         *     checks a draft against before it writes one.
         */
        post: operations["workflows_parse"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workflows/{name}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get one workflow by name. */
        get: operations["workflows_get"];
        /** Write a new document over a workflow's. */
        put: operations["workflows_update"];
        post?: never;
        /** Delete a workflow of the user's own (409 for a built-in). */
        delete: operations["workflows_delete"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workflows/{name}/reset": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Put a built-in workflow back on the document Ariadne ships. */
        post: operations["workflows_reset"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** @description One ACP agent known to the daemon and its latest discovery result. */
        AcpAgentDto: {
            capabilities: components["schemas"]["AcpCapabilitiesDto"];
            /** @description Program followed by its arguments. */
            command: string[];
            /** @description One flag for every optional capability that is absent. */
            degraded: components["schemas"]["AcpDegradation"][];
            /** @description Stable id used as the model-id prefix. */
            id: string;
            /** @description Why discovery rejected this agent. */
            rejection_reason?: string | null;
            /** @description Where the entry came from. */
            source: components["schemas"]["AcpAgentSource"];
            status: components["schemas"]["AcpAgentStatus"];
        };
        /**
         * @description Where one registry entry came from: an agent of the ACP registry index
         *     found on the daemon's `PATH`, or an `[[acp_agents]]` entry of its config.
         * @enum {string}
         */
        AcpAgentSource: "registry" | "config";
        /** @enum {string} */
        AcpAgentStatus: "ready" | "rejected";
        /** @description Required and optional ACP capabilities measured by discovery. */
        AcpCapabilitiesDto: {
            model: boolean;
            protocol_v1: boolean;
            session_list: boolean;
            session_load: boolean;
            session_new: boolean;
            stdio: boolean;
            thought_level: boolean;
        };
        /**
         * @description An optional ACP capability missing from an otherwise usable agent.
         * @enum {string}
         */
        AcpDegradation: "no_efforts" | "no_adoption" | "no_restart_resume";
        /**
         * @description Who is attempting a transition: a seat, the daemon, or the user.
         * @enum {string}
         */
        Actor: "orchestrator" | "agent" | "daemon" | "user";
        /**
         * @description One agent to staff on a task: the column it works, the skills it loads,
         *     and what it is to run on.
         *
         *     The model is written `<agent>:<model>`: the id of an agent in the ACP
         *     registry, and after the `:` one model of it. Both halves are required — a
         *     model is required, and no agent default stands in for one — and a string
         *     naming no registry agent is refused: nothing here derives one from the
         *     other.
         */
        AgentAssignment: {
            /**
             * @description What to tell this agent beyond the task itself. Omitted = the task is
             *     the whole of it.
             */
            brief?: string | null;
            /**
             * @description The reasoning effort to run that model at, one of the efforts
             *     `GET /v1/models` lists for it; anything else is refused. Omitted (or
             *     "default") = whatever the agent runs the model at.
             * @example high
             */
            effort?: string | null;
            /**
             * @description What this agent runs on, `<agent>:<model>`. Required; the empty
             *     string and the word "default" are refused.
             * @example codex-acp:o3
             */
            model: string;
            /**
             * @description The names of the skills this agent loads, in the order they reach it.
             *     Omitted or empty = the column's own skills. A name no skill answers
             *     to is refused.
             * @example [
             *       "coding",
             *       "documentation"
             *     ]
             */
            skills?: string[];
            /**
             * @description The id of the workflow column this agent works, one of the goal's
             *     columns. A task takes one agent per column, and a column named twice
             *     is refused.
             */
            step: string;
        };
        /**
         * @description How one registry agent is launched, shared by every session that runs on
         *     it.
         */
        AgentConfigDto: {
            /** @description The id of the agent in the ACP registry (`GET /v1/acp-agents`). */
            agent_id: string;
            /**
             * @description What Ariadne ships for this agent: what restoring the defaults writes
             *     back — a client resets by sending these back as `extra_flags`. An ACP
             *     agent ships with no flags, so this is empty.
             */
            default_flags: string[];
            /**
             * @description Argv flags appended to the agent's registry command on every spawn and
             *     resume.
             */
            extra_flags: string[];
        };
        AgentEventDto: {
            created_at: string;
            id: string;
            /** @description e.g. session_start, post_tool_use, stop */
            kind: string;
            payload: unknown;
            session_id?: string | null;
            /**
             * @description The one-line gist of `payload`, built by the daemon from the event's
             *     own vocabulary rather than stored: an action and its subject for a tool
             *     call, the agent's own words where it left any, and `…` where nothing
             *     of it can be read.
             */
            summary: string;
            task_id?: string | null;
        };
        /**
         * @description An agent event as the domain stream carries it: everything of
         *     [`AgentEventDto`] but the payload, which reaches 1 MB. A client that wants
         *     the payload reads `GET /v1/events`, or the console stream.
         */
        AgentEventSummaryDto: {
            created_at: string;
            id: string;
            /** @description e.g. session_start, post_tool_use, stop */
            kind: string;
            session_id?: string | null;
            /** @description The one-line gist of the payload, as [`AgentEventDto::summary`]. */
            summary: string;
            task_id?: string | null;
        };
        /**
         * @description What one staffed agent spent on a task, named the way a reader addresses
         *     it: an agent has no name of its own, so its column and its skills are what
         *     identify it.
         */
        AgentUsageDto: {
            agent_id: string;
            /** @description The skills the agent loads; empty only if the agent is gone. */
            skills: string[];
            /**
             * @description The column the agent works; null for an agent the task no longer
             *     staffs.
             */
            step?: string | null;
            usage: components["schemas"]["TokenUsageDto"];
        };
        /**
         * @description Where the install has got to.
         * @enum {string}
         */
        AiPermissionsState: "disabled" | "installing" | "ready" | "failed";
        /** @description The AI permission settings and the state of the install behind them. */
        AiPermissionsStatusDto: {
            /**
             * Format: double
             * @description Danger at or below this value is allowed, 0 to 1.
             * @example 0.0201
             */
            allow_threshold: number;
            /**
             * Format: double
             * @description Danger at or above this value is denied, 0 to 1.
             * @example 0.6321
             */
            deny_threshold: number;
            /**
             * @description The device the flavour runs on: the best one the machine could run it
             *     on, unless another was chosen.
             */
            device: components["schemas"]["Device"];
            /** @description Whether the model answers permission requests at all. */
            enabled: boolean;
            /** @description Where the model server answers, once one is running (022, Server). */
            endpoint?: string | null;
            /** @description The Kev flavour chosen, `4b` by default where the machine can run it. */
            flavour: components["schemas"]["Flavour"];
            /** @description Every flavour with every device it might run on, in wire order. */
            flavours: components["schemas"]["FlavourOptionsDto"][];
            /** @description The machine the daemon runs on, probed afresh. */
            hardware: components["schemas"]["HardwareDto"];
            /**
             * @description The pinned model package and run on disk.
             * @example kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101
             */
            installed_release?: string | null;
            /** @description Why the last install failed. */
            last_error?: string | null;
            /** @description When the last install ended well, RFC 3339 in UTC. */
            last_refresh_at?: string | null;
            /** @description The pinned model package and run the last install used. */
            latest_release?: string | null;
            python: components["schemas"]["PythonDto"];
            state: components["schemas"]["AiPermissionsState"];
            /**
             * @description Whether the pair above is the default of the chosen flavour. `false`
             *     where the user set it by hand; a flavour change then keeps it.
             */
            thresholds_default: boolean;
            /** @description Whether the checkpoints of the last good install are on disk. */
            weights_present: boolean;
        };
        /**
         * @description Body of `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review`:
         *     whether Ariadne
         *     reviews a request of the user's own (029).
         */
        AskReviewRequest: {
            asked: boolean;
            /** @description The effort that model runs at; none for the agent's own. */
            effort?: string | null;
            /**
             * @description The model the review runs on, `agent:model`: required to ask, and
             *     checked against the catalog as any pin is.
             */
            model?: string | null;
            /**
             * @description The skills the review loads beside `pr-reviewer`, which every review
             *     session loads; skills a task agent is staffed on.
             */
            skills?: string[];
        };
        /**
         * @description What kind of blocker an item is, in the one vocabulary every producer
         *     shares. `Unknown` is for a blocker a producer raises without reliable
         *     evidence of which of the other four it is — it is never grouped with
         *     another item, unknown or not (009, "Unknown causes remain separate").
         * @enum {string}
         */
        AttentionCause: "access" | "quota" | "configuration" | "resource" | "unknown";
        AttentionFlagDto: {
            /** Format: double */
            mean_wait_secs: number;
            /** Format: int64 */
            raised: number;
            reason: string;
        };
        /**
         * @description The times a person stepped in: permissions decided at the console,
         *     questions and stalls. `person_secs` is how long those waited on the
         *     person.
         */
        AttentionInterventionsDto: {
            /** Format: int64 */
            permissions: number;
            /** Format: double */
            person_secs: number;
            /** Format: int64 */
            questions: number;
            /** Format: int64 */
            stalls: number;
            /** Format: int64 */
            total: number;
        };
        /** @description One item on the Needs attention list. */
        AttentionItemDto: {
            /**
             * @description Every entity this blocker's affected work names. A grouped item
             *     (009, "A shared access or resource failure produces one grouped
             *     item") lists every one of them here rather than splitting into an
             *     item per entity.
             */
            affected: components["schemas"]["AttentionSubjectDto"][];
            /**
             * @description Stable across a restart and across the same cause recurring: derived
             *     from the cause and what it shares rather than issued fresh, so the
             *     same blocker is the same row for as long as it stands.
             */
            id: string;
            producer: components["schemas"]["AttentionProducer"];
            /**
             * @description The shared contract's `reason`: why this blocker exists, in the one
             *     vocabulary every producer draws from (`AttentionCause`).
             */
            reason: components["schemas"]["AttentionCause"];
            /** @description The one action that clears this item. */
            required_action: string;
            /** @description When this blocker was first observed, RFC 3339. */
            since: string;
            /** @description What is blocked and why, in one line. */
            summary: string;
            target: components["schemas"]["AttentionTarget"];
        };
        /** @description The whole list, as `GET /v1/attention` answers it. */
        AttentionListDto: {
            /**
             * @description False where a producer's read failed, so a partial list is never
             *     read as an all-clear: it still answers with whatever the other
             *     producers found.
             */
            complete: boolean;
            items: components["schemas"]["AttentionItemDto"][];
        };
        /**
         * @description Which producer raised an item — distinct from [`AttentionCause`], since
         *     a cause describes *why* a blocker exists and a later producer (an
         *     agent's own request, a pull request's next step) may share none of
         *     recovery's causes, or raise `unknown` for a reason a client still needs
         *     to tell apart from recovery's.
         * @enum {string}
         */
        AttentionProducer: "recovery" | "agent_request" | "pull_request";
        /**
         * @description Why a live agent session needs the user's attention.
         *
         *     Orthogonal to [`SessionStatus`]: a session waiting on a permission prompt
         *     is still `running` as far as its lifecycle goes, it just cannot make
         *     progress until someone looks at it.
         * @enum {string}
         */
        AttentionReason: "waiting_permission" | "waiting_input" | "waiting_user" | "agent_error" | "disconnected" | "stalled" | "exhausted";
        /** @description Response of `GET /v1/stats/attention`: how much did it need me? */
        AttentionStatsDto: {
            /**
             * Format: int64
             * @default 0
             */
            exhaustions: number;
            /** @default [] */
            flags: components["schemas"]["AttentionFlagDto"][];
            /**
             * @default {
             *       "permissions": 0,
             *       "questions": 0,
             *       "stalls": 0,
             *       "total": 0,
             *       "person_secs": 0
             *     }
             */
            interventions: components["schemas"]["AttentionInterventionsDto"];
            /**
             * @default {
             *       "total": 0,
             *       "person_share": 0,
             *       "by_decider": []
             *     }
             */
            permissions: components["schemas"]["PermissionStatsDto"];
            /**
             * Format: int64
             * @default 0
             */
            sessions_failed: number;
            /**
             * Format: int64
             * @default 0
             */
            sessions_stalled: number;
        };
        /** @description One entity an item's blocker affects. */
        AttentionSubjectDto: {
            id: string;
            kind: components["schemas"]["AttentionSubjectKind"];
            /**
             * @description What to call it where there is room for one word more than the id —
             *     a task's title, a session's model.
             */
            label: string;
        };
        /**
         * @description What an item is about: an entity whose work the blocker touches.
         * @enum {string}
         */
        AttentionSubjectKind: "goal" | "task" | "session" | "repository" | "pull_request";
        /**
         * @description Where opening an item takes a client. Opening a target never resolves
         *     the item on its own — only the thing the item names doing so does.
         */
        AttentionTarget: {
            /** @enum {string} */
            kind: "console";
            session_id: string;
        } | {
            /** @enum {string} */
            kind: "task";
            task_id: string;
        } | {
            /** @enum {string} */
            kind: "pull_request";
            pull_request_id: string;
        } | {
            /** @enum {string} */
            kind: "settings";
            section: string;
        };
        /** @description A binary as the daemon can — or cannot — find it. */
        BinaryDto: {
            /**
             * @description Whether it holds credentials for the service it speaks to, for the
             *     binaries that hold any: `gh auth status` and `glab auth status`, asked
             *     of the daemon's own environment because that is where the polling
             *     runs. `None` for a binary with nothing to sign in to — git — and for
             *     one that was not found to ask.
             *
             *     The distinction it exists for is the one that used to be invisible: a
             *     `gh` that is installed and signed out answers every poll of a pull
             *     request with a failure, and a task published to a forge is then
             *     watched by nothing.
             */
            authenticated?: boolean | null;
            /**
             * @description Executable name as it is looked up on PATH ("git", "gh").
             * @example git
             */
            name: string;
            /** @description Absolute path, when it was found. */
            path?: string | null;
            /**
             * @description First line of its version output, when it answered in time. A binary
             *     that is found but does not answer keeps its path and no version.
             */
            version?: string | null;
        };
        /**
         * @description The step a time axis is drawn at: a bar an hour over a very short span, a
         *     bar a day over a short one, a bar a week over a long one.
         * @enum {string}
         */
        BucketDto: "hour" | "day" | "week";
        /**
         * @description Body of `POST /v1/goals/{id}/complete`: the orchestrator says the goal is
         *     done. Its call, not the user's, and it carries nothing — every task being
         *     finished or cancelled is the whole of the argument, and the daemon checks
         *     that itself.
         */
        CompleteGoalRequest: Record<string, never>;
        /**
         * @description Body of `POST /v1/tasks/{id}/step/complete`: the current column's agent
         *     hands the task to the next column, or finishes it from the last one.
         */
        CompleteStepRequest: {
            /** @description The commit the task landed as, which the `merged` gate checks. */
            merge_commit?: string | null;
            /** @description What the next column's agent is briefed with. */
            reason: string;
        };
        /**
         * @description Body of `POST /v1/sessions/{id}/console/input`.
         *
         *     While a permission request is pending, the text selects that request's
         *     option; otherwise it becomes a fresh `session/prompt`, sent at once or
         *     queued behind the turn still running.
         */
        ConsoleInputRequest: {
            text: string;
        };
        CreateGoalRequest: {
            description?: string;
            /**
             * @description The reasoning effort to run that model at, one of the efforts `GET
             *     /v1/models` lists for it; anything else is refused. Omitted (or
             *     "default") = whatever the agent runs the model at.
             * @example high
             */
            effort?: string | null;
            issue_url?: string | null;
            /**
             * @description What the orchestrator runs on, `<agent>:<model>` — the id of an agent
             *     in the ACP registry and, after the `:`, the model of it:
             *     `codex-acp:gpt-5.3-codex`, `opencode:ollama/llama3:8b`. Required —
             *     a model is required, and no agent default stands in for one. The model
             *     half is free text, handed to that agent as typed; a string naming no
             *     registry agent is refused, and so are the empty string and the word
             *     "default".
             * @example codex-acp:gpt-5.3-codex
             */
            model: string;
            /** @description Ids of registered repositories (`POST /v1/repositories`); at least one. */
            repository_ids: string[];
            title: string;
            /**
             * @description The workflow every task of the goal runs on, a name of the catalog
             *     (`GET /v1/workflows`). Omitted = the default of the first repository.
             *     It cannot change once the goal is created.
             */
            workflow?: string | null;
        };
        CreateRepositoryRequest: {
            /** @description Omit for the repo's currently checked-out branch. */
            base_branch?: string | null;
            /**
             * @description The workflow a new goal in this repository runs on where its request
             *     names none. Omit for `develop-review-merge`.
             */
            default_workflow?: string | null;
            description?: string | null;
            forge?: null | components["schemas"]["ForgeUpdate"];
            /**
             * @description Absolute path of an existing git work tree.
             * @example /home/me/dev/ariadne
             */
            path: string;
            permission_mode?: null | components["schemas"]["PermissionMode"];
        };
        CreateSkillRequest: {
            /**
             * @description The whole `SKILL.md`, frontmatter included. A skill of the user's own
             *     has no shipped text behind it, so it carries its own.
             */
            document: string;
            /**
             * @description Kebab-case, and free: a name Ariadne already ships is refused.
             * @example api-design
             */
            name: string;
        };
        CreateTaskRequest: {
            /**
             * @description The agents to staff: one per column of the goal's workflow, each with
             *     its own model. A column left out is named when the plan is finalized.
             */
            agents: components["schemas"]["AgentAssignment"][];
            /** @description Task ids this task depends on. */
            depends_on?: string[];
            description?: string;
            /**
             * @description Id of one of the goal's repositories; may be omitted when the goal
             *     works in exactly one.
             */
            repo_id?: string | null;
            title: string;
        };
        CreateWorkflowRequest: {
            /**
             * @description The whole workflow document. Its `workflow <name>` line must equal
             *     `name`.
             */
            document: string;
            /**
             * @description Kebab-case, and free: a name Ariadne already ships is refused.
             * @example my-workflow
             */
            name: string;
        };
        /** @description The daemon's own environment, as `ariadne doctor` renders it. */
        DaemonReportDto: {
            /**
             * @description Every registry ACP agent and its cached discovery result: the agents
             *     a session can be spawned on.
             */
            acp_agents?: components["schemas"]["AcpAgentDto"][];
            db: components["schemas"]["PathStateDto"];
            /** @description Home directory the daemon resolved, and the socket it listens on. */
            home: string;
            /** @description The daemon's `PATH`, the one every agent and git lookup uses. */
            path?: string | null;
            /**
             * @description The Python interpreter the model's install runs on (022). It is reported
             *     apart from `tools` because it answers a question of its own: not
             *     whether it is there, but whether it is new enough.
             */
            python: components["schemas"]["PythonDto"];
            socket_path: string;
            /**
             * @description The other binaries the daemon runs: git, without which no worktree can
             *     be cut at all, and the forge CLIs `gh` and `glab`, which are what a
             *     published task is watched through.
             */
            tools: components["schemas"]["BinaryDto"][];
            /** @example 0.1.0 */
            version: string;
            worktree_root: components["schemas"]["PathStateDto"];
        };
        /** @description Payload of the deletion events: the id of the gone entity. */
        DeletedDto: {
            id: string;
        };
        /**
         * @description Where a Kev flavour runs.
         * @enum {string}
         */
        Device: "mlx" | "cuda" | "cpu";
        /** @description Whether one device can run one flavour, and why not. */
        DeviceOptionDto: {
            can_run: boolean;
            device: components["schemas"]["Device"];
            /** @description Why it cannot, e.g. `needs 24 GB VRAM, found 8 GB`. */
            reason?: string | null;
            /** @description A note only: the flavour can still be chosen on this device. */
            slow: boolean;
        };
        /**
         * @description One domain event. Serialized as `{"event": "<kind>", "data": <payload>}`;
         *     on the SSE wire the kind becomes the `event:` field and the payload alone
         *     the `data:` field.
         */
        DomainEvent: {
            data: components["schemas"]["GoalDto"];
            /** @enum {string} */
            event: "goal_created";
        } | {
            /** @description Covers status changes: finalize, cancel, completion. */
            data: components["schemas"]["GoalDto"];
            /** @enum {string} */
            event: "goal_updated";
        } | {
            /** @description A terminal goal was deleted, its tasks with it. */
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "goal_deleted";
        } | {
            data: components["schemas"]["TaskDto"];
            /** @enum {string} */
            event: "task_created";
        } | {
            /** @description Covers status transitions, edits, stall flags and worktree changes. */
            data: components["schemas"]["TaskUpdatedDto"];
            /** @enum {string} */
            event: "task_updated";
        } | {
            /**
             * @description Covers commits made in the task's worktree: the branch head moved, so
             *     the task's diff against its base is no longer the one a client holds.
             */
            data: components["schemas"]["TaskBranchDto"];
            /** @enum {string} */
            event: "task_branch_updated";
        } | {
            /** @description One agent said something to another. */
            data: components["schemas"]["MessageDto"];
            /** @enum {string} */
            event: "message_sent";
        } | {
            data: components["schemas"]["SessionDto"];
            /** @enum {string} */
            event: "session_created";
        } | {
            /** @description Covers status changes: kill, resume, exit, activity. */
            data: components["schemas"]["SessionDto"];
            /** @enum {string} */
            event: "session_updated";
        } | {
            /**
             * @description An agent event the ACP runtime recorded, without its payload: read the
             *     whole event from `GET /v1/events`.
             */
            data: components["schemas"]["AgentEventSummaryDto"];
            /** @enum {string} */
            event: "agent_event";
        } | {
            data: components["schemas"]["SkillDto"];
            /** @enum {string} */
            event: "skill_created";
        } | {
            data: components["schemas"]["SkillDto"];
            /** @enum {string} */
            event: "skill_updated";
        } | {
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "skill_deleted";
        } | {
            data: components["schemas"]["WorkflowDto"];
            /** @enum {string} */
            event: "workflow_created";
        } | {
            data: components["schemas"]["WorkflowDto"];
            /** @enum {string} */
            event: "workflow_updated";
        } | {
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "workflow_deleted";
        } | {
            data: components["schemas"]["RepositoryDto"];
            /** @enum {string} */
            event: "repository_created";
        } | {
            data: components["schemas"]["RepositoryDto"];
            /** @enum {string} */
            event: "repository_updated";
        } | {
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "repository_deleted";
        } | {
            /** @description The AI permission settings or the state of its install moved (022). */
            data: components["schemas"]["AiPermissionsStatusDto"];
            /** @enum {string} */
            event: "ai_permissions_updated";
        } | {
            data: components["schemas"]["LearnedPermissionDto"];
            /** @enum {string} */
            event: "learned_permission_created";
        } | {
            data: components["schemas"]["LearnedPermissionDto"];
            /** @enum {string} */
            event: "learned_permission_updated";
        } | {
            data: components["schemas"]["LearnedPermissionDto"];
            /** @enum {string} */
            event: "learned_permission_deleted";
        } | {
            /** @description The forge settings or the tunnel state moved (027). */
            data: components["schemas"]["ForgeTunnelDto"];
            /** @enum {string} */
            event: "forge_settings_updated";
        } | {
            /**
             * @description The open issues of one repository moved on its forge (028): read them
             *     again from `GET /v1/repositories/{id}/issues`.
             */
            data: components["schemas"]["IssuesChangedDto"];
            /** @enum {string} */
            event: "issues_changed";
        } | {
            /**
             * @description The requests of one repository moved: on its forge, or in what
             *     Ariadne keeps of them (026). Read them again; they are the forge's.
             */
            data: components["schemas"]["PullRequestsChangedDto"];
            /** @enum {string} */
            event: "pull_requests_changed";
        };
        /**
         * @description One reasoning effort an entry can be run at: the name it is passed by, and
         *     what spending it buys.
         *
         *     At most one effort of a model is the `default`: what its agent runs it at
         *     when a task pins no effort at all. None of them are where the agent has no
         *     default to name.
         */
        EffortDto: {
            /** @description Whether this is what the agent runs the model at when none is passed. */
            default: boolean;
            /**
             * @description What spending this effort buys, where the agent describes it. `null`
             *     where nothing knows.
             */
            description?: string | null;
            /** @example high */
            id: string;
        };
        /**
         * @description Which end of the recorded events a page of `GET /v1/events` is taken from.
         * @enum {string}
         */
        EventOrder: "asc" | "desc";
        /**
         * @description Body of `POST /v1/tasks/{id}/step/fail`: the current column's agent hands
         *     the task back to the previous column, or fails it from the first one.
         */
        FailStepRequest: {
            /** @description What the previous column's agent is told to fix. */
            reason: string;
        };
        /** @description One check that failed on a request's head. */
        FailedCheckDto: {
            /**
             * @description The forge's own word for how it ended: `failure`, `cancelled` and
             *     the like.
             */
            conclusion: string;
            name: string;
            /** @description Where the forge shows the check's run; empty where it names none. */
            url: string;
        };
        /**
         * @description Body of `POST /v1/goals/{id}/finalize`: the orchestrator ends planning and
         *     execution starts. The orchestrator's call, not the user's, and it carries
         *     nothing — the plan is the tasks it wrote.
         */
        FinalizePlanRequest: Record<string, never>;
        /**
         * @description A Kev flavour: how large a model to run.
         * @enum {string}
         */
        Flavour: "0.8b" | "4b" | "9b" | "27b";
        /** @description One flavour with every device it might run on. */
        FlavourOptionsDto: {
            devices: components["schemas"]["DeviceOptionDto"][];
            flavour: components["schemas"]["Flavour"];
        };
        /** @description The forge a repository's remote is on, and whether Ariadne works with it. */
        ForgeDto: {
            enabled: boolean;
            /**
             * @description Lower-cased, like `owner` and `name`.
             * @example github.com
             */
            host: string;
            kind: components["schemas"]["ForgeKind"];
            /** @description The account the forge CLI is signed in as, stored on enable. */
            login?: string | null;
            name: string;
            owner: string;
            /** @description The remote it was read off, `origin` where there is one. */
            remote: string;
            review_effort?: string | null;
            /** @description The pin of the session that reviews a request; null starts none. */
            review_model?: string | null;
            webhook: components["schemas"]["WebhookDto"];
        };
        /**
         * @description The forge a repository's remote is on (025): which CLI speaks to it.
         * @enum {string}
         */
        ForgeKind: "github" | "gitlab";
        /**
         * @description The forge settings and the tunnel state, as `GET /v1/forge/tunnel` and
         *     `forge_settings_updated` carry them (027).
         */
        ForgeTunnelDto: {
            /** @description The switch: whether the daemon opens a tunnel. */
            enabled: boolean;
            /** @description Why the last attempt failed, while the tunnel is down. */
            error?: string | null;
            /**
             * @description The bound webhook listener address the tunnel forwards to.
             * @example 127.0.0.1:49152
             */
            listen?: string | null;
            /** @description When the tunnel entered its state. */
            since: string;
            state: components["schemas"]["TunnelState"];
            /** @description The public URL while the tunnel is up. */
            url?: string | null;
        };
        /**
         * @description A change to the forge integration; absent fields stay unchanged. A model
         *     written empty clears that role's pin and its effort.
         */
        ForgeUpdate: {
            enabled?: boolean | null;
            review_effort?: string | null;
            review_model?: string | null;
        };
        GoalDto: {
            created_at: string;
            description: string;
            /**
             * @description The reasoning effort that model is run at, pinned like `model`. None =
             *     whatever the agent runs it at on its own.
             * @example high
             */
            effort?: string | null;
            id: string;
            issue_url?: string | null;
            /**
             * @description What the orchestrator runs on, `<agent>:<model>`: the registry agent
             *     and, after the `:`, the model of it.
             * @example claude-acp:claude-opus-5
             */
            model: string;
            /** @description Whether this goal has an orchestrator for its lifetime. */
            orchestrated: boolean;
            /**
             * @description The registered repositories the goal works in, as they stand now: a
             *     goal references them, so an edit to one shows up here.
             */
            repos: components["schemas"]["RepositoryDto"][];
            status: components["schemas"]["GoalStatus"];
            /**
             * @description The columns of that workflow as they were when the goal was created:
             *     a later edit of the catalog reaches later goals alone.
             */
            steps: components["schemas"]["WorkflowStepDto"][];
            title: string;
            updated_at: string;
            /** @description What the agents of this goal have spent between them. */
            usage: components["schemas"]["GoalUsageDto"];
            /**
             * @description The workflow every task of the goal runs on, chosen when the goal was
             *     created.
             */
            workflow: string;
        };
        /**
         * @description Goal lifecycle status.
         * @enum {string}
         */
        GoalStatus: "planning" | "active" | "completed" | "cancelled";
        /**
         * @description What a goal cost, by the seat that spent it, and by the agent of every
         *     column of every task under it.
         */
        GoalUsageDto: {
            /**
             * @description One entry per agent of every task of the goal that has a session,
             *     task by task and column by column.
             */
            agents: components["schemas"]["AgentUsageDto"][];
            /** @description The orchestrator's sessions, which belong to no task. */
            orchestrator: components["schemas"]["TokenUsageDto"];
            /** @description Every session of the goal summed, the orchestrator's included. */
            total: components["schemas"]["TokenUsageDto"];
        };
        /** @description The GPU with the largest VRAM the daemon's probe found. */
        GpuDto: {
            name: string;
            /** Format: int64 */
            vram_bytes: number;
        };
        /**
         * @description The machine the daemon runs on, as far as choosing a Kev flavour and
         *     device cares.
         */
        HardwareDto: {
            /** @example aarch64 */
            arch: string;
            gpu?: null | components["schemas"]["GpuDto"];
            /** Format: int64 */
            memory_bytes: number;
            /** @example macos */
            os: string;
        };
        /** @description Response of `GET /v1/health`. */
        HealthResponse: {
            /**
             * @description Always "ok" when the daemon is able to answer.
             * @example ok
             */
            status: string;
            /**
             * Format: int64
             * @description Seconds since the daemon started.
             */
            uptime_secs: number;
        };
        /**
         * @description Payload of the `heartbeat` control event.
         *
         *     Sent when a connection opens and on every idle interval afterwards, so a
         *     client can tell a live daemon from a dead one without polling, and tell a
         *     restarted daemon from the one it was talking to: `started_at` changes when
         *     the daemon does.
         */
        HeartbeatDto: {
            /** @description When this daemon started, RFC 3339 in UTC. */
            started_at: string;
            /** @description The daemon's version, as `GET /v1/version` reports it. */
            version: string;
        };
        IssueDto: {
            assignees: string[];
            body: string;
            labels: string[];
            /** Format: int64 */
            number: number;
            title: string;
            updated_at: string;
            url: string;
        };
        /** @description Payload of `issues_changed`: the repository whose open issues moved. */
        IssuesChangedDto: {
            repository_id: string;
        };
        LeadTimeDto: {
            /** Format: double */
            mean_secs: number;
            /** Format: double */
            median_secs: number;
            /** Format: double */
            p90_secs: number;
        };
        /**
         * @description One user choice or denial of an ACP permission request, keyed by the
         *     repository, the tool name, the level and the normalized input.
         */
        LearnedPermissionDto: {
            created_at: string;
            /**
             * @description The command family of a `Bash` request, such as `git rebase`, else
             *     the tool name.
             * @example git rebase
             */
            family: string;
            id: string;
            /**
             * @description The kept fields of `rawInput` as compact JSON with sorted keys, its
             *     one-time values replaced by placeholders such as `<HASH>`.
             * @example {"command":"git rebase main"}
             */
            key: string;
            level: components["schemas"]["LearnedPermissionLevel"];
            /** @description The ACP `options`. */
            options: unknown;
            /** @description The model decision, when the model was called; null otherwise. */
            output: unknown;
            repository_id: string;
            /**
             * @description The derived risk tags of the request. The row answers only a request
             *     whose tags are all among them.
             */
            risk_tags: string[];
            scope: components["schemas"]["LearnedPermissionScope"];
            /** @description The option id of the final choice. */
            selected_option: string;
            /** @description The repository permission mode at the time of the decision. */
            target: components["schemas"]["LearnedPermissionTarget"];
            /** @description The ACP `toolCall`, its `rawInput` with sorted keys. */
            tool_call: unknown;
            /**
             * @description `toolCall.name`, else `toolCall._meta.claudeCode.toolName`, else `toolCall.title`.
             * @example Bash
             */
            tool_name: string;
            updated_at: string;
        };
        /**
         * @description How much of a request a learned permission answers for.
         * @enum {string}
         */
        LearnedPermissionLevel: "once" | "command" | "family";
        /**
         * @description Where a learned permission answers.
         * @enum {string}
         */
        LearnedPermissionScope: "repository" | "all";
        /**
         * @description The repository permission mode a learned permission was decided under.
         * @enum {string}
         */
        LearnedPermissionTarget: "auto" | "ask" | "learn" | "ai";
        LearnedPermissionsResponse: {
            items: components["schemas"]["LearnedPermissionDto"][];
        };
        /** @description One captured daemon log line. */
        LogLineDto: {
            /**
             * @description Log level as tracing prints it.
             * @example INFO
             */
            level: string;
            /** @description Message followed by the event's fields as ` key=value` pairs. */
            message: string;
            /**
             * @description Module path the event was emitted from.
             * @example ariadne_daemon::scheduler
             */
            target: string;
            /**
             * @description When the event was recorded, RFC 3339.
             * @example 2026-08-18T12:34:56.789012Z
             */
            ts: string;
        };
        /** @description Response of `GET /v1/logs`: the in-memory ring buffer, oldest first. */
        LogSnapshotResponse: {
            lines: components["schemas"]["LogLineDto"][];
        };
        MessageDto: {
            body: string;
            created_at: string;
            /**
             * @description When it was handed to the recipient's agent, or None while it is still
             *     waiting for one to be free.
             */
            delivered_at?: string | null;
            from_actor: components["schemas"]["Actor"];
            /**
             * @description The staffed agent that sent it, or None for the orchestrator, the
             *     daemon and the user.
             */
            from_agent_id?: string | null;
            from_session?: string | null;
            goal_id: string;
            id: string;
            kind: components["schemas"]["MessageKind"];
            /** @description The task it is about, or None for a message about the goal itself. */
            task_id?: string | null;
            to_actor: components["schemas"]["Actor"];
            /** @description The staffed agent it is for, or None for the orchestrator. */
            to_agent_id?: string | null;
        };
        /**
         * @description What one agent is saying to another.
         *
         *     Agents talk to each other through one channel, and one kind carries
         *     everything they say on it, whether it asks something or answers it. There
         *     is no `answer` kind and no `reply` tool: an answer is a message to whoever
         *     asked, addressed the way the question was, so nothing threads. Each
         *     message reaches its agent as a turn — which is why the tool that sends one
         *     takes questions and answers and nothing else, no confirmation and no
         *     thanks.
         *
         *     A review used to run on the channel too, as three kinds of its own; a
         *     workflow column moves a task through the step routes now, and the one
         *     kind left is what the agents say.
         * @enum {string}
         */
        MessageKind: "message";
        /**
         * @description One thing an agent can be pinned to, as served by `GET /v1/models`: a
         *     registry agent on a model discovery found it offering
         *     (`claude-acp:claude-opus-5`). Every entry names both halves — there
         *     is no bare-agent entry, because a model is required wherever an agent is
         *     pinned.
         *
         *     The id is what a request writes as its `model`, whole. `agent_id` is its
         *     registry prefix. Discovery supplies the description and efforts.
         *     The user sets the enabled flag and optional rank independently.
         */
        ModelDto: {
            /** @description Stable registry agent id. */
            agent_id: string;
            /** @description One line about the model, which is what a picker shows beside the id. */
            description?: string | null;
            /**
             * @description The reasoning efforts this entry can be run at, cheapest first; empty
             *     where the model takes none, or where nothing knows what it takes.
             */
            efforts: components["schemas"]["EffortDto"][];
            /**
             * @description Whether an agent can be staffed on this entry. Every model is enabled
             *     until the user turns it off; a disabled one stays in the catalog,
             *     where it is shown as off and refused as a pin.
             */
            enabled: boolean;
            /** @example claude-acp:claude-opus-5 */
            id: string;
            rank?: null | components["schemas"]["ModelRank"];
        };
        /**
         * @description A user-set rank, independent of discovery and model availability.
         * @enum {string}
         */
        ModelRank: "frontier" | "balanced" | "fast" | "local";
        /** @description What one model spent, every seat that ran on it pooled together. */
        ModelSpendDto: {
            /** Format: int64 */
            cached_input_tokens: number;
            /** Format: int64 */
            input_tokens: number;
            model: string;
            /** Format: int64 */
            output_tokens: number;
            /**
             * Format: double
             * @description `input_tokens + output_tokens` over the same total of every model.
             */
            share: number;
        };
        /** @description What one model did in one seat. */
        ModelStatDto: {
            /**
             * Format: int64
             * @description Distinct goals with a session of this model in this seat.
             */
            goals: number;
            /**
             * Format: int64
             * @description The messages this model sent in this seat.
             */
            messages: number;
            model: string;
            seat?: string | null;
            /**
             * Format: int64
             * @description Distinct tasks with a session of this model in this seat.
             */
            tasks: number;
            /**
             * Format: int64
             * @description The tasks this model's column ended `finished`.
             */
            tasks_finished: number;
            /**
             * Format: double
             * @description The sum of the session lifetimes.
             */
            time_secs: number;
            /**
             * Format: int64
             * @description Input and output tokens of the ended sessions. Cached tokens are part of input.
             */
            tokens: number;
        };
        /** @description Models compared within each seat. */
        ModelStatsDto: {
            items: components["schemas"]["ModelStatDto"][];
        };
        /**
         * @description Start a loose session: a new conversation with an agent, in a directory,
         *     with no goal, task or seat behind it.
         */
        NewSessionRequest: {
            /** @description The effort to run that model at; omitted = the agent's own. */
            effort?: string | null;
            /**
             * @description The model to run, `<agent>:<model>`: a registry agent (`GET
             *     /v1/acp-agents`) and a model of it (`GET /v1/models`).
             * @example claude-acp:sonnet
             */
            model: string;
            /** @description The absolute path of an existing directory the agent works in. */
            working_directory: string;
        };
        /**
         * @description The agent of the `pr` column asking the daemon to open the pull or merge
         *     request its task lands by. The daemon runs the forge's own CLI, so this
         *     carries only what the agent cannot read off the task or the repository
         *     itself.
         */
        OpenPullRequestRequest: {
            /** @description Filled from the repository's own request template. */
            body: string;
            /** @description Opens the request as a draft. Defaults to false. */
            draft?: boolean;
            /** @description Titled by the repository's own commit conventions. */
            title: string;
        };
        ParseWorkflowRequest: {
            document: string;
        };
        /**
         * @description Response of `POST /v1/workflows/parse`: a document's name and columns,
         *     read without saving it anywhere.
         */
        ParsedWorkflowDto: {
            name: string;
            steps: components["schemas"]["WorkflowStepDto"][];
        };
        /** @description A file or directory the daemon depends on. */
        PathStateDto: {
            exists: boolean;
            path: string;
            /**
             * @description Whether the daemon may write it, asked of the kernel (`access(2)`)
             *     rather than inferred from the permission bits, which say nothing
             *     about the user the daemon happens to run as. For a path that does not
             *     exist yet this is its directory's answer: whether it could be created.
             *     Nothing is written to find out.
             */
            writable: boolean;
        };
        /**
         * @description What a finished task spends, on average: the tokens of every session of a
         *     task that finished, divided by how many tasks finished. 0 where none.
         */
        PerFinishedTaskDto: {
            /** Format: double */
            input_tokens: number;
            /** Format: double */
            output_tokens: number;
            /** Format: int64 */
            tasks: number;
        };
        PermissionDeciderDto: {
            /** Format: int64 */
            allowed: number;
            /** Format: int64 */
            cancelled: number;
            decided_by: string;
            /** Format: int64 */
            denied: number;
            /** Format: double */
            mean_wait_ms: number;
            /** Format: int64 */
            total: number;
        };
        /**
         * @description How the ACP runtime answers a tool permission request.
         * @enum {string}
         */
        PermissionMode: "auto" | "ask" | "learn" | "ai";
        /** @description The fields in a `permission.replied` event payload. */
        PermissionReplyDto: {
            ai_error?: string | null;
            /** Format: double */
            allow_threshold?: number | null;
            cap?: string | null;
            console_option_id?: string | null;
            /** Format: double */
            danger?: number | null;
            decided_by: string;
            /** Format: double */
            deny_threshold?: number | null;
            label?: string | null;
            /** @description The row that answered without a console choice. */
            learned_id: string | null;
            /** @description The normalized command key or family of the answering row. */
            learned_key: string | null;
            /** @description `command` or `family` when a row answered. */
            learned_level: string | null;
            operation?: string | null;
            option_id?: string | null;
            probabilities?: unknown;
            risk_tags?: string[] | null;
            session_id: unknown;
        };
        PermissionStatsDto: {
            by_decider: components["schemas"]["PermissionDeciderDto"][];
            /** Format: double */
            person_share: number;
            /** Format: int64 */
            total: number;
        };
        PersonWaitDto: {
            /** Format: double */
            median_secs: number;
            /** Format: int64 */
            prompts: number;
            /** Format: double */
            total_secs: number;
        };
        /**
         * @description One comment on a request, as the forge holds it now, read live (026),
         *     with the marks Ariadne keeps of it.
         */
        PullRequestCommentDto: {
            /** @description A later comment in the thread is by the integration login. */
            answered: boolean;
            author_is_bot: boolean;
            author_login: string;
            body: string;
            created_at: string;
            /** @description An Ariadne review session posted it (029). */
            from_review?: boolean;
            /**
             * @description The forge's id of the comment, `rc-<n>`, `ic-<n>`, `rv-<n>` or
             *     `note-<n>`: what `reply` and `resolve` take.
             */
            id: string;
            in_reply_to?: string | null;
            /** @description `review_comment`, `issue_comment` or `review`. */
            kind: string;
            /** Format: int64 */
            line?: number | null;
            path?: string | null;
            pull_request_id: string;
            /** @description The forge reports the thread resolved. */
            resolved: boolean;
            /**
             * @description The forge's thread or discussion; the conversation of the request
             *     is one thread.
             */
            thread_id: string;
            /** @description When the request's session was told of it. */
            told_at?: string | null;
        };
        /**
         * @description A pull request as the forge holds it now, read live (026), with what
         *     Ariadne keeps of it where Ariadne works on it: the request a task opened,
         *     one that asks for the user's review on a repository with a review pin,
         *     or one of the user's they asked Ariadne to review.
         */
        PullRequestDto: {
            author_login: string;
            base_branch: string;
            /** @description Whether the base branch has commits the head does not. */
            behind_base?: boolean;
            /** @description The request's description. */
            body?: string;
            /** @description The rolled-up checks: `pending`, `success`, `failure` or `none`. */
            checks: string;
            draft: boolean;
            /** @description The checks that failed on the head, read as `unanswered_comments` is. */
            failed_checks?: components["schemas"]["FailedCheckDto"][];
            head_branch: string;
            head_repo?: string | null;
            head_sha: string;
            /**
             * @description Ariadne's id of the request while it works on it; null on a request
             *     nobody works on, which the forge alone holds.
             */
            id?: string | null;
            /**
             * @description The forge's own mergeability for the head now: `clean`, `blocked`,
             *     `dirty` or `unknown` where the forge has not finished computing it.
             */
            mergeable?: string;
            /** Format: int64 */
            number: number;
            opened_at: string;
            /** @description The task that opened the request: its author keeps it (005). */
            origin_task_id?: string | null;
            /** @description Whether the request's session reported it ready to merge. */
            ready?: boolean;
            repository_id: string;
            /**
             * @description Whether the user asked Ariadne to review this request of their own
             *     (029).
             */
            review_asked?: boolean;
            review_decision: string;
            review_effort?: string | null;
            /** @description The pin the user picked for that review. */
            review_model?: string | null;
            /** @description Whether the request asks for the user's review (029). */
            review_requested?: boolean;
            /** @description The skills that review loads beside `pr-reviewer`. */
            review_skills?: string[];
            /** @description `author` for a request of the user's, `reviewer` for any other. */
            role: string;
            /**
             * @description The newest session on this request, if any: its review session
             *     (029), or the author of the task that opened it (005).
             */
            session_id?: string | null;
            /** @description `open`, `merged` or `closed`. */
            state: string;
            title: string;
            /**
             * Format: int64
             * @description The threads that wait on the user's login. Read where Ariadne works
             *     on the request, or where the request is read on its own; 0 on a row
             *     of a list nobody works on.
             */
            unanswered_comments?: number;
            /** @description When the forge last saw the request move. */
            updated_at: string;
            url: string;
        };
        /** @description An open request a search found that is not the user's own. */
        PullRequestMatchDto: {
            author_login: string;
            /** Format: int64 */
            number: number;
            title: string;
            /** @description Whether Ariadne works on it. */
            tracked: boolean;
            url: string;
        };
        /** @description Payload of `pull_requests_changed`: the repository whose requests moved. */
        PullRequestsChangedDto: {
            repository_id: string;
        };
        /** @description The Python interpreter the daemon found, as it answered `--version`. */
        PythonDto: {
            /** @description Whether it is Python 3.12 or 3.13, which the model needs. */
            ok: boolean;
            /**
             * @description Absolute path, when one was found.
             * @example /usr/bin/python3
             */
            path?: string | null;
            /**
             * @description The version it printed, without the `Python ` in front of it.
             * @example 3.12.1
             */
            version?: string | null;
        };
        /** @description Body of `POST /v1/pull-requests/{id}/comments/{comment_id}/reply`. */
        ReplyCommentRequest: {
            body: string;
        };
        /**
         * @description Body of `POST /v1/pull-requests/{id}/report`: what the request's session
         *     says of it.
         */
        ReportPullRequestRequest: {
            /** @description Every required approval and check reads green. */
            ready?: boolean | null;
            /** @description The head a reviewer session posted its review on (029). */
            reviewed_sha?: string | null;
        };
        RepositoryDto: {
            base_branch: string;
            created_at: string;
            /**
             * @description The workflow a new goal runs on where its request names none, a name
             *     of the catalog (`GET /v1/workflows`).
             * @example develop-review-merge
             */
            default_workflow: string;
            description?: string | null;
            forge?: null | components["schemas"]["ForgeDto"];
            id: string;
            /** @description Absolute path of the checkout. */
            path: string;
            /**
             * @description How the ACP permission requests of every session in this checkout are
             *     answered.
             */
            permission_mode: components["schemas"]["PermissionMode"];
            updated_at: string;
        };
        /** @description Resume a stored conversation without a goal, task or seat. */
        ResumeOutsideSessionRequest: {
            agent_id: string;
            internal_session_id: string;
        };
        /**
         * @description Payload of the `resync` control event.
         *
         *     Sent as the last message of a connection that fell too far behind: the
         *     daemon dropped `missed` events for it and closes the stream. The client
         *     must refetch its REST state before following the stream again (an
         *     `EventSource` reconnects on its own).
         */
        ResyncDto: {
            /**
             * Format: int64
             * @description Events this connection lost. Informational: they cannot be recovered.
             */
            missed: number;
        };
        /** @description One inline comment of a review (029). */
        ReviewCommentRequest: {
            /** @description What goes wrong, and how to fix it. */
            body: string;
            /**
             * Format: int64
             * @description The line in the new version of the file.
             */
            line: number;
            path: string;
            /** @description `P0`, `P1` or `P2`. */
            priority: string;
            /** @description A short title of the defect; the comment opens on `[P0] Title`. */
            title?: string;
        };
        /**
         * @description Where an agent sits: the orchestrator of a goal, the agent of one workflow
         *     column of a task, or the reviewer of a pull request (029).
         *
         *     A seat is a position, not an identity. Every agent below the orchestrator
         *     is generic, and what it can do comes from the skills it loads; the seat is
         *     only what the state machine and the launcher need to know about it.
         * @enum {string}
         */
        Seat: "orchestrator" | "agent" | "reviewer";
        /**
         * @description Body of `POST /v1/tasks/{id}/messages` and `POST /v1/goals/{id}/messages`.
         *
         *     Who it is *from* comes from the session header rather than the body: an
         *     agent cannot send a message as somebody else, and a message with no session
         *     behind it is the user's.
         */
        SendMessageRequest: {
            body: string;
            /** @description Who it is for. `orchestrator` needs no agent id — a goal has one. */
            to_actor: components["schemas"]["Actor"];
            /**
             * @description The staffed agent it is for, as `GET /v1/tasks/{id}` lists them.
             *     Required for `agent`, refused for the orchestrator.
             */
            to_agent_id?: string | null;
        };
        SessionDto: {
            attention_reason?: null | components["schemas"]["AttentionReason"];
            /** @description When the current `attention_reason` was first raised. */
            attention_since?: string | null;
            /** Format: int64 */
            context_size?: number | null;
            /**
             * Format: int64
             * @description The context window position the agent most recently reported. Both
             *     fields stay null until the agent sends a `usage_update`.
             */
            context_used?: number | null;
            created_at: string;
            /**
             * @description Effort that model was launched at, off the same pin as `model`; null =
             *     whatever the agent runs it at.
             * @example high
             */
            effort?: string | null;
            ended_at?: string | null;
            goal_id?: string | null;
            id: string;
            /** @description The ACP agent's own session id. */
            internal_session_id?: string | null;
            last_activity_at?: string | null;
            /**
             * @description Model requested at launch, `<agent>:<model>`: the registry agent the
             *     session runs on, and the model of it.
             */
            model: string;
            /**
             * @description The pull request this session watches (026); null for every other
             *     session.
             */
            pull_request_id?: string | null;
            seat?: null | components["schemas"]["Seat"];
            status: components["schemas"]["SessionStatus"];
            /**
             * @description The session this one replaced on its seat, when a switch started it
             *     (`POST /v1/sessions/{id}/switch`); null otherwise.
             */
            switched_from?: string | null;
            /** @description The staffed agent this session runs; None for an orchestrator or loose session. */
            task_agent_id?: string | null;
            /** @description None for an orchestrator or a loose session. */
            task_id?: string | null;
            /**
             * @description A loose session's own title: the first prompt of the conversation it
             *     resumed, or the first one typed into it. Null on a task's or a goal's
             *     session, which goes by its work's title.
             */
            title?: string | null;
            /**
             * @description What this session's agent has spent, summed over every transcript it
             *     reported under. Zeros while nothing has been reported.
             */
            usage: components["schemas"]["TokenUsageDto"];
            /** @description The worktree, or the recorded working directory of a loose session. */
            worktree_path?: string | null;
        };
        /**
         * @description One row of `GET /v1/sessions`: a session of either kind.
         *
         *     An outside conversation carries no goal, task, seat or status, because
         *     Ariadne runs no work behind it; what it does carry is the agent it belongs
         *     to, the id that agent loads it back by, where it ran and when it last did.
         */
        SessionEntryDto: {
            /** @description The registry agent this conversation runs on (`GET /v1/acp-agents`). */
            agent_id: string;
            attention_reason?: null | components["schemas"]["AttentionReason"];
            attention_since?: string | null;
            /** Format: int64 */
            context_size?: number | null;
            /** Format: int64 */
            context_used?: number | null;
            /** @description When Ariadne created the row; None on an outside row. */
            created_at?: string | null;
            /**
             * @description Effort that model was launched at, off the same pin as `model`.
             * @example high
             */
            effort?: string | null;
            ended_at?: string | null;
            goal_id?: string | null;
            /** @description Ariadne's session id, or the agent's own session id on an outside row. */
            id: string;
            /** @description The ACP agent's own session id, which is `id` on an outside row. */
            internal_session_id?: string | null;
            kind: components["schemas"]["SessionKind"];
            /** @description When this session was last active, which the page is ordered by. */
            last_activity_at?: string | null;
            /** @description Model requested at launch, `<agent>:<model>`; None on an outside row. */
            model?: string | null;
            /** @description The pull request this session watches; None for every other session. */
            pull_request_id?: string | null;
            seat?: null | components["schemas"]["Seat"];
            status?: null | components["schemas"]["SessionStatus"];
            /**
             * @description The staffed agent this session runs; None for an orchestrator, a loose
             *     session or an outside one.
             */
            task_agent_id?: string | null;
            task_id?: string | null;
            /**
             * @description What this session is about: its task's title, the goal's title behind
             *     an orchestrator, or the first prompt of an outside conversation.
             */
            title?: string | null;
            usage?: null | components["schemas"]["TokenUsageDto"];
            /** @description The worktree, or the recorded working directory. */
            working_directory?: string | null;
        };
        /**
         * @description Which half of the listing a row came from: a session Ariadne runs, or a
         *     conversation an ACP agent stored that Ariadne did not start.
         * @enum {string}
         */
        SessionKind: "ariadne" | "outside";
        /** @description One page of `GET /v1/sessions`, newest activity first. */
        SessionPageDto: {
            /** @description The `cursor` that continues after this page; null on the last one. */
            next_cursor?: string | null;
            sessions: components["schemas"]["SessionEntryDto"][];
            /** @description When the outside snapshot this page was cut from was taken, RFC 3339. */
            snapshot_at: string;
            /** @description How many sessions the filters leave, over every page. */
            total: number;
        };
        /**
         * @description Agent session lifecycle status.
         * @enum {string}
         */
        SessionStatus: "starting" | "running" | "idle" | "exited" | "failed";
        /**
         * @description Body of `PUT /v1/models/enabled`: one model of the catalog, turned on or
         *     off.
         *
         *     The id is a field rather than a path segment because a model id carries
         *     `:` and often `/` (`opencode:anthropic/claude-sonnet-4`) — which is a
         *     path of its own, not a segment of one.
         */
        SetModelEnabledRequest: {
            /** @description What it becomes. */
            enabled: boolean;
            /**
             * @description The entry, as `GET /v1/models` spells its `id`.
             * @example claude-acp:claude-opus-5
             */
            id: string;
        };
        /** @description Body of `PUT /v1/models/rank`. The id can contain colons and slashes. */
        SetModelRankRequest: {
            /** @description The entry, as `GET /v1/models` spells its `id`. */
            id: string;
            rank?: null | components["schemas"]["ModelRank"];
        };
        /** @description `PUT /v1/forge/tunnel`: turn the tunnel on or off. */
        SetTunnelRequest: {
            enabled: boolean;
        };
        SkillDto: {
            /**
             * @description Whether Ariadne ships this skill. A built-in is reset rather than
             *     deleted; a skill of the user's own is deleted rather than reset.
             */
            builtin: boolean;
            created_at: string;
            /**
             * @description The whole `SKILL.md`: YAML frontmatter naming the skill and describing
             *     it, then the body. This is the text set on the skill, or the one
             *     Ariadne ships while a built-in has none of its own.
             */
            document: string;
            /** @description Whether `document` is the shipped text rather than one somebody wrote. */
            document_is_default: boolean;
            /**
             * @description Kebab-case; how an agent loads the skill and how a task names it.
             * @example code-review
             */
            name: string;
            /**
             * @description The seat this skill serves. An `orchestrator` skill cannot staff a
             *     task agent.
             */
            seat: components["schemas"]["SkillSeat"];
            /**
             * @description The one line the skill says about itself, read off the `description`
             *     of its frontmatter. It is what an agent sees before it opens the
             *     document, and what a listing shows.
             */
            summary: string;
            updated_at: string;
        };
        /**
         * @description Where a skill is used: by the orchestrator, or to staff a task agent.
         * @enum {string}
         */
        SkillSeat: "orchestrator" | "task" | "pull_request";
        /** @description What was spent in one bucket of the time axis. */
        SpendBucketDto: {
            /** Format: int64 */
            cached_input_tokens: number;
            /** Format: int64 */
            input_tokens: number;
            /** Format: int64 */
            output_tokens: number;
            /** @description The RFC 3339 start of the bucket. */
            start: string;
        };
        /**
         * @description Response of `GET /v1/stats/spend`: what did it spend? Tokens over time, by
         *     model, and per finished task — never a cost.
         */
        SpendStatsDto: {
            /** @default week */
            bucket: components["schemas"]["BucketDto"];
            /**
             * @description One row per bucket from the first fact to the last, zeros included.
             * @default []
             */
            buckets: components["schemas"]["SpendBucketDto"][];
            /**
             * @description One row per model, every seat pooled, heaviest first.
             * @default []
             */
            by_model: components["schemas"]["ModelSpendDto"][];
            /**
             * @default {
             *       "tasks": 0,
             *       "input_tokens": 0,
             *       "output_tokens": 0
             *     }
             */
            per_finished_task: components["schemas"]["PerFinishedTaskDto"];
            /**
             * @default {
             *       "sessions": 0,
             *       "input_tokens": 0,
             *       "cached_input_tokens": 0,
             *       "output_tokens": 0,
             *       "cached_share": 0
             *     }
             */
            totals: components["schemas"]["SpendTotalsDto"];
        };
        /** @description Tokens spent across every ended session the filter keeps. */
        SpendTotalsDto: {
            /** Format: int64 */
            cached_input_tokens: number;
            /**
             * Format: double
             * @description `cached_input_tokens` over `input_tokens`; 0 where `input_tokens` is 0.
             */
            cached_share: number;
            /** Format: int64 */
            input_tokens: number;
            /** Format: int64 */
            output_tokens: number;
            /** Format: int64 */
            sessions: number;
        };
        StatusTimeDto: {
            /** Format: double */
            median_secs: number;
            /** Format: double */
            share: number;
            status: string;
            /** Format: double */
            total_secs: number;
        };
        /**
         * @description What a step waits for before the next one may start: the author's commit,
         *     a push, a merge onto the base, or a request merged by a human.
         * @enum {string}
         */
        StepGate: "committed" | "pushed" | "merged" | "request_merged";
        /**
         * @description Body of `POST /v1/pull-requests/{id}/reviews` (029): one review, posted
         *     in the name of the integration login.
         */
        SubmitReviewRequest: {
            /**
             * @description The review's summary as it stands now: the daemon writes it into the
             *     one summary comment the review keeps on the request.
             */
            body: string;
            comments?: components["schemas"]["ReviewCommentRequest"][];
            /** @description `request_changes` or `comment`. Every other event is refused. */
            event: string;
        };
        /**
         * @description Switch a session to another model or agent: the old session ends, and a
         *     new one starts on the same seat, on this pin, in a new conversation.
         */
        SwitchSessionRequest: {
            /** @description The effort to run that model at; omitted = the agent's own. */
            effort?: string | null;
            /**
             * @description The model to run, `<agent>:<model>`: a registry agent (`GET
             *     /v1/acp-agents`) and a model of it (`GET /v1/models`).
             * @example claude-acp:sonnet
             */
            model: string;
        };
        /**
         * @description One agent staffed on a task: the column it works, what it knows, and what
         *     it runs on.
         *
         *     The agent has no identity of its own. `step` says only which column of
         *     the workflow it works; the skills are what it can do. What it runs on was
         *     sized by the orchestrator when it staffed the task, or chosen by the user
         *     since — either way it is what this agent runs on, and nothing behind it
         *     changes that.
         */
        TaskAgentDto: {
            /**
             * @description What the orchestrator told this agent beyond the task itself. None =
             *     the task is the whole of it.
             */
            brief?: string | null;
            /**
             * @description The reasoning effort that model is run at. None = whatever the agent
             *     runs it at on its own.
             * @example high
             */
            effort?: string | null;
            id: string;
            /**
             * @description What this agent runs on, `<agent>:<model>`.
             * @example codex-acp:o3
             */
            model: string;
            /**
             * @description This agent's live session, the id `POST /v1/sessions/{id}/switch`
             *     takes. None while it carries no live session.
             */
            session_id?: string | null;
            /**
             * @description The skills this agent loads, in the order they reach it.
             * @example [
             *       "coding",
             *       "documentation"
             *     ]
             */
            skills: string[];
            /** @description The id of the workflow column this agent works. */
            step: string;
        };
        /**
         * @description Payload of `task_branch_updated`: where a task's branch points now.
         *
         *     A commit in the task's worktree changes nothing in the store, so no
         *     other event says the task's diff is no longer the one a client fetched.
         */
        TaskBranchDto: {
            /** @description The task branch whose head moved. */
            branch: string;
            goal_id: string;
            /** @description Full sha of the commit the branch points at now. */
            head: string;
            task_id: string;
        };
        TaskDto: {
            /**
             * @description The agents staffed on the task, one per column of its goal's
             *     workflow, in the order the orchestrator listed them. What each one can
             *     do is the skills it carries.
             */
            agents: components["schemas"]["TaskAgentDto"][];
            /** @description The branch every column works on, in the one worktree the task has. */
            branch: string;
            created_at: string;
            /** @description Ids of tasks that must finish before this one starts. */
            depends_on: string[];
            description: string;
            goal_id: string;
            id: string;
            /** @description The commit the task landed as, where its last column reported one. */
            merge_commit?: string | null;
            /**
             * @description URL of the pull or merge request the task's `pr` column opened, once
             *     it has; null for a task landed directly.
             */
            pr_url?: string | null;
            /**
             * @description Why a `failed` or `cancelled` task ended — an agent's own `fail_task`
             *     reason, a dependency that never landed, a cancelled goal. Null for
             *     every other status, and for an ending nobody gave a reason for.
             */
            reason?: string | null;
            /** @description Id of the repository the task works in, one of its goal's. */
            repo_id: string;
            /** @description Set when the agent went idle without advancing the task. */
            stalled: boolean;
            status: components["schemas"]["TaskStatus"];
            /**
             * @description The column of its goal's workflow the task is in while it is
             *     `in_progress`; null before its first column and once it has ended.
             */
            step?: string | null;
            title: string;
            updated_at: string;
            /** @description What the agents of this task have spent between them. */
            usage: components["schemas"]["TaskUsageDto"];
            worktree_path?: string | null;
        };
        /**
         * @description Task lifecycle status.
         * @enum {string}
         */
        TaskStatus: "pending" | "ready" | "in_progress" | "finished" | "cancelled" | "failed";
        TaskTransitionDto: {
            actor: string;
            created_at: string;
            from_status: string;
            /** @description The column the task left, where the move crossed columns. */
            from_step?: string | null;
            id: string;
            reason?: string | null;
            to_status: string;
            /** @description The column the task entered. */
            to_step?: string | null;
        };
        /**
         * @description Payload of `task_updated`: the task as it now stands, plus the audit row
         *     when the update was a status transition.
         */
        TaskUpdatedDto: {
            task: components["schemas"]["TaskDto"];
            transition?: null | components["schemas"]["TaskTransitionDto"];
        };
        /**
         * @description What a task cost, by who spent it: one entry per agent that has a session
         *     on the task, and the total of every session on it.
         */
        TaskUsageDto: {
            /**
             * @description One entry per agent that has a session on the task, every run of it
             *     summed, in column order. An agent whose session has yet to report
             *     anything is listed with zeros; one that has never been spawned is not
             *     listed at all.
             */
            agents: components["schemas"]["AgentUsageDto"][];
            /** @description Every session on the task summed, whatever its column. */
            total: components["schemas"]["TokenUsageDto"];
        };
        /**
         * @description One permission request to score with the AI permission model, without
         *     selecting an ACP option or recording an approval.
         */
        TestAiPermissionRequest: {
            /** @description The raw JSON input the model sees in compact form. */
            input: unknown;
            /** @description The tool call kind the model sees. */
            kind?: string | null;
            /** @description The paths the tool call touches, as an agent sends them in `locations`. */
            locations?: string[] | null;
            /** @description The option names the model sees. */
            options?: string[] | null;
            /**
             * @description The tool call title the model sees, such as `git status` or
             *     `Read /etc/hosts`, not the tool name.
             */
            tool: string;
            /** @description The workspace used to derive whether a path is outside it. */
            workspace?: string | null;
        };
        /**
         * @description The AI permission model's score for a request, held to the current
         *     thresholds.
         */
        TestAiPermissionResponse: {
            /** @description Why the model did not answer. */
            ai_error?: string | null;
            /**
             * Format: double
             * @description Danger at or below this value is allowed.
             */
            allow_threshold: number;
            /** @description The first cap that changed an allow into an ask. */
            cap?: string | null;
            /**
             * Format: double
             * @description The model's normalized danger score when it answered.
             */
            danger?: number | null;
            /**
             * Format: double
             * @description Danger at or above this value is denied.
             */
            deny_threshold: number;
            /** @description `allow`, `ask`, or `deny` when the model answered. */
            label?: string | null;
            /** @description The derived operation hint, when one is known. */
            operation?: string | null;
            /** @description Kev's probabilities for the decision question. */
            probabilities?: unknown;
            /** @description The ordered risk tags derived from the complete request. */
            risk_tags?: string[] | null;
        };
        /** @description Response of `GET /v1/stats/time`: how long does finished work take? */
        TimeStatsDto: {
            in_status: components["schemas"]["StatusTimeDto"][];
            lead_time: components["schemas"]["LeadTimeDto"];
            /** Format: int64 */
            tasks: number;
            waiting_on_person: components["schemas"]["PersonWaitDto"];
        };
        /**
         * @description Tokens spent, as the agents' own transcripts report them.
         *
         *     Always present and always a number: nothing reported is zero, not null.
         */
        TokenUsageDto: {
            /**
             * Format: int64
             * @description The subset of `input_tokens` served from the prompt cache, so never
             *     added to it.
             */
            cached_input_tokens: number;
            /**
             * Format: int64
             * @description Prompt tokens, cache reads and cache writes included.
             */
            input_tokens: number;
            /**
             * Format: int64
             * @description Completion tokens, thinking and reasoning included.
             */
            output_tokens: number;
        };
        TransitionRequest: {
            /**
             * @description The commit the task landed as, where `to` is `finished` and a column
             *     reported one.
             */
            merge_commit?: string | null;
            reason?: string | null;
            to: components["schemas"]["TaskStatus"];
        };
        /**
         * @description Where the webhook tunnel stands (027).
         * @enum {string}
         */
        TunnelState: "up" | "down" | "off";
        /** @description Body of `PUT /v1/agents/{id}`: the whole new flag list, empty included. */
        UpdateAgentConfigRequest: {
            extra_flags: string[];
        };
        /** @description Partial update of the AI permission settings; an absent field stays unchanged. */
        UpdateAiPermissionsRequest: {
            /**
             * Format: double
             * @description Danger at or below this value is allowed. Values outside 0 to 1 are refused.
             */
            allow_threshold?: number | null;
            /**
             * @description `true` drops a pair set by hand and goes back to the default pair of
             *     the chosen flavour. Refused alongside either threshold.
             */
            default_thresholds?: boolean;
            /**
             * Format: double
             * @description Danger at or above this value is denied. Values outside 0 to 1 are refused.
             */
            deny_threshold?: number | null;
            device?: null | components["schemas"]["Device"];
            /** @description Turning it on starts an install; turning it off keeps the files. */
            enabled?: boolean | null;
            flavour?: null | components["schemas"]["Flavour"];
        };
        /** @description Widen a row to every repository, or narrow it back to its own. */
        UpdateLearnedPermissionRequest: {
            scope: components["schemas"]["LearnedPermissionScope"];
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateRepositoryRequest: {
            base_branch?: string | null;
            /** @description Absent = unchanged. */
            default_workflow?: string | null;
            /** @description New description, or empty to clear it. Absent = unchanged. */
            description?: string | null;
            forge?: null | components["schemas"]["ForgeUpdate"];
            path?: string | null;
            permission_mode?: null | components["schemas"]["PermissionMode"];
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateSkillRequest: {
            /**
             * @description The new document. Absent = unchanged; putting a built-in back on the
             *     text Ariadne ships is `POST /v1/skills/{name}/document/reset`.
             */
            document?: string | null;
        };
        /** @description Partial update; only allowed while the task is pending, ready or failed. */
        UpdateTaskRequest: {
            /**
             * @description The whole staffing, replaced: every column is staffed afresh, with
             *     the skills and the model it names. The way to staff a column a task
             *     lacks before it is retried.
             */
            agents?: components["schemas"]["AgentAssignment"][] | null;
            /** @description The whole dependency list, replaced. */
            depends_on?: string[] | null;
            description?: string | null;
            title?: string | null;
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateWorkflowRequest: {
            /**
             * @description The new document. Absent = unchanged; putting a built-in back on the
             *     text Ariadne ships is `POST /v1/workflows/{name}/reset`.
             */
            document?: string | null;
        };
        /** @description Response of `GET /v1/version`. */
        VersionResponse: {
            /** @example ariadned */
            name: string;
            /** @example 0.1.0 */
            version: string;
        };
        /** @description Public hook status. The hook secret is never serialized. */
        WebhookDto: {
            error?: string | null;
            /**
             * @description Why the last fetch of the repository's requests failed, or null once
             *     one works: what says polling works, where no hook is live.
             */
            fetch_error?: string | null;
            last_delivery_at?: string | null;
            state: string;
            url?: string | null;
        };
        /** @description One bucket of the time axis: the counts of the facts that fall in it. */
        WorkBucketDto: {
            /**
             * Format: int64
             * @default 0
             */
            goals_completed: number;
            /**
             * Format: int64
             * @default 0
             */
            landed: number;
            /**
             * @description The RFC 3339 start of the bucket.
             * @default
             */
            start: string;
            /**
             * Format: int64
             * @default 0
             */
            tasks_cancelled: number;
            /**
             * Format: int64
             * @default 0
             */
            tasks_failed: number;
            /**
             * Format: int64
             * @default 0
             */
            tasks_finished: number;
        };
        /** @description Response of `GET /v1/stats/work`: what got done? */
        WorkStatsDto: {
            /**
             * @description `"hour"`, `"day"` or `"week"`: the bucket every
             *     [`WorkBucketDto::start`] falls on.
             * @default
             */
            bucket: string;
            /**
             * @description One row per bucket from the first fact to the last, zeros included.
             * @default []
             */
            buckets: components["schemas"]["WorkBucketDto"][];
            /**
             * @default {
             *       "goals_completed": 0,
             *       "goals_cancelled": 0,
             *       "median_goal_lead_time_secs": 0,
             *       "tasks_finished": 0,
             *       "tasks_failed": 0,
             *       "tasks_cancelled": 0,
             *       "finish_rate": 0,
             *       "landed": 0
             *     }
             */
            totals: components["schemas"]["WorkTotalsDto"];
        };
        /** @description The counts `WorkStatsDto` answers over the whole span a filter keeps. */
        WorkTotalsDto: {
            /**
             * Format: double
             * @description `tasks_finished` over the three endings; 0 where there are none.
             * @default 0
             */
            finish_rate: number;
            /**
             * Format: int64
             * @default 0
             */
            goals_cancelled: number;
            /**
             * Format: int64
             * @default 0
             */
            goals_completed: number;
            /**
             * Format: int64
             * @description Finished tasks whose `landing` was `merge` or `pull_request`.
             * @default 0
             */
            landed: number;
            /**
             * Format: double
             * @default 0
             */
            median_goal_lead_time_secs: number;
            /**
             * Format: int64
             * @default 0
             */
            tasks_cancelled: number;
            /**
             * Format: int64
             * @default 0
             */
            tasks_failed: number;
            /**
             * Format: int64
             * @default 0
             */
            tasks_finished: number;
        };
        WorkflowDto: {
            /**
             * @description Whether Ariadne ships this workflow. A built-in is reset rather than
             *     deleted; a workflow of the user's own is deleted rather than reset.
             */
            builtin: boolean;
            created_at: string;
            /** @description The effective document: the override, or the text Ariadne ships. */
            document: string;
            /**
             * @description Kebab-case; named on the document's first line.
             * @example develop-review-merge
             */
            name: string;
            /** @description `document`, parsed into its columns. */
            steps: components["schemas"]["WorkflowStepDto"][];
            updated_at: string;
        };
        /** @description One column of a workflow. */
        WorkflowStepDto: {
            description: string;
            gate?: null | components["schemas"]["StepGate"];
            /**
             * @description Kebab-case, unique within the workflow.
             * @example develop
             */
            id: string;
            rank?: null | components["schemas"]["ModelRank"];
            /** @description The skills an agent on this step loads. */
            skills: string[];
            /**
             * @description The display title.
             * @example Develop
             */
            title: string;
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    "acp-agents_list": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AcpAgentDto"][];
                };
            };
        };
    };
    "acp-agents_refresh": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AcpAgentDto"][];
                };
            };
        };
    };
    agents_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentConfigDto"][];
                };
            };
        };
    };
    agents_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description the agent's registry id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateAgentConfigRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentConfigDto"];
                };
            };
            /** @description no such agent in the registry */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    attention_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AttentionListDto"];
                };
            };
        };
    };
    system_report: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The daemon's own environment */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DaemonReportDto"];
                };
            };
        };
    };
    events_list: {
        parameters: {
            query?: {
                /** @description Filter by session id. */
                session?: string | null;
                /** @description Filter by task id. */
                task?: string | null;
                /**
                 * @description Filter by goal id: every event whose session, or whose task, belongs
                 *     to that goal.
                 */
                goal?: string | null;
                /**
                 * @description Return events with an id less than this one, which is how a descending
                 *     page walks further back.
                 */
                before?: string | null;
                /**
                 * @description Which end of the recorded events the page is taken from (default
                 *     `asc`).
                 */
                order?: null | components["schemas"]["EventOrder"];
                /** @description Return items with id greater than this. */
                after?: string | null;
                /** @description Max items to return (default 50, cap 200). */
                limit?: number | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentEventDto"][];
                };
            };
        };
    };
    events_stream: {
        parameters: {
            query?: {
                /** @description Only events belonging to this goal. */
                goal?: string | null;
                /** @description Only events belonging to this task. */
                task?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description SSE stream of domain events (text/event-stream). No replay on reconnect: refetch REST state first. A `heartbeat` event (HeartbeatDto) opens the connection and repeats every 15 idle seconds. A lagging client gets a final `resync` event (ResyncDto) and the connection is closed. `task_branch_updated` (TaskBranchDto) comes from the daemon's watch on the task branch rather than from a store write: it says a commit landed and the task's diff has moved. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": components["schemas"]["DomainEvent"];
                };
            };
        };
    };
    repositories_get_tunnel: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ForgeTunnelDto"];
                };
            };
        };
    };
    repositories_set_tunnel: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SetTunnelRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ForgeTunnelDto"];
                };
            };
        };
    };
    goals_list: {
        parameters: {
            query?: {
                /**
                 * @description Filter by status: one status, or several comma-separated
                 *     (`status=active,completed`), matching goals in any of them.
                 * @example active,completed
                 */
                status?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"][];
                };
            };
        };
    };
    goals_create: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateGoalRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description no such repository or workflow */
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_create: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                goal_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateTaskRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the goal is not finished yet; cancel it first */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_cancel: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_complete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CompleteGoalRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_finalize: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["FinalizePlanRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["GoalDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    goals_list_goal_messages: {
        parameters: {
            query?: {
                /** @description Only the messages for this staffed agent. */
                to_agent_id?: string | null;
                /** @description Only the ones that have not reached their agent yet. */
                undelivered?: boolean;
                /**
                 * @description Hand the caller its own undelivered messages: narrow the list to the
                 *     calling session's agent, and stamp every row it returns delivered.
                 *
                 *     This is a delivery, not a read. It needs a session behind it, and it
                 *     is what makes a read of the channel count the same as a prompt: a
                 *     message handed over here is never handed over again.
                 */
                deliver?: boolean;
            };
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MessageDto"][];
                };
            };
        };
    };
    goals_post_goal_message: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description goal id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SendMessageRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MessageDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    system_health: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Daemon is healthy */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["HealthResponse"];
                };
            };
        };
    };
    logs_snapshot: {
        parameters: {
            query?: {
                /** @description Return only the last N lines. */
                tail?: number | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LogSnapshotResponse"];
                };
            };
        };
    };
    logs_stream: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description SSE stream of daemon log lines (text/event-stream). A `snapshot` event with the current buffer (LogSnapshotResponse), then a `delta` event per new line (LogLineDto). A follower that falls too far behind is disconnected; reconnecting starts over from a fresh snapshot. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": components["schemas"]["LogSnapshotResponse"];
                };
            };
        };
    };
    models_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModelDto"][];
                };
            };
        };
    };
    models_set_enabled: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SetModelEnabledRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModelDto"];
                };
            };
            /** @description no such model in the catalog */
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description it is the last model left enabled */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    models_set_rank: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SetModelRankRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModelDto"];
                };
            };
            /** @description no such model in the catalog */
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description rank must be frontier, balanced, fast or local */
            422: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_resume_outside: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ResumeOutsideSessionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_get: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AiPermissionsStatusDto"];
                };
            };
        };
    };
    permissions_update: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateAiPermissionsRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AiPermissionsStatusDto"];
                };
            };
            /** @description no Python 3.12 or 3.13 to install into */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description invalid thresholds, or a flavour and device combination that cannot run */
            422: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_refresh: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            202: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AiPermissionsStatusDto"];
                };
            };
            /** @description the AI permission model is off, or an install is already running */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_test: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TestAiPermissionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TestAiPermissionResponse"];
                };
            };
            /** @description the AI permission model is off */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the tool is empty */
            422: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_list_learned: {
        parameters: {
            query?: {
                repository?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LearnedPermissionsResponse"];
                };
            };
        };
    };
    permissions_get_learned: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LearnedPermissionDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_update_learned: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateLearnedPermissionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LearnedPermissionDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            422: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    permissions_delete_learned: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_list": {
        parameters: {
            query?: {
                repo?: string | null;
                /** @description `reviewer` for the requests of others, `author` for the user's own. */
                role?: string | null;
                /** @description The task that opened the request. */
                task?: string | null;
                /**
                 * @description True for the requests that ask for the user's review (029), false
                 *     for the ones that do not.
                 */
                requested?: boolean | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestDto"][];
                };
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_refresh": {
        parameters: {
            query?: {
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            202: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_get": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_comments": {
        parameters: {
            query?: {
                /** @description Only the threads that wait on an answer from the integration login. */
                unanswered_only?: boolean | null;
            };
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestCommentDto"][];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_comment": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                comment_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestCommentDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_reply": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                comment_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ReplyCommentRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestCommentDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_resolve": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                comment_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestCommentDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_diff": {
        parameters: {
            query?: {
                /** @description Read the diff from this sha to the head, not from the base. */
                since?: string | null;
            };
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/plain": string;
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_report": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ReportPullRequestRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_submit_review": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SubmitReviewRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestCommentDto"][];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    repositories_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RepositoryDto"][];
                };
            };
        };
    };
    repositories_create: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateRepositoryRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RepositoryDto"];
                };
            };
            /** @description not an absolute path, not a git work tree, an unknown branch, or a forge pin that is no catalog model */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description this path and base branch are already registered, `ai` was asked for while the AI permission model is off, or the forge integration cannot be enabled */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    repositories_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RepositoryDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    repositories_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateRepositoryRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RepositoryDto"];
                };
            };
            /** @description not an absolute path, not a git work tree, an unknown branch, or a forge pin that is no catalog model */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description this path and base branch are already registered, `ai` was asked for while the AI permission model is off, or the forge integration cannot be enabled */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    repositories_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    issues_list: {
        parameters: {
            query?: {
                /** @description `me` filters to the forge login; `all` reads all open issues. */
                assigned?: string | null;
            };
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["IssueDto"][];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    issues_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                number: number;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["IssueDto"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_search": {
        parameters: {
            query?: {
                q?: string;
            };
            header?: never;
            path: {
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestMatchDto"][];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_get_by_number": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                number: number;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestDto"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    "pull-requests_ask_review": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                id: string;
                number: number;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AskReviewRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PullRequestDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            502: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_list: {
        parameters: {
            query?: {
                /** @description Only sessions of this kind. Omitted lists both. */
                kind?: null | components["schemas"]["SessionKind"];
                /** @description Only sessions of this registry agent (`GET /v1/acp-agents`). */
                agent?: string | null;
                /** @description Filter by goal id. No outside session has one. */
                goal?: string | null;
                /** @description Filter by task id. No outside session has one. */
                task?: string | null;
                /** @description Filter by pull request id. No outside session has one. */
                pull_request?: string | null;
                /**
                 * @description Filter by status. No outside session has one. A named status also
                 *     lists sessions that have ended, as `all` does.
                 */
                status?: null | components["schemas"]["SessionStatus"];
                /** @description Filter by seat. No outside session has one. */
                seat?: null | components["schemas"]["Seat"];
                /**
                 * @description Only sessions currently flagged as needing attention. No outside
                 *     session is.
                 */
                attention?: boolean | null;
                /**
                 * @description Only sessions whose directory is this absolute path, or a path under
                 *     it.
                 */
                dir?: string | null;
                /** @description Only sessions last active at or after this moment, RFC 3339. */
                since?: string | null;
                /** @description Only sessions last active at or before this moment, RFC 3339. */
                until?: string | null;
                /** @description Only sessions whose title contains this text, case-insensitive. */
                q?: string | null;
                /**
                 * @description List every session, whatever its age and whether it has ended, rather
                 *     than the live ones of the last 7 days.
                 */
                all?: boolean | null;
                /** @description Max sessions in the page (default 50, cap 200). */
                limit?: number | null;
                /** @description The `next_cursor` of the page before this one; opaque. */
                cursor?: string | null;
                /** @description Ask every agent again before answering, whatever the snapshot's age. */
                refresh?: boolean | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionPageDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_create_session: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewSessionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_snapshot: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentEventDto"][];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the console streamed faster than a snapshot could be read, every time it was tried; ask again */
            503: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_console_cancel: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The running turn was told to cancel */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_console_input: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ConsoleInputRequest"];
            };
        };
        responses: {
            /** @description Permission answer or prompt accepted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_stream: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description SSE stream of console events (text/event-stream). A `snapshot` event carrying the newest page of recorded events (`[AgentEventDto]`) and the running turn's text so far, then an `event` per new one (`AgentEventDto`) — message and thought chunks, tool call progress, tool calls, plans, permission requests and turn status all arrive this way, in the vocabulary the ACP runtime reports them in. The chunks and the progress are live only: they are never stored and never reach `/v1/events`. A client that falls behind gets a `resync` event (ResyncDto) and the connection closes. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": components["schemas"]["AgentEventDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_console_terminal: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description WebSocket. Client to server: JSON text frames, one `TerminalClientMessage` each — `resize` first, then `key` and `paste`. Server to client: binary frames of the terminal bytes that draw the console, and JSON text frames of `TerminalServerMessage`. */
            101: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_kill: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_resume: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    sessions_switch_session: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description session id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SwitchSessionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionDto"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    skills_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SkillDto"][];
                };
            };
        };
    };
    skills_create: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateSkillRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SkillDto"];
                };
            };
            /** @description name already exists */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    skills_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description skill name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SkillDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    skills_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description skill name */
                name: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateSkillRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SkillDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    skills_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description skill name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    skills_reset_document: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description skill name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SkillDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    stats_attention: {
        parameters: {
            query?: {
                /**
                 * @description Only facts written since then: an RFC 3339 moment, or a span back
                 *     from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
                 *     Absent is every fact there is.
                 */
                since?: string | null;
                /** @description Only facts about this repository id. */
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AttentionStatsDto"];
                };
            };
            /** @description `since` is neither a moment nor a span */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    stats_models: {
        parameters: {
            query?: {
                /**
                 * @description Only facts written since then: an RFC 3339 moment, or a span back
                 *     from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
                 *     Absent is every fact there is.
                 */
                since?: string | null;
                /** @description Only facts about this repository id. */
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModelStatsDto"];
                };
            };
            /** @description `since` is neither a moment nor a span */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    stats_spend: {
        parameters: {
            query?: {
                /**
                 * @description Only facts written since then: an RFC 3339 moment, or a span back
                 *     from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
                 *     Absent is every fact there is.
                 */
                since?: string | null;
                /** @description Only facts about this repository id. */
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SpendStatsDto"];
                };
            };
            /** @description `since` is neither a moment nor a span */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    stats_time: {
        parameters: {
            query?: {
                /**
                 * @description Only facts written since then: an RFC 3339 moment, or a span back
                 *     from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
                 *     Absent is every fact there is.
                 */
                since?: string | null;
                /** @description Only facts about this repository id. */
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TimeStatsDto"];
                };
            };
            /** @description `since` is neither a moment nor a span */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    stats_work: {
        parameters: {
            query?: {
                /**
                 * @description Only facts written since then: an RFC 3339 moment, or a span back
                 *     from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
                 *     Absent is every fact there is.
                 */
                since?: string | null;
                /** @description Only facts about this repository id. */
                repo?: string | null;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkStatsDto"];
                };
            };
            /** @description `since` is neither a moment nor a span */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_list: {
        parameters: {
            query?: {
                /** @description Filter by goal id. */
                goal?: string | null;
                /** @description Filter by status. */
                status?: null | components["schemas"]["TaskStatus"];
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"][];
                };
            };
        };
    };
    tasks_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateTaskRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_cancel: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_diff: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/plain": string;
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_list_task_messages: {
        parameters: {
            query?: {
                /** @description Only the messages for this staffed agent. */
                to_agent_id?: string | null;
                /** @description Only the ones that have not reached their agent yet. */
                undelivered?: boolean;
                /**
                 * @description Hand the caller its own undelivered messages: narrow the list to the
                 *     calling session's agent, and stamp every row it returns delivered.
                 *
                 *     This is a delivery, not a read. It needs a session behind it, and it
                 *     is what makes a read of the channel count the same as a prompt: a
                 *     message handed over here is never handed over again.
                 */
                deliver?: boolean;
            };
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MessageDto"][];
                };
            };
        };
    };
    tasks_post_task_message: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["SendMessageRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MessageDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_open_pull_request: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["OpenPullRequestRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            /** @description not the current column's agent */
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the task is not in progress, has no forge or its integration is off, is not authenticated, or is not pushed */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_retry: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_complete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CompleteStepRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description step_gate_failed */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_fail: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["FailStepRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_list_transitions: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskTransitionDto"][];
                };
            };
        };
    };
    tasks_transition: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description task id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TransitionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TaskDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    system_version: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Daemon version */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["VersionResponse"];
                };
            };
        };
    };
    workflows_list: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkflowDto"][];
                };
            };
        };
    };
    workflows_create: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateWorkflowRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkflowDto"];
                };
            };
            /** @description name already exists, or the document is invalid */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    workflows_parse: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ParseWorkflowRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ParsedWorkflowDto"];
                };
            };
            /** @description workflow_invalid, with details.line */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    workflows_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description workflow name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkflowDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    workflows_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description workflow name */
                name: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateWorkflowRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkflowDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    workflows_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description workflow name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    workflows_reset: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description workflow name */
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkflowDto"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
}

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
        /** Every built-in and configured ACP agent with its cached probe result. */
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
        /** List agent events (poll with `after` for tailing). */
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
         *     there moves this goal too.
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
         *     approve.
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
    "/v1/outside-sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List sessions Ariadne did not start: the stored sessions of every ACP
         *     agent that can list them, newest first.
         */
        get: operations["sessions_list_outside"];
        put?: never;
        post?: never;
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
    "/v1/repositories/{repository_id}/memories": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["memories_list"];
        put?: never;
        post: operations["memories_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{repository_id}/memories/search": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["memories_search"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/repositories/{repository_id}/memories/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete: operations["memories_delete"];
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
        /** List agent sessions. */
        get: operations["sessions_list"];
        put?: never;
        post?: never;
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
         * The session's events so far, in order: the whole transcript a console
         *     opens on.
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
         *     every event recorded so far, oldest first — then an `event` per later one,
         *     each an `AgentEventDto`. Subscribing happens before the snapshot is read
         *     and every later event is compared against the snapshot's last id, so
         *     nothing committed in between is ever missed or delivered twice.
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
         * @description `409` when there is nothing to come back to: no stored agent id, a
         *     worktree that was cleaned up — or a goal that has finished, whose live
         *     sessions the scheduler takes down anyway.
         */
        post: operations["sessions_resume"];
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
        /** Edit a pending/ready task (orchestrator or user). */
        patch: operations["tasks_update"];
        trace?: never;
    };
    "/v1/tasks/{id}/author-session": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Make a stored ACP session the author of a ready task. */
        post: operations["tasks_adopt_author_session"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
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
         *     refuses an author or a reviewer reaching for it.
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
         * @description On a task staffed with several authors, `agent` names the author whose
         *     branch to read; left out, the task's own branch is read — the first
         *     author's until the pick settles, and the winner's after it.
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
    "/v1/tasks/{id}/pick": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * One reviewer's pick of the winning author, on a task staffed with several.
         * @description The gate is here: the pick starts only once every author is approved, and
         *     a pick before that is refused. One pick per reviewer per task — a second
         *     is refused by the reviewer's name — and the daemon settles the winner once
         *     every staffed reviewer has picked.
         */
        post: operations["tasks_pick_winner"];
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
         * Record the pull or merge request the author opened for a task.
         * @description The URL travels as a tool call, so a published task is either one the UI
         *     and the CLI can point at or one that was never reported.
         *
         *     And this is the moment the task becomes the user's: a request nobody can
         *     merge but a human is exactly what `waiting_user` says, so it goes up here,
         *     on the session that opened it — the console they answer in, and the one place
         *     the request can be traced back to. It used to be raised by the message the
         *     landing briefing told the author to write, and a published task with
         *     nothing on the strip is one nobody knows to go and merge.
         *
         *     It stays up until the user acts: an agent's own events never take
         *     `waiting_user` down (`clear_agent_attention`), and the author polling its
         *     request is exactly such an agent. `Scheduler::keep_waiting_user` puts it
         *     back on whatever comes up when that author is restarted, which is what
         *     makes the two halves one flag rather than two.
         */
        post: operations["tasks_record_pull_request"];
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
         * Retry a failed task: failed -> ready. The user's call, and the
         *     orchestrator's — the daemon wakes it when a task fails, and retrying is
         *     one of the three answers it has.
         */
        post: operations["tasks_retry"];
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
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** @description One ACP agent known to the daemon and its latest discovery result. */
        AcpAgentDto: {
            /** @description Whether Ariadne supplied this entry. */
            builtin: boolean;
            capabilities: components["schemas"]["AcpCapabilitiesDto"];
            /** @description Program followed by its arguments. */
            command: string[];
            /** @description One flag for every optional capability that is absent. */
            degraded: components["schemas"]["AcpDegradation"][];
            /** @description Stable id used as the model-id prefix. */
            id: string;
            /** @description Why discovery rejected this agent. */
            rejection_reason?: string | null;
            status: components["schemas"]["AcpAgentStatus"];
        };
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
        Actor: "orchestrator" | "author" | "reviewer" | "daemon" | "user";
        /** @description The stored ACP session to adopt as a task author. */
        AdoptOutsideSessionRequest: {
            /** @description Which registry agent the session belongs to. */
            agent_id: string;
            internal_session_id: string;
        };
        /**
         * @description One agent to staff on a task: where it sits, the skills it loads, and what
         *     it is to run on.
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
             * @description `author` or `reviewer`. A task takes one author or more; several
             *     authors need at least one reviewer, to pick the winner.
             */
            seat: components["schemas"]["Seat"];
            /**
             * @description The names of the skills this agent loads, in the order they reach it.
             *     A name no skill answers to is refused.
             * @example [
             *       "coding",
             *       "testing"
             *     ]
             */
            skills?: string[];
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
         * @description What one staffed agent spent on a task, named the way a reader addresses
         *     it: an agent has no name of its own, so its skills are what identify it.
         */
        AgentUsageDto: {
            agent_id: string;
            /** @description The skills the agent loads; empty only if the agent is gone. */
            skills: string[];
            usage: components["schemas"]["TokenUsageDto"];
        };
        /**
         * @description Why a live agent session needs the user's attention.
         *
         *     Orthogonal to [`SessionStatus`]: a session waiting on a permission prompt
         *     is still `running` as far as its lifecycle goes, it just cannot make
         *     progress until someone looks at it.
         * @enum {string}
         */
        AttentionReason: "waiting_permission" | "waiting_input" | "waiting_user" | "agent_error" | "disconnected" | "stalled";
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
         * @description Body of `POST /v1/goals/{id}/complete`: the orchestrator says the goal is
         *     done. Its call, not the user's, and it carries nothing — every task being
         *     finished or cancelled is the whole of the argument, and the daemon checks
         *     that itself.
         */
        CompleteGoalRequest: Record<string, never>;
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
            /**
             * @description What the orchestrator runs on, `<agent>:<model>` — the id of an agent
             *     in the ACP registry and, after the `:`, the model of it:
             *     `codex-acp:gpt-5.3-codex`, `opencode-acp:ollama/llama3:8b`. Required —
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
        };
        CreateMemoryRequest: {
            /** @description The RFC 3339 time after which this entry stays hidden. */
            expires_at: string;
            text: string;
        };
        CreateRepositoryRequest: {
            /** @description Omit for the repo's currently checked-out branch. */
            base_branch?: string | null;
            description?: string | null;
            /**
             * @description Absolute path of an existing git work tree.
             * @example /home/me/dev/ariadne
             */
            path: string;
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
             * @description The agents to staff: the authors first — one or more, each with its
             *     own model — then the reviewers in review order. Several authors need
             *     at least one reviewer, to pick the winner.
             */
            agents: components["schemas"]["AgentAssignment"][];
            /** @description Task ids this task depends on. */
            depends_on?: string[];
            description?: string;
            landing?: null | components["schemas"]["Landing"];
            permission_mode?: null | components["schemas"]["PermissionMode"];
            /**
             * @description Id of one of the goal's repositories; may be omitted when the goal
             *     works in exactly one.
             */
            repo_id?: string | null;
            title: string;
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
            /** @description A raw agent event the ACP runtime recorded. */
            data: components["schemas"]["AgentEventDto"];
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
            data: components["schemas"]["MemoryDto"];
            /** @enum {string} */
            event: "memory_created";
        } | {
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "memory_deleted";
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
         * @description Body of `POST /v1/goals/{id}/finalize`: the orchestrator ends planning and
         *     execution starts. The orchestrator's call, not the user's, and it carries
         *     nothing — the plan is the tasks it wrote.
         */
        FinalizePlanRequest: Record<string, never>;
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
            /**
             * @description What the orchestrator runs on, `<agent>:<model>`: the registry agent
             *     and, after the `:`, the model of it (`claude-agent-acp:claude-opus-5`).
             * @example claude-agent-acp:claude-opus-5
             */
            model: string;
            /**
             * @description The registered repositories the goal works in, as they stand now: a
             *     goal references them, so an edit to one shows up here.
             */
            repos: components["schemas"]["RepositoryDto"][];
            status: components["schemas"]["GoalStatus"];
            title: string;
            updated_at: string;
            /** @description What the agents of this goal have spent between them. */
            usage: components["schemas"]["GoalUsageDto"];
        };
        /**
         * @description Goal lifecycle status.
         * @enum {string}
         */
        GoalStatus: "planning" | "active" | "completed" | "cancelled";
        /**
         * @description What a goal cost, by the seat that spent it. Grouped by seat rather than
         *     by agent: a goal's authors are as many as it has tasks, and what is
         *     read at this height is where the tokens went, not which agent went there.
         */
        GoalUsageDto: {
            /** @description Every author session of every task of the goal. */
            authors: components["schemas"]["TokenUsageDto"];
            /** @description The orchestrator's sessions, which belong to no task. */
            orchestrator: components["schemas"]["TokenUsageDto"];
            /** @description Every reviewer session of every task of the goal, all rounds. */
            reviewers: components["schemas"]["TokenUsageDto"];
            /** @description Every session of the goal summed, the orchestrator's included. */
            total: components["schemas"]["TokenUsageDto"];
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
        /**
         * @description How one task ends.
         *
         *     The one thing about the end of a task the author has to be told, since the
         *     commands it runs differ entirely between the three. The orchestrator agrees
         *     it with the user task by task: some work lands on the base branch, some
         *     goes through a request the author then sees to its merge, and some has
         *     nothing to land at all — a report filed, a document published, a release
         *     cut. All three reach [`TaskStatus::Finished`]; landing is one way of
         *     getting there rather than the meaning of being there.
         *
         *     Which forge a published request goes to is *not* here: `origin` says
         *     whether it is GitHub or GitLab, and asking the remote at landing time
         *     cannot go stale the way a second copy of the answer would.
         * @enum {string}
         */
        Landing: "merge" | "pull_request" | "none";
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
        MemoryDto: {
            created_at: string;
            expires_at: string;
            id: string;
            repository_id: string;
            source_goal_id: string;
            source_session_id: string;
            source_task_id?: string | null;
            text: string;
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
         *     Agents talk to each other through one channel, and this is what tells a
         *     message from the three steps of a review. A verdict used to be a row of its own; it is a
         *     message like the rest now, which is what carries a review's whole
         *     conversation in one place.
         *
         *     One kind carries everything the agents say outside a review, whether it
         *     asks something or answers it. There is no `answer` kind and no `reply`
         *     tool: an answer is a message to whoever asked, addressed the way the
         *     question was, so nothing threads. Each message reaches its agent as a turn
         *     — which is why the tool that sends one takes questions and answers and
         *     nothing else, no acknowledgement and no thanks.
         *
         *     The kind is what the daemon reads. Two of them move the task
         *     ([`TaskStatus`]), and the rest are said and left.
         * @enum {string}
         */
        MessageKind: "review_request" | "approve" | "request_changes" | "message";
        /**
         * @description One thing an agent can be pinned to, as served by `GET /v1/models`: a
         *     registry agent on a model discovery found it offering
         *     (`claude-agent-acp:claude-opus-5`). Every entry names both halves — there
         *     is no bare-agent entry, because a model is required wherever an agent is
         *     pinned.
         *
         *     The id is what a request writes as its `model`, whole. `agent_id` is its
         *     registry prefix. The rest is what the agent itself said when discovery
         *     asked: one line about the model, and the efforts it can be run at.
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
            /** @example claude-agent-acp:claude-opus-5 */
            id: string;
        };
        /**
         * @description A stored session of an ACP agent that Ariadne did not start, listed over
         *     `session/list`.
         */
        OutsideSessionDto: {
            /**
             * @description Which ACP registry agent this session belongs to (`GET
             *     /v1/acp-agents`).
             */
            agent_id: string;
            first_prompt: string;
            /** @description The id the agent loads this conversation back by. */
            internal_session_id: string;
            last_activity_at: string;
            working_directory: string;
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
         * @description How the ACP runtime answers a tool permission request.
         * @enum {string}
         */
        PermissionMode: "auto" | "ask" | "learn";
        /**
         * @description One reviewer picking the winning author of a task staffed with several:
         *     the author whose branch lands.
         */
        PickWinnerRequest: {
            /** @description Id of the author picked, one of the task's authors. */
            author_agent_id: string;
        };
        /**
         * @description The author reporting the pull or merge request it opened for a task, so
         *     the user has somewhere to go and read it: taken off `gh pr create`'s output
         *     and recorded on the task.
         */
        RecordPullRequestRequest: {
            /** @description The request's URL, e.g. `https://github.com/owner/repo/pull/12`. */
            url: string;
        };
        RepositoryDto: {
            base_branch: string;
            created_at: string;
            description?: string | null;
            id: string;
            /** @description Absolute path of the checkout. */
            path: string;
            updated_at: string;
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
        /**
         * @description Where an agent sits: the orchestrator of a goal, or the author or a
         *     reviewer of one task.
         *
         *     A seat is a position, not an identity. Every agent below the orchestrator
         *     is generic, and what it can do comes from the skills it loads; the seat is
         *     only what the state machine and the launcher need to know about it.
         * @enum {string}
         */
        Seat: "orchestrator" | "author" | "reviewer";
        /**
         * @description Body of `POST /v1/tasks/{id}/messages` and `POST /v1/goals/{id}/messages`.
         *
         *     Who it is *from* comes from the session header rather than the body: an
         *     agent cannot send a message as somebody else, and a message with no session
         *     behind it is the user's.
         */
        SendMessageRequest: {
            body: string;
            kind: components["schemas"]["MessageKind"];
            /** @description Who it is for. `orchestrator` needs no agent id — a goal has one. */
            to_actor: components["schemas"]["Actor"];
            /**
             * @description The staffed agent it is for, as `GET /v1/tasks/{id}` lists them.
             *     Required for `author` and `reviewer`, refused for the orchestrator.
             */
            to_agent_id?: string | null;
        };
        SessionDto: {
            attention_reason?: null | components["schemas"]["AttentionReason"];
            /** @description When the current `attention_reason` was first raised. */
            attention_since?: string | null;
            created_at: string;
            /**
             * @description Effort that model was launched at, off the same pin as `model`; null =
             *     whatever the agent runs it at.
             * @example high
             */
            effort?: string | null;
            ended_at?: string | null;
            goal_id: string;
            id: string;
            /** @description The ACP agent's own session id. */
            internal_session_id?: string | null;
            last_activity_at?: string | null;
            /**
             * @description Model requested at launch, `<agent>:<model>`: the registry agent the
             *     session runs on, and the model of it.
             */
            model: string;
            seat: components["schemas"]["Seat"];
            status: components["schemas"]["SessionStatus"];
            /**
             * @description The staffed agent this session runs; None for an orchestrator,
             *     which no task staffs.
             */
            task_agent_id?: string | null;
            /** @description None = orchestrator session. */
            task_id?: string | null;
            /**
             * @description What this session's agent has spent, summed over every transcript it
             *     reported under. Zeros while nothing has been reported.
             */
            usage: components["schemas"]["TokenUsageDto"];
            worktree_path?: string | null;
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
         *     `:` and often `/` (`opencode-acp:anthropic/claude-sonnet-4`) — which is a
         *     path of its own, not a segment of one.
         */
        SetModelEnabledRequest: {
            /** @description What it becomes. */
            enabled: boolean;
            /**
             * @description The entry, as `GET /v1/models` spells its `id`.
             * @example claude-agent-acp:claude-opus-5
             */
            id: string;
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
        SkillSeat: "orchestrator" | "task";
        TaskAgentDto: {
            /**
             * @description The branch this agent works on: the task branch for the first author,
             *     a suffixed sibling of it for every later one. None for a reviewer,
             *     which owns no branch.
             */
            branch?: string | null;
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
            /** @description `author` or `reviewer`. */
            seat: components["schemas"]["Seat"];
            /**
             * @description The skills this agent loads, in the order they reach it.
             * @example [
             *       "coding",
             *       "testing"
             *     ]
             */
            skills: string[];
        };
        /**
         * @description Payload of `task_branch_updated`: where a task's branch points now.
         *
         *     A commit in the author's worktree changes nothing in the store, so no
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
             * @description The agents staffed on the task: the authors first, then the reviewers
             *     in review order. What each one can do is the skills it carries. Most
             *     tasks staff one author; one staffed with several runs them in
             *     parallel, and the reviewers pick the branch that lands.
             */
            agents: components["schemas"]["TaskAgentDto"][];
            branch: string;
            created_at: string;
            /** @description Ids of tasks that must merge before this one starts. */
            depends_on: string[];
            description: string;
            goal_id: string;
            id: string;
            /**
             * @description How the task ends: a change on the base branch, a request somebody
             *     else merges, or nothing at all.
             */
            landing: components["schemas"]["Landing"];
            merge_commit?: string | null;
            /**
             * @description The author the reviewers picked, on a task staffed with several: the
             *     one whose branch lands. None for a one-author task, and until the
             *     pick settles.
             */
            picked_agent_id?: string | null;
            /**
             * @description The picks the reviewers have recorded so far, oldest first. Empty for
             *     a one-author task.
             */
            picks: components["schemas"]["TaskPickDto"][];
            /**
             * @description URL of the pull or merge request the task was published as, once its
             *     author has reported one; None for a task landed directly.
             */
            pr_url?: string | null;
            /**
             * @description Why a `failed` or `cancelled` task ended — the author's own
             *     `fail_task` reason, a dependency that never landed, a cancelled goal.
             *     None for every other status, and for an ending nobody gave a reason
             *     for.
             */
            reason?: string | null;
            /** @description Id of the repository the task works in, one of its goal's. */
            repo_id: string;
            /** @description Set when the agent went idle without advancing the task. */
            stalled: boolean;
            status: components["schemas"]["TaskStatus"];
            title: string;
            updated_at: string;
            /** @description What the agents of this task have spent between them. */
            usage: components["schemas"]["TaskUsageDto"];
            worktree_path?: string | null;
        };
        /**
         * @description One agent staffed on a task: where it sits, what it knows, and what it
         *     runs on.
         *
         *     The agent has no identity of its own. `seat` says only whether it authors
         *     the task or reviews it; the skills are what it can do. What it runs on was
         *     sized by the orchestrator when it staffed the task, or chosen by the user
         *     since — either way it is what this agent runs on, and nothing behind it
         *     changes that.
         *     One reviewer's pick of the winning author, on a task staffed with several
         *     authors.
         */
        TaskPickDto: {
            /** @description The author it picked. */
            author_agent_id: string;
            created_at: string;
            /** @description The reviewer that picked. One pick per reviewer per task. */
            reviewer_agent_id: string;
        };
        /**
         * @description Task lifecycle status.
         * @enum {string}
         */
        TaskStatus: "pending" | "ready" | "in_progress" | "under_review" | "changes_requested" | "approved" | "finished" | "cancelled" | "failed";
        TaskTransitionDto: {
            actor: string;
            created_at: string;
            from_status: string;
            id: string;
            reason?: string | null;
            to_status: string;
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
         * @description What a task cost, by who spent it: its author, its reviewers one entry
         *     each, and the total of every session on the task.
         */
        TaskUsageDto: {
            /** @description The author's own, across every run of it. */
            author: components["schemas"]["TokenUsageDto"];
            /**
             * @description One entry per reviewer that has a session on the task, every review
             *     round of it summed, in review order. A reviewer whose session has yet
             *     to report anything is listed with zeros; one that has never been
             *     spawned is not listed at all.
             */
            reviewers: components["schemas"]["AgentUsageDto"][];
            /** @description Every session on the task summed, whatever its seat. */
            total: components["schemas"]["TokenUsageDto"];
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
            /** @description Required when `to` is `finished`, unless the task lands nothing. */
            merge_commit?: string | null;
            reason?: string | null;
            to: components["schemas"]["TaskStatus"];
        };
        /** @description Body of `PUT /v1/agents/{id}`: the whole new flag list, empty included. */
        UpdateAgentConfigRequest: {
            extra_flags: string[];
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateRepositoryRequest: {
            base_branch?: string | null;
            /** @description New description, or empty to clear it. Absent = unchanged. */
            description?: string | null;
            path?: string | null;
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateSkillRequest: {
            /**
             * @description The new document. Absent = unchanged; putting a built-in back on the
             *     text Ariadne ships is `POST /v1/skills/{name}/document/reset`.
             */
            document?: string | null;
        };
        /** @description Partial update; only allowed while the task is pending/ready. */
        UpdateTaskRequest: {
            /**
             * @description The whole author list, replaced: every author is staffed afresh, with
             *     the skills and the model it names. The way to give a task several
             *     authors, or to take them back to one. `model` and `effort` above are
             *     refused while a task has several authors: each author names its own.
             */
            authors?: components["schemas"]["AgentAssignment"][] | null;
            depends_on?: string[] | null;
            description?: string | null;
            /**
             * @description The reasoning effort to run the model at: absent leaves it alone,
             *     "default" (or the empty string) puts it back on whatever the agent
             *     runs the model at, and anything else is checked against the model it
             *     will run at — the one this request names, or the task's own where it
             *     names none — and refused where that model does not take it. A `model`
             *     written without an effort runs at the agent's own default: the effort
             *     belonged to the model that was left behind.
             * @example xhigh
             */
            effort?: string | null;
            landing?: null | components["schemas"]["Landing"];
            /**
             * @description What the author runs on, `<agent>:<model>`: absent leaves the
             *     author's pins alone, and anything else pins what it spells. A model is
             *     required, so "default" and the empty string are refused — there is no
             *     default to hand the pin back to.
             * @example codex-acp:gpt-5.3-codex
             */
            model?: string | null;
            /**
             * @description The whole reviewer list, replaced: every reviewer is staffed afresh,
             *     with the skills and the model it names.
             */
            reviewers?: components["schemas"]["AgentAssignment"][] | null;
            title?: string | null;
        };
        /** @description Response of `GET /v1/version`. */
        VersionResponse: {
            /** @example ariadned */
            name: string;
            /** @example 0.1.0 */
            version: string;
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
            /** @description no such repository or orchestrator profile */
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
    sessions_list_outside: {
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
                    "application/json": components["schemas"]["OutsideSessionDto"][];
                };
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
            /** @description not an absolute path, not a git work tree, or an unknown branch */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description this path and base branch are already registered */
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
            /** @description not an absolute path, not a git work tree, or an unknown branch */
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
            /** @description this path and base branch are already registered */
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
    memories_list: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                repository_id: string;
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
                    "application/json": components["schemas"]["MemoryDto"][];
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
        };
    };
    memories_create: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                repository_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateMemoryRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MemoryDto"];
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
    memories_search: {
        parameters: {
            query: {
                /** @description Find entries that contain this text, without case sensitivity. */
                q: string;
            };
            header?: never;
            path: {
                /** @description repository id */
                repository_id: string;
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
                    "application/json": components["schemas"]["MemoryDto"][];
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
        };
    };
    memories_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description repository id */
                repository_id: string;
                /** @description memory id */
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
    sessions_list: {
        parameters: {
            query?: {
                /** @description Filter by goal id. */
                goal?: string | null;
                /** @description Filter by task id. */
                task?: string | null;
                /** @description Filter by status. */
                status?: null | components["schemas"]["SessionStatus"];
                /** @description Only sessions currently flagged as needing attention. */
                attention?: boolean | null;
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
                    "application/json": components["schemas"]["SessionDto"][];
                };
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
            /** @description SSE stream of console events (text/event-stream). A `snapshot` event carrying every event recorded so far (`[AgentEventDto]`), then an `event` per new one (`AgentEventDto`) — message chunks, thoughts, tool calls, plans, permission requests and turn status all arrive this way, in the vocabulary the ACP runtime reports them in. A client that falls behind gets a `resync` event (ResyncDto) and the connection closes. */
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
    tasks_adopt_author_session: {
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
                "application/json": components["schemas"]["AdoptOutsideSessionRequest"];
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
            query?: {
                /** @description Id of the author whose branch to read, on a task staffed with several. */
                agent?: string | null;
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
    tasks_pick_winner: {
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
                "application/json": components["schemas"]["PickWinnerRequest"];
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
            /** @description not an author of the task */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description not a reviewer session */
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the pick has not started, or this reviewer has picked already */
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_record_pull_request: {
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
                "application/json": components["schemas"]["RecordPullRequestRequest"];
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
            /** @description empty URL */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description not an author session */
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description the task is not approved */
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
}

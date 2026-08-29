/**
 * Types generated from the ariadned OpenAPI document — DO NOT EDIT BY HAND.
 * Regenerate with `npm run gen:api` (see ui/README.md).
 */

export interface paths {
    "/v1/agents": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Every agent kind's flags, current and default. */
        get: operations["agents_list"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/agents/{kind}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Replace an agent kind's flags.
         * @description The list is replaced whole, and an empty one is a legitimate answer.
         *     Restoring the defaults is this same call with the `default_flags` the GET
         *     hands out — nothing else to learn, and nothing that can drift from them.
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
         * Create a goal on registered repositories; the planner session is spawned
         *     by the scheduler once agent execution lands.
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
        /** Create a task in a goal (planner via MCP, or the user). */
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
         *     planner's call alone — it makes it once the user has validated the plan in
         *     the goal thread, and there is nothing left for the user to approve.
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
        /** Goal-level message thread (planner discussion). */
        get: operations["goals_list_messages"];
        put?: never;
        /** Post to the goal-level thread. */
        post: operations["goals_post_message"];
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
         * Everything an agent can be pinned to, `<agent_kind>[:<model>]` apiece:
         *     each agent CLI on its own — that CLI on its own default model — and
         *     then the models of it, curated for claude_code and codex, discovered
         *     live (`opencode models --verbose`) for opencode.
         * @description The union always, and grouped by agent CLI: a model is chosen by one
         *     string that carries its CLI, so nothing scopes this catalog any more.
         *     Each entry carries the reasoning efforts it can be run at, cheapest
         *     first, and what its CLI runs it at when none is passed.
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
    "/v1/profiles": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List profiles. */
        get: operations["profiles_list"];
        put?: never;
        /**
         * Create a profile.
         * @description It starts on the prompts of its role, every one of them the default: a
         *     briefing is given to it afterwards, one `PUT` per kind.
         */
        post: operations["profiles_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/profiles/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a profile by id or unique name. */
        get: operations["profiles_get"];
        /** Update a profile. */
        put: operations["profiles_update"];
        post?: never;
        /** Delete a profile (409 while referenced). */
        delete: operations["profiles_delete"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/profiles/{id}/prompts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The profile's briefing prompts, in briefing order: each one as it takes
         *     effect, saying whether that is the default of its kind or a text set here.
         */
        get: operations["profiles_list_prompts"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/profiles/{id}/prompts/{kind}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Set the text of one prompt, which is what makes it the profile's own. A
         *     template may drop every `{placeholder}` of its kind, but not name one the
         *     kind has no value for: that token would reach the agent as literal text, so
         *     it is refused here.
         */
        put: operations["profiles_update_prompt"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/profiles/{id}/prompts/{kind}/reset": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Put one prompt back on the default of its kind, dropping the text set on
         *     the profile.
         */
        post: operations["profiles_reset_prompt"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/profiles/{id}/system-prompt/reset": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Put the profile's system prompt back on the default of its role. */
        post: operations["profiles_reset_system_prompt"];
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
    "/v1/sessions/{id}/input": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Type into a session's pane: the write counterpart of the log stream.
         * @description The bytes go to tmux verbatim, so the agent sees exactly what was typed in
         *     front of it and the echo comes back through `/logs/stream` like any other
         *     pane output. Nothing is appended — a submit carries its own `\r`.
         *
         *     And it is the user acting on the session, so whatever it was flagged for
         *     comes down with the input.
         *
         *     Both halves of "live" are checked, as in `logs_stream`: the row's status,
         *     because a finished session must not be typed into, and tmux itself,
         *     because tmux names are reused and a `send-keys` at a stale name would land
         *     in a successor's pane.
         */
        post: operations["sessions_input"];
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
        /** Kill a session's tmux process. */
        post: operations["sessions_kill"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/logs": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Recent tmux pane output of a session. */
        get: operations["sessions_logs"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/logs/stream": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Follow a session's terminal output.
         * @description The stream opens with a `resize` event (`SessionPaneSize`) carrying the
         *     grid the output is drawn at: the snapshot is wrapped at that width and
         *     every later repaint is addressed in it. A live pane is measured; a
         *     finished one is reported at the last size it was seen at, if it ever was.
         *
         *     Then a `snapshot` event carrying the scrollback the `/logs` endpoint would
         *     return — as the pane's screen rather than as text: it ends where the pane's
         *     cursor is (see [`as_screen`]), so the repaints that follow land where they
         *     were addressed. Then a `delta` event per burst of new output. Both payloads
         *     are a `SessionLogChunk`: raw terminal bytes, escape sequences and all, are
         *     JSON-encoded so they cannot break SSE's line framing.
         *
         *     A pane resized under the stream — by `ariadne attach`, say — sends a
         *     `resize` and a *fresh* `snapshot` rather than continuing with deltas: the
         *     output in flight straddles the change and belongs to neither grid, so the
         *     client starts over at the new one. `snapshot` therefore means "replace
         *     everything you have", whenever it arrives. Nothing is sent in between: a
         *     delta drawn at a grid the client does not have is the corruption this is
         *     all here to avoid. If no coherent screen can be had — the pane cannot be
         *     read, or keeps changing shape while it is — the connection is closed
         *     *without* an `end`, at the opening as much as later on: the session is not
         *     over, and a fresh connection is the shortest way back to a grid and a
         *     screen that agree. Only a pane confirmed gone ends a stream.
         *
         *     When the session ends — or if it was already over when the request arrived
         *     — the remaining output is flushed, a final `end` event (`SessionLogEnd`)
         *     is sent and the connection closes. There is no replay and no
         *     `Last-Event-ID`: reconnecting starts again from a fresh snapshot.
         */
        get: operations["sessions_logs_stream"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{id}/resize": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Resize a session's pane to the grid a viewer is showing it at.
         * @description The web terminal is not a tmux client, so nothing sizes the pane for it:
         *     left alone a detached session stays at tmux's 80×24 and a panel with room
         *     for far more shows a small pane in a large box. This is the attach a
         *     browser cannot make — the same `resize-window` a `tmux attach` performs —
         *     and the new grid comes back to every viewer through the log stream, which
         *     already notices a pane that changed size.
         *
         *     Several viewers each fit the pane to their own panel; the last one to ask
         *     wins, exactly as the last client to attach does in tmux.
         *
         *     Liveness is checked as it is for input: a finished session's status, and
         *     tmux itself, since a stale name may belong to a successor's pane by now.
         */
        post: operations["sessions_resize"];
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
         * Revive an ended session: new tmux, same agent conversation (resumed via
         *     the stored internal session id). Returns the session to attach to, which
         *     is this one either way — relaunched under its own id and tmux name, or
         *     untouched when its tmux turned out to be alive already.
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
        /** Edit a pending/ready task (planner or user). */
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
        /** Cancel a task (user). */
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
        /** Task conversation. */
        get: operations["tasks_list_messages"];
        put?: never;
        /** Post into the task conversation. */
        post: operations["tasks_post_message"];
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
         * Record the pull or merge request the engineer opened for a task.
         * @description The URL travels as a tool call rather than as a sentence in the
         *     conversation, so a published task is either one the UI and the CLI can
         *     point at or one that was never reported — never half-known from a message
         *     somebody has to parse.
         *
         *     Recording it writes the URL and nothing else. Telling the user where the
         *     request is belongs to the engineer that opened it — its landing briefing
         *     says to `post_message` them the link — and a notice the daemon composed
         *     beside this write would be a second author for the same news.
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
        /** Retry a failed task (user): failed -> ready. */
        post: operations["tasks_retry"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/tasks/{id}/reviews": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Reviews of a task. */
        get: operations["tasks_list_reviews"];
        put?: never;
        /** Submit a review verdict for the current round. */
        post: operations["tasks_post_review"];
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
        /** @description How one agent CLI is launched, shared by every profile that runs on it. */
        AgentConfigDto: {
            agent_kind: components["schemas"]["AgentKind"];
            /**
             * @description What Ariadne ships for this agent kind: what `extra_flags` was seeded
             *     with, and what restoring the defaults writes back — a client resets by
             *     sending these back as `extra_flags`.
             */
            default_flags: string[];
            /** @description Argv flags appended on every spawn and resume of this agent CLI. */
            extra_flags: string[];
        };
        AgentEventDto: {
            agent_kind?: string | null;
            created_at: string;
            id: string;
            /** @description e.g. session_start, post_tool_use, stop, turn_complete */
            kind: string;
            payload: unknown;
            session_id?: string | null;
            task_id?: string | null;
        };
        /**
         * @description Which coding-agent CLI a profile runs on.
         * @enum {string}
         */
        AgentKind: "claude_code" | "codex" | "opencode";
        /**
         * @description Why a live agent session needs the user's attention.
         *
         *     Orthogonal to [`SessionStatus`]: a session waiting on a permission prompt
         *     is still `running` as far as its lifecycle goes, it just cannot make
         *     progress until someone looks at it.
         * @enum {string}
         */
        AttentionReason: "waiting_permission" | "waiting_input" | "waiting_user" | "agent_error" | "disconnected" | "stalled";
        /**
         * @description Author of a conversation message.
         * @enum {string}
         */
        AuthorRole: "planner" | "engineer" | "reviewer" | "user" | "system";
        /** @description A binary as the daemon can — or cannot — find it. */
        BinaryDto: {
            agent_kind?: null | components["schemas"]["AgentKind"];
            /**
             * @description Whether it holds credentials for the service it speaks to, for the
             *     binaries that hold any: `gh auth status` and `glab auth status`, asked
             *     of the daemon's own environment because that is where the polling
             *     runs. `None` for a binary with nothing to sign in to — tmux, git, the
             *     agent CLIs — and for one that was not found to ask.
             *
             *     The distinction it exists for is the one that used to be invisible: a
             *     `gh` that is installed and signed out answers every poll of a pull
             *     request with a failure, and a task published to a forge is then
             *     watched by nothing.
             */
            authenticated?: boolean | null;
            /**
             * @description Executable name as it is looked up on PATH ("claude", "tmux").
             * @example claude
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
        CreateGoalRequest: {
            description?: string;
            /**
             * Format: int64
             * @description Max tasks the planner may create (default: unbounded).
             */
            max_tasks?: number | null;
            /**
             * @description What the planner runs on, `<agent_kind>[:<model>]` — the agent CLI and,
             *     after a `:`, the model of it: `codex`, `codex:gpt-5.3-codex`,
             *     `opencode:ollama/llama3:8b`. The model half is free text, handed to
             *     that CLI as typed; an agent CLI on its own runs it on its own default
             *     model, and a string naming no agent CLI is refused. Omitted (or
             *     "default") = the planner profile's own model, as it stands now.
             * @example codex:gpt-5.3-codex
             */
            model?: string | null;
            /** @description Planner profile id or unique name. */
            planner_profile: string;
            /** @description Ids of registered repositories (`POST /v1/repositories`); at least one. */
            repository_ids: string[];
            /**
             * Format: int64
             * @description Approvals required to merge a task (default 1).
             */
            required_approvals?: number | null;
            title: string;
        };
        CreateMessageRequest: {
            body: string;
            /**
             * @description Whom to address: a profile id or name, as tasks name their profiles, or
             *     the literal `"user"`. Omitted leaves the message addressed to the
             *     thread. Only a participant of the thread may be addressed.
             * @example reviewer-default
             */
            to?: string | null;
        };
        CreateProfileRequest: {
            /**
             * @description What this profile runs on, `<agent_kind>[:<model>]` — the agent CLI
             *     and, after a `:`, the model of it: `codex`, `codex:gpt-5.3-codex`,
             *     `opencode:ollama/llama3:8b`. A string naming no agent CLI is refused.
             *     Omitted (or "default") = auto: the first installed agent CLI at spawn
             *     time, on its own default model.
             * @example codex:gpt-5.3-codex
             */
            model?: string | null;
            /** @example rust-engineer */
            name: string;
            role: components["schemas"]["Role"];
            /**
             * @description Absent or null = the default of the role, which the profile then
             *     follows. Briefings are set afterwards, one `PUT` per kind.
             */
            system_prompt?: string | null;
        };
        CreateRepositoryRequest: {
            /** @description Omit for the repo's currently checked-out branch. */
            base_branch?: string | null;
            description?: string | null;
            merge_strategy?: null | components["schemas"]["MergeStrategy"];
            /**
             * @description Absolute path of an existing git work tree.
             * @example /home/me/dev/ariadne
             */
            path: string;
        };
        CreateReviewRequest: {
            body?: string | null;
            /**
             * @description Reviewer profile id or name. Derived from the session context when the
             *     call comes from an agent; required for user-submitted reviews.
             */
            reviewer_profile?: string | null;
            verdict: components["schemas"]["ReviewVerdict"];
        };
        CreateTaskRequest: {
            /** @description Task ids this task depends on. */
            depends_on?: string[];
            description?: string;
            /** @description Engineer profile id or unique name. */
            engineer_profile: string;
            /**
             * @description What the engineer runs on, `<agent_kind>[:<model>]`; omitted (or
             *     "default") = the engineer profile's own model. Resolved the way
             *     [`ReviewerAssignment::model`] is.
             * @example codex:gpt-5.3-codex
             */
            model?: string | null;
            /**
             * @description Id of one of the goal's repositories; may be omitted when the goal
             *     works in exactly one.
             */
            repo_id?: string | null;
            /** @description The reviewers of the task, in review order. At least one. */
            reviewers: components["schemas"]["ReviewerAssignment"][];
            title: string;
        };
        /** @description The daemon's own environment, as `ariadne doctor` renders it. */
        DaemonReportDto: {
            /** @description One entry per [`AgentKind`], in `AgentKind::ALL` order. */
            agents: components["schemas"]["BinaryDto"][];
            db: components["schemas"]["PathStateDto"];
            /** @description Home directory the daemon resolved, and the socket it listens on. */
            home: string;
            /** @description The daemon's `PATH`, the one every agent, tmux and git lookup uses. */
            path?: string | null;
            socket_path: string;
            /**
             * @description The other binaries the daemon runs: tmux and git, without which no
             *     session can be spawned at all, and the forge CLIs `gh` and `glab`,
             *     which are what a published task is watched through.
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
            /** @description A terminal goal was deleted, tasks and messages with it. */
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
            data: components["schemas"]["MessageDto"];
            /** @enum {string} */
            event: "message_created";
        } | {
            data: components["schemas"]["ReviewDto"];
            /** @enum {string} */
            event: "review_created";
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
            /** @description A raw agent event reported by a hook. */
            data: components["schemas"]["AgentEventDto"];
            /** @enum {string} */
            event: "agent_event";
        } | {
            data: components["schemas"]["ProfileDto"];
            /** @enum {string} */
            event: "profile_created";
        } | {
            data: components["schemas"]["ProfileDto"];
            /** @enum {string} */
            event: "profile_updated";
        } | {
            data: components["schemas"]["DeletedDto"];
            /** @enum {string} */
            event: "profile_deleted";
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
        };
        /**
         * @description Body of `POST /v1/goals/{id}/finalize`: the planner ends planning once the
         *     user has validated the plan in the goal thread, and execution starts. The
         *     planner's call, not the user's.
         */
        FinalizePlanRequest: {
            /** @description Plan summary, recorded in the goal thread. */
            summary: string;
        };
        GoalDto: {
            created_at: string;
            description: string;
            id: string;
            /**
             * Format: int64
             * @description None = unbounded.
             */
            max_tasks?: number | null;
            /**
             * @description What the planner runs on, `<agent_kind>[:<model>]`: the agent CLI and,
             *     after a `:`, the model of it (`codex`, `claude_code:claude-opus-5`).
             *     Pinned when the goal was created, from the model chosen for it or,
             *     where none was, from the planner profile — editing the profile
             *     afterwards leaves it alone. None = auto: the first installed CLI,
             *     resolved at spawn time, on its own default model.
             * @example claude_code:claude-opus-5
             */
            model?: string | null;
            planner_profile_id: string;
            /**
             * @description The registered repositories the goal works in, as they stand now: a
             *     goal references them, so an edit to one shows up here.
             */
            repos: components["schemas"]["RepositoryDto"][];
            /** Format: int64 */
            required_approvals: number;
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
         * @description What a goal cost, by the role that spent it. Grouped by role rather than
         *     by profile: a goal's engineers are as many as it has tasks, and what is
         *     read at this height is where the tokens went, not which agent went there.
         */
        GoalUsageDto: {
            /** @description Every engineer session of every task of the goal. */
            engineers: components["schemas"]["TokenUsageDto"];
            /** @description The planner's sessions, which belong to no task. */
            planner: components["schemas"]["TokenUsageDto"];
            /** @description Every reviewer session of every task of the goal, all rounds. */
            reviewers: components["schemas"]["TokenUsageDto"];
            /** @description Every session of the goal summed, the planner's included. */
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
        /**
         * @description How a repository takes the change a task lands on its base branch: the one
         *     thing about a repository the engineer that finishes a task has to be told,
         *     since the commands it runs at the end differ entirely between the two.
         *
         *     Which forge a published request goes to is *not* here: `origin` says
         *     whether it is GitHub or GitLab, and asking the remote at landing time
         *     cannot go stale the way a second copy of the answer would.
         * @enum {string}
         */
        MergeStrategy: "direct" | "pull_request";
        MessageDto: {
            author_role: components["schemas"]["AuthorRole"];
            author_session_id?: string | null;
            body: string;
            created_at: string;
            goal_id: string;
            id: string;
            recipient?: null | components["schemas"]["MessageRecipientDto"];
            /** @description None = goal-level thread. */
            task_id?: string | null;
        };
        /**
         * @description A message's addressee, resolved: an agent profile comes with its name, so a
         *     client renders "to Alice" without a lookup of its own.
         */
        MessageRecipientDto: {
            kind: components["schemas"]["RecipientKind"];
            /** @description The addressed profile, set exactly when `kind` is `profile`. */
            profile_id?: string | null;
            /** @description That profile's name, unless the profile is gone. */
            profile_name?: string | null;
        };
        /**
         * @description One thing an agent can be pinned to, as served by `GET /v1/models`: an
         *     agent CLI on a model of it (`claude_code:claude-fable-5`), or an agent CLI
         *     on its own, which is that CLI on its own default model.
         *
         *     The id is what a request writes as its `model`, whole. `agent_kind` is the
         *     same fact taken apart, so a picker can group the catalog by CLI without
         *     parsing anything.
         */
        ModelDto: {
            /** @description The agent CLI this entry runs on. */
            agent_kind: components["schemas"]["AgentKind"];
            /**
             * @description What the agent CLI runs this model at when no effort is passed.
             * @example high
             */
            default_effort?: string | null;
            /** @description One-line capability summary (absent for discovered opencode models). */
            description?: string | null;
            /**
             * @description The reasoning efforts this entry can be run at, cheapest first; empty
             *     where the model takes none, or where nothing knows what it takes.
             */
            efforts: string[];
            /** @example claude_code:claude-fable-5 */
            id: string;
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
        ProfileDto: {
            created_at: string;
            id: string;
            /**
             * @description What this profile runs on, `<agent_kind>[:<model>]`: the agent CLI and,
             *     after a `:`, the model of it. None = auto: the first installed agent
             *     CLI (claude_code, then codex, then opencode), resolved at spawn time,
             *     on its own default model.
             * @example claude_code:claude-opus-5
             */
            model?: string | null;
            name: string;
            role: components["schemas"]["Role"];
            /**
             * @description The system prompt this profile is spawned with: the one set on it, or
             *     the default of its role while it has none of its own.
             */
            system_prompt: string;
            /**
             * @description Whether `system_prompt` is that role default rather than a text set on
             *     this profile.
             */
            system_prompt_is_default: boolean;
            updated_at: string;
        };
        /**
         * @description One of the briefing prompts a profile owns beside its system prompt, as it
         *     takes effect.
         */
        ProfilePromptDto: {
            /**
             * @description Template text with `{placeholder}` tokens the daemon fills in: the one
             *     set on the profile, or the default of the kind while it has none.
             */
            content: string;
            /**
             * @description Whether `content` is that default rather than a text set on this
             *     profile.
             */
            is_default: boolean;
            kind: components["schemas"]["PromptKind"];
            /**
             * @description When the text set on the profile was last written; null while the
             *     default stands, which nothing dates.
             */
            updated_at?: string | null;
        };
        /** @description What one profile spent on a task, named the way a reader addresses it. */
        ProfileUsageDto: {
            profile_id: string;
            /** @description The profile's name; None only if that profile is gone. */
            profile_name?: string | null;
            usage: components["schemas"]["TokenUsageDto"];
        };
        /**
         * @description A prompt a profile owns beside its system prompt: one of the texts an
         *     agent of that role is started, resumed or nudged with. Every briefing a
         *     profile carries is one of these, and each kind belongs to the role that
         *     receives it (see [`PromptKind::roles`]).
         * @enum {string}
         */
        PromptKind: "planner_briefing" | "planner_resume" | "engineer_briefing" | "engineer_resume" | "changes_requested" | "reviewer_briefing" | "reviewer_resume" | "landing_direct" | "landing_pull_request";
        /**
         * @description Who a conversation message is addressed to: one agent profile, or the
         *     human user. Orthogonal to the author role, and optional — a message with
         *     no recipient is addressed to the thread rather than to anyone in it.
         * @enum {string}
         */
        RecipientKind: "profile" | "user";
        /**
         * @description The engineer reporting the pull or merge request it opened for a task, so
         *     the user has somewhere to go and read it: taken off `gh pr create`'s output
         *     rather than out of the conversation.
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
            /** @description How a task lands on `base_branch` here. */
            merge_strategy: components["schemas"]["MergeStrategy"];
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
        ReviewDto: {
            body?: string | null;
            created_at: string;
            id: string;
            /** @description The reviewer of the round whose verdict this is. */
            reviewer_profile_id: string;
            /** Format: int64 */
            round: number;
            session_id?: string | null;
            task_id: string;
            verdict: components["schemas"]["ReviewVerdict"];
        };
        /**
         * @description Review verdict for one reviewer in one round.
         * @enum {string}
         */
        ReviewVerdict: "approve" | "request_changes";
        /**
         * @description One reviewer of a task: the profile that reviews, and what it is to run on.
         *
         *     The model is written `<agent_kind>[:<model>]`: the agent CLI on its own
         *     runs it on its own default model, an agent with a model after the `:` pins
         *     both, and a string naming no agent CLI is refused — nothing here derives
         *     one from the other. Omitted, the slot takes the profile's own model as it
         *     stands when the slot is assigned.
         */
        ReviewerAssignment: {
            /**
             * @description What this reviewer runs on, `<agent_kind>[:<model>]`; omitted (or
             *     "default") = the profile's own.
             * @example codex:o3
             */
            model?: string | null;
            /** @description Reviewer profile id or unique name. */
            profile: string;
        };
        /**
         * @description The role an agent plays in the orchestration.
         * @enum {string}
         */
        Role: "planner" | "engineer" | "reviewer";
        SessionDto: {
            agent_kind: components["schemas"]["AgentKind"];
            attention_reason?: null | components["schemas"]["AttentionReason"];
            /** @description When the current `attention_reason` was first raised. */
            attention_since?: string | null;
            created_at: string;
            ended_at?: string | null;
            goal_id: string;
            id: string;
            /** @description Agent-internal id: claude session uuid / codex thread id / opencode session id. */
            internal_session_id?: string | null;
            last_activity_at?: string | null;
            /** @description Model requested at launch; null = the agent CLI's default. */
            model?: string | null;
            profile_id: string;
            /** Format: int64 */
            review_round?: number | null;
            role: components["schemas"]["Role"];
            status: components["schemas"]["SessionStatus"];
            /** @description None = planner session. */
            task_id?: string | null;
            tmux_session: string;
            /**
             * @description What this session's agent has spent, summed over every transcript it
             *     reported under. Zeros while nothing has been reported.
             */
            usage: components["schemas"]["TokenUsageDto"];
            worktree_path?: string | null;
        };
        /** @description Body of `POST /v1/sessions/{id}/input`. */
        SessionInputRequest: {
            /**
             * @description Keystrokes to type into the pane, exactly as the terminal produced
             *     them: `\r` for Return, `\x03` for Ctrl-C, `\x1b[A` for Up. Sent
             *     verbatim — nothing is appended, so a submit has to carry its own `\r`.
             */
            data: string;
        };
        /**
         * @description Payload of the `snapshot` and `delta` events of
         *     `GET /v1/sessions/{id}/logs/stream`.
         *
         *     Terminal output is raw bytes — newlines, escape sequences, control
         *     characters — none of which survive SSE's line-oriented `data:` framing on
         *     their own, so every chunk travels as JSON.
         */
        SessionLogChunk: {
            /** @description Terminal output as written, decoded lossily from UTF-8. */
            chunk: string;
        };
        /**
         * @description Payload of the final `end` event of `GET /v1/sessions/{id}/logs/stream`:
         *     the session is over and no further output is coming.
         */
        SessionLogEnd: {
            session_id: string;
        };
        /** @description Response of `GET /v1/sessions/{id}/logs`. */
        SessionLogsResponse: {
            /** @description Recent pane contents captured from tmux. */
            logs: string;
            session_id: string;
            tmux_session: string;
        };
        /**
         * @description Payload of the `resize` event of `GET /v1/sessions/{id}/logs/stream`: the
         *     grid the pane is drawing against, in cells.
         *
         *     A terminal stream only means anything at a size. The agent addresses the
         *     cursor and erases lines against *this* grid, so a viewer that renders the
         *     bytes at any other one has every repaint land on the wrong row.
         */
        SessionPaneSize: {
            /** Format: int32 */
            cols: number;
            /** Format: int32 */
            rows: number;
        };
        /**
         * @description Body of `POST /v1/sessions/{id}/resize`.
         *
         *     The grid a viewer wants the pane to draw at, in cells — what a terminal
         *     hands its pty when its window changes, and what `tmux attach` gives the
         *     pane it attaches to.
         */
        SessionResizeRequest: {
            /** Format: int32 */
            cols: number;
            /** Format: int32 */
            rows: number;
        };
        /**
         * @description Agent session lifecycle status.
         * @enum {string}
         */
        SessionStatus: "starting" | "running" | "idle" | "exited" | "failed";
        /**
         * @description Payload of `task_branch_updated`: where a task's branch points now.
         *
         *     A commit in the engineer's worktree changes nothing in the store, so no
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
            branch: string;
            created_at: string;
            /** @description Ids of tasks that must merge before this one starts. */
            depends_on: string[];
            description: string;
            engineer_profile_id: string;
            /**
             * @description Name of the engineer's profile, the way a message addresses it; None
             *     only if that profile is gone.
             */
            engineer_profile_name?: string | null;
            goal_id: string;
            id: string;
            merge_commit?: string | null;
            /**
             * @description What the engineer runs on, `<agent_kind>[:<model>]`: the agent CLI and,
             *     after a `:`, the model of it (`codex`, `claude_code:claude-opus-5`).
             *     Pinned when the task was created, from the model chosen for it at
             *     creation or on an edit or, where none was, from the engineer profile —
             *     editing the profile afterwards leaves it alone. None = auto: the first
             *     installed CLI, resolved at spawn time, on its own default model.
             * @example claude_code:claude-opus-5
             */
            model?: string | null;
            /**
             * @description Name of the planner profile of the task's goal, which takes part in
             *     every task thread without being a field of the task.
             */
            planner_profile_name?: string | null;
            /**
             * @description URL of the pull or merge request the task was published as, once its
             *     engineer has reported one; None for a task landed directly.
             */
            pr_url?: string | null;
            /** @description Id of the repository the task works in, one of its goal's. */
            repo_id: string;
            /** Format: int64 */
            review_round: number;
            /** @description Reviewer slots in planner-assigned order, each carrying its own pin. */
            reviewers: components["schemas"]["TaskReviewerDto"][];
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
         * @description One reviewer slot of a task: which profile reviews it, and what that
         *     reviewer was pinned to run on when the slot was assigned — the profile's
         *     own model, or the one chosen for the slot. Pinned the same way the engineer
         *     is, and read the same way: what a reviewer of this task runs on, not what
         *     its profile says today.
         */
        TaskReviewerDto: {
            /**
             * @description What this reviewer runs on, `<agent_kind>[:<model>]`. None = auto: the
             *     first installed CLI, resolved at spawn time, on its own default model.
             * @example codex:o3
             */
            model?: string | null;
            profile_id: string;
            /**
             * @description Name of the reviewer's profile, the way a message addresses it; None
             *     only if that profile is gone.
             */
            profile_name?: string | null;
        };
        /**
         * @description Task lifecycle status.
         * @enum {string}
         */
        TaskStatus: "pending" | "ready" | "in_progress" | "under_review" | "changes_requested" | "approved" | "merged" | "cancelled" | "failed";
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
         * @description What a task cost, by who spent it: its engineer, its reviewers one entry
         *     each, and the total of every session on the task.
         */
        TaskUsageDto: {
            /** @description The engineer's own, across every run of it. */
            engineer: components["schemas"]["TokenUsageDto"];
            /**
             * @description One entry per reviewer profile that has a session on the task, every
             *     review round of it summed, ordered like `reviewers`. A reviewer whose
             *     session has yet to report anything is listed with zeros; one that has
             *     never been spawned is not listed at all.
             */
            reviewers: components["schemas"]["ProfileUsageDto"][];
            /** @description Every session on the task summed, whatever its role. */
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
            /** @description Required when `to` is `merged`. */
            merge_commit?: string | null;
            reason?: string | null;
            to: components["schemas"]["TaskStatus"];
        };
        /** @description Body of `PUT /v1/agents/{kind}`: the whole new flag list, empty included. */
        UpdateAgentConfigRequest: {
            extra_flags: string[];
        };
        /** @description Body of `PUT /v1/profiles/{id}/prompts/{kind}`: the whole new text. */
        UpdateProfilePromptRequest: {
            content: string;
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateProfileRequest: {
            /**
             * @description What this profile runs on, `<agent_kind>[:<model>]`, or "default" (or
             *     the empty string) to clear it back to auto — the first installed CLI at
             *     spawn time, on its own default model. Absent = unchanged.
             * @example codex:gpt-5.3-codex
             */
            model?: string | null;
            name?: string | null;
            /**
             * @description New system prompt. Absent = unchanged; putting it back on the role
             *     default is `POST /v1/profiles/{id}/system-prompt/reset`.
             */
            system_prompt?: string | null;
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateRepositoryRequest: {
            base_branch?: string | null;
            /** @description New description, or empty to clear it. Absent = unchanged. */
            description?: string | null;
            merge_strategy?: null | components["schemas"]["MergeStrategy"];
            path?: string | null;
        };
        /** @description Partial update; only allowed while the task is pending/ready. */
        UpdateTaskRequest: {
            depends_on?: string[] | null;
            description?: string | null;
            /**
             * @description What the engineer runs on, `<agent_kind>[:<model>]`: absent leaves the
             *     task's pins alone, "default" (or the empty string) puts them back on
             *     the engineer profile's own model as it stands now, and anything else
             *     pins what it spells. The same clearing word
             *     [`crate::profiles::UpdateProfileRequest::model`] takes.
             * @example codex:gpt-5.3-codex
             */
            model?: string | null;
            /**
             * @description The whole reviewer list, replaced: each slot is cut afresh and pinned
             *     to the model it names or, where it names none, to its profile's.
             */
            reviewers?: components["schemas"]["ReviewerAssignment"][] | null;
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
                /** @description claude_code, codex or opencode */
                kind: string;
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
            /** @description unknown agent kind */
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
            /** @description no such repository or planner profile */
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
    goals_list_messages: {
        parameters: {
            query?: {
                /** @description Return items with id greater than this. */
                after?: string | null;
                /** @description Max items to return (default 50, cap 200). */
                limit?: number | null;
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
    goals_post_message: {
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
                "application/json": components["schemas"]["CreateMessageRequest"];
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
            /** @description unknown addressee, or one taking no part in the goal */
            400: {
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
    profiles_list: {
        parameters: {
            query?: {
                /** @description Filter by role. */
                role?: null | components["schemas"]["Role"];
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
                    "application/json": components["schemas"]["ProfileDto"][];
                };
            };
        };
    };
    profiles_create: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateProfileRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ProfileDto"];
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
    profiles_get: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id or name */
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
                    "application/json": components["schemas"]["ProfileDto"];
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
    profiles_update: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id */
                id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateProfileRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ProfileDto"];
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
    profiles_delete: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id */
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
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    profiles_list_prompts: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id or name */
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
                    "application/json": components["schemas"]["ProfilePromptDto"][];
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
    profiles_update_prompt: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id or name */
                id: string;
                /** @description prompt kind, e.g. engineer_briefing */
                kind: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateProfilePromptRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ProfilePromptDto"];
                };
            };
            /** @description unknown kind, a kind the profile's role does not own, or a placeholder the kind cannot fill in */
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
        };
    };
    profiles_reset_prompt: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id or name */
                id: string;
                /** @description prompt kind, e.g. engineer_briefing */
                kind: string;
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
                    "application/json": components["schemas"]["ProfilePromptDto"];
                };
            };
            /** @description unknown kind, or a kind the profile's role does not own */
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
        };
    };
    profiles_reset_system_prompt: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description profile id or name */
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
                    "application/json": components["schemas"]["ProfileDto"];
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
            /** @description not an absolute path, not a git work tree, or unknown branch */
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
            /** @description not an absolute path, not a git work tree, or unknown branch */
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
    sessions_input: {
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
                "application/json": components["schemas"]["SessionInputRequest"];
            };
        };
        responses: {
            /** @description Input handed to the pane */
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
    sessions_logs: {
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
                    "application/json": components["schemas"]["SessionLogsResponse"];
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
    sessions_logs_stream: {
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
            /** @description SSE stream of terminal output (text/event-stream). A `resize` event with the grid the output is drawn at (`{"cols": 80, "rows": 24}`, SessionPaneSize), then a `snapshot` event with the current scrollback and a `delta` event per burst of new output — both `{"chunk": "..."}` (SessionLogChunk). A pane resized under the stream sends a new `resize` followed by a fresh `snapshot`, which replaces everything sent so far. A final `end` event (SessionLogEnd) closes the stream when the session is over. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": components["schemas"]["SessionLogChunk"];
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
    sessions_resize: {
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
                "application/json": components["schemas"]["SessionResizeRequest"];
            };
        };
        responses: {
            /** @description Pane resized */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
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
    tasks_list_messages: {
        parameters: {
            query?: {
                /** @description Return items with id greater than this. */
                after?: string | null;
                /** @description Max items to return (default 50, cap 200). */
                limit?: number | null;
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
    tasks_post_message: {
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
                "application/json": components["schemas"]["CreateMessageRequest"];
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
            /** @description unknown addressee, or one taking no part in the task */
            400: {
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
            /** @description not an engineer session */
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
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    tasks_list_reviews: {
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
                    "application/json": components["schemas"]["ReviewDto"][];
                };
            };
        };
    };
    tasks_post_review: {
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
                "application/json": components["schemas"]["CreateReviewRequest"];
            };
        };
        responses: {
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ReviewDto"];
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

/**
 * Types generated from the ariadned OpenAPI document — DO NOT EDIT BY HAND.
 * Regenerate with `npm run gen:api` (see ui/README.md).
 */

export interface paths {
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
         *     There is **no replay or backfill**: the `id` is informational and
         *     `Last-Event-ID` is ignored. On (re)connect, refetch the REST state you care
         *     about and then follow the stream.
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
         * Create a goal. Validates repos and resolves base branches; the planner
         *     session is spawned by the scheduler once agent execution lands.
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
        delete?: never;
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
        /** Finalize planning: goal moves planning -> active (planner or user). */
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
        /** Create a profile. */
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
         *     everything you have", whenever it arrives.
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
         * @description Author of a conversation message.
         * @enum {string}
         */
        AuthorRole: "planner" | "engineer" | "reviewer" | "user" | "system";
        CreateGoalRequest: {
            description?: string;
            /**
             * Format: int64
             * @description Max tasks the planner may create (default: unbounded).
             */
            max_tasks?: number | null;
            /** @description Planner profile id or unique name. */
            planner_profile: string;
            repos: components["schemas"]["RepoSpec"][];
            /**
             * Format: int64
             * @description Approvals required to merge a task (default 1).
             */
            required_approvals?: number | null;
            title: string;
        };
        CreateMessageRequest: {
            body: string;
        };
        CreateProfileRequest: {
            agent_kind?: null | components["schemas"]["AgentKind"];
            extra_flags?: string[];
            model?: string | null;
            /** @example rust-engineer */
            name: string;
            role: components["schemas"]["Role"];
            system_prompt: string;
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
            /** @description Repo id within the goal; may be omitted when the goal has exactly one repo. */
            repo_id?: string | null;
            /** @description Reviewer profile ids or names, in review order. At least one. */
            reviewer_profiles: string[];
            title: string;
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
            data: components["schemas"]["TaskDto"];
            /** @enum {string} */
            event: "task_created";
        } | {
            /** @description Covers status transitions, edits, stall flags and worktree changes. */
            data: components["schemas"]["TaskUpdatedDto"];
            /** @enum {string} */
            event: "task_updated";
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
        };
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
            planner_profile_id: string;
            repos: components["schemas"]["GoalRepoDto"][];
            /** Format: int64 */
            required_approvals: number;
            status: components["schemas"]["GoalStatus"];
            title: string;
            updated_at: string;
        };
        GoalRepoDto: {
            base_branch: string;
            id: string;
            path: string;
        };
        /**
         * @description Goal lifecycle status.
         * @enum {string}
         */
        GoalStatus: "planning" | "active" | "completed" | "cancelled";
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
        MessageDto: {
            author_role: components["schemas"]["AuthorRole"];
            author_session_id?: string | null;
            body: string;
            created_at: string;
            goal_id: string;
            id: string;
            /** @description None = goal-level thread. */
            task_id?: string | null;
        };
        ProfileDto: {
            agent_kind?: null | components["schemas"]["AgentKind"];
            created_at: string;
            /** @description Extra argv flags appended when spawning the agent CLI. */
            extra_flags: string[];
            id: string;
            model?: string | null;
            name: string;
            role: components["schemas"]["Role"];
            system_prompt: string;
            updated_at: string;
        };
        RepoSpec: {
            /** @description Base branch tasks merge into; defaults to the repo's current branch. */
            base_branch?: string | null;
            /**
             * @description Absolute path to an existing git repository.
             * @example /home/me/projects/webapp
             */
            path: string;
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
         * @description The role an agent plays in the orchestration.
         * @enum {string}
         */
        Role: "planner" | "engineer" | "reviewer";
        SessionDto: {
            agent_kind: components["schemas"]["AgentKind"];
            created_at: string;
            ended_at?: string | null;
            goal_id: string;
            id: string;
            /** @description Agent-internal id: claude session uuid / codex thread id / opencode session id. */
            internal_session_id?: string | null;
            last_activity_at?: string | null;
            profile_id: string;
            /** Format: int64 */
            review_round?: number | null;
            role: components["schemas"]["Role"];
            status: components["schemas"]["SessionStatus"];
            /** @description None = planner session. */
            task_id?: string | null;
            tmux_session: string;
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
         * @description Agent session lifecycle status.
         * @enum {string}
         */
        SessionStatus: "starting" | "running" | "idle" | "exited" | "failed";
        TaskDto: {
            branch: string;
            created_at: string;
            /** @description Ids of tasks that must merge before this one starts. */
            depends_on: string[];
            description: string;
            engineer_profile_id: string;
            goal_id: string;
            id: string;
            merge_commit?: string | null;
            repo_id: string;
            /** Format: int64 */
            review_round: number;
            /** @description Reviewer profile ids in planner-assigned order. */
            reviewer_profile_ids: string[];
            /** @description Set when the agent went idle without advancing the task. */
            stalled: boolean;
            status: components["schemas"]["TaskStatus"];
            title: string;
            updated_at: string;
            worktree_path?: string | null;
        };
        /**
         * @description Task lifecycle status.
         * @enum {string}
         */
        TaskStatus: "pending" | "ready" | "in_progress" | "under_review" | "changes_requested" | "approved" | "merging" | "merged" | "cancelled" | "failed";
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
        TransitionRequest: {
            /** @description Required when `to` is `merged`. */
            merge_commit?: string | null;
            reason?: string | null;
            to: components["schemas"]["TaskStatus"];
        };
        /** @description Partial update; absent fields stay unchanged. */
        UpdateProfileRequest: {
            /**
             * @description New agent kind, or "auto" to clear it (resolve the first installed
             *     CLI at spawn time). Absent = unchanged.
             */
            agent_kind?: string | null;
            extra_flags?: string[] | null;
            /**
             * @description New model, or "default" (or empty) to clear it back to the agent's
             *     default. Absent = unchanged.
             */
            model?: string | null;
            name?: string | null;
            system_prompt?: string | null;
        };
        /** @description Partial update; only allowed while the task is pending/ready. */
        UpdateTaskRequest: {
            depends_on?: string[] | null;
            description?: string | null;
            reviewer_profiles?: string[] | null;
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
            /** @description SSE stream of domain events (text/event-stream). No replay on reconnect: refetch REST state first. A lagging client gets a final `resync` event (ResyncDto) and the connection is closed. */
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
                /** @description Filter by status. */
                status?: null | components["schemas"]["GoalStatus"];
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
    sessions_list: {
        parameters: {
            query?: {
                /** @description Filter by goal id. */
                goal?: string | null;
                /** @description Filter by task id. */
                task?: string | null;
                /** @description Filter by status. */
                status?: null | components["schemas"]["SessionStatus"];
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

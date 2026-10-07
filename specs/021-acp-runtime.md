---
id: acp-runtime
status: current
updated: 2026-10-01
areas: [daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_discovery.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-daemon/tests/it/transcript_usage.rs
  - crates/ariadne-daemon/tests/it/auto_switch.rs
---

# ACP runtime

How the daemon runs an agent: as its own child process, driven over the Agent
Client Protocol.

`ariadned` spawns the agent executable with piped standard input and output,
speaks ACP version 1 over those pipes, and reports what the agent does through
the daemon's one event ingestion path. Every seat of every session runs this
way — orchestrator, author and reviewer. The runtime is
`crates/ariadne-daemon/src/acp.rs`; the launch it consumes — the command and
the launch file — is described in 007.

The wire is the `agent-client-protocol` crate's, the protocol's own Rust SDK.
The daemon owns the process — it spawns it, signals its process group and
reaps it — and hands the SDK the two pipes and nothing else
(`acp_transport`). Every request it sends is built from the SDK's typed v1
schema (`acp_calls`, `acp_schema`), so a field the protocol renames is a
compile error rather than a payload an agent ignores.

The daemon advertises no compaction. An agent compacts its own conversation
near the context limit and carries on, and the daemon asks for none — the
report said only that the agent was back at its prompt, which whatever it
does next says anyway. It was read when the daemon typed the compaction
itself and had to know when to let the agent go; that has not been true
since (007, rule 17).

Both messages an agent sends are read as the JSON it sent, not as the SDK's
typed enums. `SessionUpdate` is a closed set, and the runtime has always
handled the update kinds it knows and let the rest by, which is what keeps a
kind added to the protocol from failing a live session; and a tool call is
stored as the agent wrote it, so whatever an adapter attaches to one reaches
the console.

## Scope

In: the child process and its lifecycle, the protocol conversation, the model
and effort pins, the events the runtime reports, permission requests, prompt
delivery, liveness, and the kill.

Out: the registry, discovery, and what a launch is made of (007), which model
is pinned (011), what the session is briefed with (006), the console a person
reads and types into (008), the sweep that retires a row whose agent is gone
(009), and the optional advisory diagnosis a `session.error` may later carry
beside it, which reads what this spec's rule 14 already decided and never
changes it (024).

Discovery closes each catalog session when the agent advertises `session/close`.
For `opencode-acp`, it then runs the registry entry's program as
`session delete <session id>` with the probe's launch environment and a
discovery timeout. A failed close or delete is logged as a warning and does
not reject the agent or discard its catalog.

## Behavior

1. The agent is a direct child of the daemon. It runs the registry command
   the launch names (007), with the launch's environment, in the seat's
   working directory, on piped stdio, as the leader of a process group of
   its own. A launch for a session that already has an agent here kills that
   agent first (rule 11), and starts once it is reaped: one seat, one agent.
   A launch after a kill of the session's agent that has not been reaped yet
   waits for that reap the same way.
2. The runtime speaks ACP version 1. It sends `initialize` and refuses an
   agent that negotiates any other version (see Known gap). It then opens
   the session with `session/new` — or, for a resume, `session/resume` where
   the agent advertises it and `session/load` where it only advertises
   loading — and passes the MCP servers of the launch file every time.
   An outside loose session always opens through `session/load` (020).
   A loose session receives no task MCP server or seat instructions.
3. The model is set through `session/set_config_option` on the option of
   category `model`, or, where no option has that category, on the option
   whose id or name is `model`. The effort is set the same way, on category
   `thought_level` with `effort`, `reasoning` and `thought_level` as the
   id-or-name fallback, and only when the launch or a same-agent switch carries one. An agent that
   offers no matching option fails the launch or switch rather than run on a default
   (see Known gap).
   A same-agent switch also calls `session/set_config_option` on the live
   conversation, after its running turn and before queued prompts
   (`switch.rs::a_same_agent_switch_keeps_the_row_and_conversation`,
   `::a_same_agent_switch_during_a_turn_precedes_queued_input`). A refused
   option fails a switch between turns with the agent's reason
   (`::a_refused_same_agent_option_keeps_the_old_pin`).
   Loose sessions instead retain and record the loaded model (020).
4. Every prompt the runtime sends is the system prompt, a blank line, and
   the text of the prompt, except a command. Where text from any source starts
   with `/` and its first word, without `/`, equals the `name` of a command the
   agent last offered, the runtime sends the text alone. The first prompt of a
   launch is the one its launch file carries, if any. `user_prompt_submit`
   carries the whole as `prompt`, the text alone as `text`, and a `source`:
   `console` for console input (008), `daemon` for everything the daemon
   itself says.
5. What the agent does becomes agent events on the ingestion path (012):
   `session_start` with the agent's own session id, `user_prompt_submit`,
   tool calls as `pre_tool_use` and `post_tool_use`, `plan` with the entries
   of each ACP plan update, the turn's text (rule 6), `stop` with the stop
   reason, `permission_request` and
   `permission.replied`, `session.error`, and `session_end`. Every event
   carries the launch id (007), and the agent's session id is recorded on
   the row.
   The `stop` event carries `ariadne_usage` with the launch id as its
   source: the launch's totals read from the agent's own transcript, a Codex
   rollout or a Claude Code transcript (012 rule 16). While a turn runs, the
   runtime reads the transcript again every `Timeouts::transcript_poll` and
   writes the totals to the store, so a long turn's figure moves. Where no
   transcript is found by the end of a turn, the launch uses the prompt
   responses instead: quota totals before standard usage, read as what that
   turn spent and added to the launch's earlier turns. A launch never
   reports both.
   A `usage_update` keeps its latest `used` and `size` on the session while
   the turn runs. Its `cost`, when present, is ignored.
   An `available_commands_update` keeps the agent's latest complete command
   list while it runs, including between turns and through every `session/load`
   replay. It goes live to the console alone as
   `available_commands_update` with `{session_id, available_commands}`; each
   command is the ACP JSON the agent sent. It is never stored. A later list,
   including an empty one, replaces the earlier list. The list is dropped
   when the agent ends.
6. A turn's text is stored run by run, where the agent wrote it. A run is
   the chunks of one kind in a row, and of one message where the chunks
   carry an ACP `messageId`; it ends at a chunk of the other kind, a chunk
   that names another `messageId` than the run's, a plan, a tool call or its
   update, a permission request, or the end of the
   turn, and it is stored then, once, whole —
   `agent_thought` or `agent_message` `{session_id, text}` — before whatever
   ended it. A turn that speaks around a tool call stores its text before the
   call and its text after it as two events, and the last run comes before
   `stop`; a turn that fails stores its last run before the error. Text that
   arrives between turns is not stored. While the
   turn runs, each chunk goes live to the session's console stream alone
   (008) — `agent_message_chunk` and `agent_thought_chunk`, each
   `{session_id, text}` — and so does each non-terminal `tool_call_update`
   as `tool_call_update` `{session_id, tool_call_id, acp}`. None of the
   three is stored, and none reaches `/v1/events` or `/v1/events/stream`.
7. Tool call updates are merged per `toolCallId`: every field an update sets
   replaces the one on record, `content` included. `pre_tool_use` carries
   the call as it opened, including its input. `post_tool_use`, on a
   `completed` or `failed` update, carries the compact merged call under
   `acp` — `title`, `kind`, `status`, `content`, and `locations` — beside its
   `tool_name`. It drops `rawInput` and `tool_input`. Where a `content` entry
   has text, it is the stored output and `rawOutput` is dropped; otherwise
   `rawOutput` remains. A `content` diff and locations remain. A live
   `tool_call_update` carries the call merged so far.
8. A running turn is cancelled with ACP `session/cancel`, sent while the
   `session/prompt` it interrupts is still in flight. The response then ends
   the turn as any other: the text so far stored, and `stop` with
   `stop_reason: cancelled`. Between turns there is nothing to cancel, and
   the runtime refuses. A cancel may name the launch it was decided on, and
   is then refused too where the session has been relaunched since. Each
   launch also reports its turns as they go — a tool call ended, by the name
   its `post_tool_use` carries, and the turn ended — to followers of its own,
   each on an unbounded channel so that no report is ever dropped, followed
   by session and launch and refused for a launch that is not the one
   running, so a report of the process before never passes for the current
   one. The turn an author asked for its review in is ended that way
   (004): its own launch's report of the review call ended, then the cancel,
   which never lands on the next launch's briefing.
9. The ACP permission mode is the repository's (002), read at launch: a
   task's sessions take its repository's, an orchestrator its goal's first
   repository's, and a loose session (020) the registered repository its
   directory lies deepest in — `auto` when it lies in none. `auto` selects the
   first allowing option, then the first option, and cancels only an empty
   list. `ask` records the request in the console, raises session attention
   and blocks until console input selects an option. `learn` does the same
   for a request that no allowing row answers. In `learn` and `ai`, a console
   question offers `once` (Allow once), `command` (Allow this command),
   `family` (Allow every `<family>` call), then `reject` (Reject). It offers
   the allow choices only if the agent offers an allowing option, and Reject
   only if the agent offers a rejecting option. The request keeps the agent's
   options as `agent_options`; `ask` keeps them as `options`. A console answer
   selects the agent's first `allow_once`, else first `allow_always`, or its
   first `reject_once`, else its first `reject_always` when that is its only
   rejecting kind. The tool name is
   `toolCall.name`, else `toolCall._meta.claudeCode.toolName`, else
   `toolCall.title`, else `toolCall.toolCallId`.
   The table `learned_permissions` keeps one row per repository, tool name,
   level and key (`learned_key::normalize`). The key is made from
   `rawInput` in four steps:
   - The fields per tool. `Bash` keeps `command`. `Edit`, `Write`, `Read`,
     `MultiEdit` and `NotebookEdit` keep `file_path`. An `mcp__` tool keeps every field but
     `body`, `summary`, `description`, `title` and `reason`. `WebFetch` keeps
     the host of `url` as `{"host": …}`. `WebSearch` keeps no field. Every
     other tool keeps every field but `description`.
   - The Bash command. A leading `cd <path>` followed by `&&`, `;` or a
     newline goes, where the path is the worktree, the repository or a
     directory under one of them; a relative path without `..` counts as
     under the worktree. Every `2>&1` goes. A trailing pipe chain goes where
     every program in it is `tail`, `head`, `wc`, `cat`, `sort` or `uniq`
     and none of it redirects or substitutes.
   - The placeholders, in every string value. The repository path becomes
     `<REPO>`, the worktree `<WORKTREE>` and the session's branch
     `<BRANCH>`, the longest first, then the home directory `<HOME>`. Then
     a `/tmp/…` or `/private/tmp/…` path becomes `<TMP>`, a whole word of 7
     to 64 hex characters in either case, with a letter and a digit,
     `<HASH>`, and an Ariadne id (`0` and 25 of `[0-9a-z]`, a whole word)
     `<ID>`. Numbers stay. An
     orchestrator and a loose session have no branch. An author's branch is
     its own, the task branch with a suffix for a second author. A
     reviewer's is the branch it reviews, the task branch or one author's,
     and it changes when a live reviewer is moved to the next author's
     review. Another author's branch stays as it is.
   - The key is the compact JSON of the kept fields with sorted keys:
     `git rebase main 2>&1 | tail -40` with a `description` is
     `{"command":"git rebase main"}`, and a `merge_commit` of any SHA is
     `{"merge_commit":"<HASH>"}`.
   The family of a `Bash` request is the program of the first simple command
   of the normalized command, after `NAME=value` assignments and the wrappers
   `sudo`, `env`, `time`, `nohup`, `timeout`, `nice`, `command` and `exec`,
   with their options, the value of an option that takes one (`time -f`,
   `-o`), and the duration of `timeout`. For `git`, `cargo`, `npm`, `npx`,
   `pnpm`, `yarn`, `go`, `docker`, `gh`, `make`, `kubectl`, `pip`, `pip3`
   and `brew`, the
   first argument that does not start with `-` follows, after the global
   options of `git`: `git push`, `cargo nextest`, `ls`. The family of every
   other tool is the tool name.
   The level is `once`, `command` or `family`, and the scope `repository` or
   `all`. Each row keeps the key, the level, the family, the request's
   derived risk tags (022) in `derive` order, the scope, the whole `toolCall`
   with its `rawInput` keys sorted, the `options`, the selected option, the
   `target` and the model `output`. `target` is the permission mode at the
   time of the decision. A console choice in `learn` or `ai` writes a row at
   level `once`, `command`, `family`, or `once` for the four choices in that
   order. A `family` row uses its family as its key; every other row uses the
   normalized key. A console choice in `ask` writes at level `once`. All use
   scope `repository`. A
   model deny, a rule deny and a cap deny write it too. A model allow, a
   learned allow and an `auto` allow write nothing. A write on an existing
   key keeps its id and `created_at`, and replaces the rest, the scope
   included. In `learn` and `ai` only, an allowing `command` row for the
   normalized key answers first, then an allowing `family` row for the family.
   At each level, the repository's own row takes precedence where one exists;
   otherwise a row of scope `all` from any repository can answer when its
   tool name, level and key match. The selected agent option must have kind
   `allow_once` or `allow_always`. At both levels, every derived risk tag of
   the request must be among the row's tags. A row is written with scope
   `repository`, and a person widens it to `all`, or narrows it back, with
   `PUT /v1/permissions/learned/{id}` (012, rule 26), which keeps its key,
   its level and everything else about it. A `once` row never answers a later
   request. A denied row
   never auto-allows and never auto-denies. A request with no `rawInput`
   never auto-allows, and a session outside every repository records and
   answers nothing. The AI permission model reads the raw input, not the key.
   In `ai`, the AI permission model decides first (022), then `learn`
   handles every answer that is not a confident allow or deny.
   The answering row's id, level and key appear on `permission_request` and
   `permission.replied`; all three are null if no row answered.
10. After a turn ends the agent stays up and the runtime keeps serving it.
   Everything the daemon says to the agent after the launch — a scheduler
   nudge, a review briefing, an agent message — is a `session/prompt`, sent
   at once between turns and queued in order behind a running one. Console
   input (008) arrives the same way, except that a pending permission question takes it as
   the answer (rule 9): only a person's input selects a console choice.
   A prompt that carries an agent message is claimed right before it goes
   out, and skipped when the claim fails: a read took the message first
   (018). A message already in the queue is not queued again. The runtime
   watches the agent's stdin for each `session/prompt` line written whole. A
   claimed prompt that was never written gives its claim back when its launch
   ends. A written one keeps it, an error answer included.
11. The child is reaped whenever it ends. Its own exit ends the session on
   the record: `session.error` first if the protocol failed, then
   `session_end`. Killing the session kills the child and retires the row;
   the kill does not wait for the reap. Killing the child, and reaping it,
   sends the kill to its whole process group: an adapter that runs the agent
   in a process of its own — codex-acp's `codex app-server` — leaves no
   process behind that still writes the conversation.
   A kill that finds a turn running first sends `session/cancel`, starts no
   queued prompt, and waits up to five seconds for the turn's response, whose
   `stop` records what the turn spent; a turn waiting on a permission answer
   is not cancelled, and an agent that does not answer is killed when the
   wait runs out.
12. A resume starts a fresh agent process on the stored conversation. The
   predecessor is reaped, and its exit takes neither the seat nor the row
   down.
13. Liveness is the runtime's own registry of running agents, and it always
    answers. After a daemon restart no child of the old daemon is running,
    and a revive reaches the stored conversation through a new process.
14. A failed prompt is exhausted when `data.codexErrorInfo` is
    `usageLimitExceeded`, when `_meta.jetbrains.air.sessionFailure.category`
    in its error or response is `limit`, or when its message contains one
    configured exhausted pattern without regard to case. `session.error`
    preserves the JSON-RPC code and message, and adds `exhausted: true`
    plus the structured value or matching pattern as `exhausted_reason`.
    For every ACP agent, an error with data and no non-empty string
    `data.message` adds one line there. It takes a data string first, then
    the first non-empty string at `details`, `detail`, `error`, `reason`,
    `description`, or `stderr`, including a string one object level below
    those keys. Otherwise it takes compact JSON, cut to 200 characters.
    It puts the ACP message before the detail unless they are equal. It
    keeps every other data key. For data that is not an object, it also
    keeps the original value in `data.details`. An error without data and
    one with a non-empty string `data.message` stay unchanged.

## Acceptance criteria

- An OpenCode probe closes and deletes the session id returned by
  `session/new` (`acp_discovery.rs::an_opencode_probe_closes_and_deletes_its_catalog_session`).
- A failed OpenCode delete keeps the agent ready with its catalog
  (`acp_discovery.rs::a_failed_opencode_delete_keeps_the_agent_ready_with_its_catalog`).
- A non-OpenCode probe does not run session delete
  (`acp_discovery.rs::a_non_opencode_probe_runs_no_session_delete`).
- An ACP error with `data.details` records the ACP message and detail in
  `session.error` while keeping the other fields
  (`acp_runtime.rs::an_acp_error_reports_its_details_in_the_event_message`).
- The event summary and the transcript used by `ariadne session logs` show
  the combined line
  (`acp_runtime.rs::an_acp_error_detail_reaches_the_event_summary_and_session_logs`).
- A string `data.message` stays unchanged, while string and other JSON data
  supply readable error messages
  (`acp_runtime.rs::an_acp_error_keeps_an_existing_message_and_reads_other_data_shapes`).
- Common detail keys, nested errors, strings, and compact JSON all supply
  readable messages for any ACP agent. Long JSON is cut
  (`acp_runtime.rs::acp_errors_read_common_detail_fields_for_every_agent`).
- An error without data stays unchanged, and an empty `data.message` takes
  the reason from another field
  (`acp_runtime.rs::an_acp_error_without_data_stays_unchanged_and_an_empty_message_is_filled`).
- A detail equal to the ACP message appears only once
  (`acp_runtime.rs::an_acp_error_does_not_repeat_its_message_as_the_detail`).
- `learn` publishes four choices and preserves the agent choices, records a
  family answer, reuses it for another call in that family, and asks about a
  different family
  (`acp_console.rs::family_choice_answers_later_rebase_calls_but_not_other_families`).
- A `git push` family row with `remote` does not answer a force push with the
  extra `force` tag (`acp_console.rs::a_family_row_keeps_the_force_tag_guard`).
- `once` and `reject` record only a one-time row and ask again
  (`acp_console.rs::once_and_reject_choices_record_once_and_ask_again`).
- A console Reject choice remains available when the agent offers only
  `reject_always` (`ai_permissions_decisions.rs::a_console_reject_uses_the_only_agent_rejection_option`).
- `ask` keeps the agent's choices and records a one-time row
  (`acp_console.rs::ask_records_the_console_choice_and_never_auto_allows`).
- An author runs end to end — the handshake in order, the worktree, the
  `ariadne` MCP server, the model and effort pins, the briefing behind the
  system prompt as the first prompt, the events in the store, the captured
  agent session id, and the agent still up after the turn
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio`).
- An orchestrator seat runs the same way, with the ariadne MCP server in
  `session/new` (`acp_runtime.rs::an_orchestrator_runs_on_the_registry_agent`).
- Codex, Claude, and configured message signals classify exhausted prompts,
  while another error remains plain. The stub preserves configured error data
  (`auto_switch.rs::a_codex_exhaustion_switches_to_another_agent_at_the_same_rank`,
  `auto_switch.rs::claude_and_message_signals_classify_while_a_plain_error_does_not`).
- A reviewer seat runs the same way, in its detached worktree
  (`acp_runtime.rs::a_reviewer_runs_on_the_registry_agent`).
- Killing the session kills the agent process and retires the row
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`), and
  kills the processes the agent started
  (`acp_runtime.rs::killing_an_acp_session_kills_the_processes_its_agent_started`).
- A kill mid-turn cancels the turn and keeps what it spent
  (`acp_console.rs::a_turn_killed_mid_way_is_cancelled_and_keeps_what_it_spent`),
  a relaunch over a running turn starts once the old agent is reaped and
  keeps what the old launch spent
  (`::a_relaunch_over_a_running_turn_keeps_what_the_old_launch_spent`), a
  relaunch after a kill resumes once the killed agent and its writer are gone
  (`::a_relaunch_after_a_kill_resumes_once_the_killed_agent_is_gone`), and
  an agent that ignores the cancel is killed when the wait runs out
  (`::an_agent_that_ignores_the_cancel_is_killed_when_the_grace_runs_out`).
- An agent that dies mid-turn is reaped, and its session ends on the record
  (`acp_runtime.rs::a_dead_acp_agent_is_reaped_and_its_session_retired`).
- A resume loads the stored conversation on a fresh process, the instruction
  rides the new prompt, and the predecessor's exit takes nothing down
  (`acp_runtime.rs::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
- After a daemon restart a revive reaches the stored conversation through
  `session/load`, on an agent that advertises no `session/resume`
  (`acp_runtime.rs::a_stub_session_resumes_through_session_load_after_a_daemon_restart`).
- `auto` approves a permission request with the allowing option, and the ask
  and answer are events
  (`acp_runtime.rs::auto_approves_a_permission_request_with_the_allowing_option`),
  and the allowing option is selected wherever it stands
  (`acp.rs::the_allowing_option_is_selected_wherever_it_stands`).
- A repository set to `ask` raises session attention and console input
  unblocks the turn
  (`acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`).
- A repository set to `learn` records a console denial, asks again after it,
  replaces the row with the approval that follows, and auto-allows from that
  row across a daemon restart
  (`acp_console.rs::learn_remembers_an_approval_per_repository_across_a_daemon_restart`).
- A console choice in `ask` writes a row with `target = ask` and the tool name
  from `toolCall.name`, and `ask` never auto-allows from a row
  (`acp_console.rs::ask_records_the_console_choice_and_never_auto_allows`).
- An allowed `Bash` row does not auto-allow another command
  (`acp_console.rs::an_allowed_row_does_not_allow_another_command_of_the_same_tool`).
- A request with no `rawInput` gets a row but never auto-allows
  (`acp_console.rs::a_request_without_raw_input_never_auto_allows`).
- `learn` answers a request that differs from an approved one only by a SHA,
  a `description` and a `2>&1 | tail -60`
  (`acp_console.rs::learn_answers_a_request_that_differs_only_by_one_time_values`),
  and a row whose risk tags lack one of the request's does not answer it
  (`::a_row_without_the_requests_risk_tags_does_not_answer_it`).
- The key keeps the fields of each tool, drops a `cd` into the worktree, a
  `2>&1` and an output filter, and replaces one-time values in order
  (`learned_key.rs::the_four_forms_of_one_rebase_give_one_key`,
  `::two_finish_task_inputs_with_different_shas_give_one_key`,
  `::two_edits_of_one_file_give_one_key`,
  `::two_verdicts_with_different_bodies_give_one_key`,
  `::a_cd_into_the_worktree_and_an_output_filter_are_dropped`,
  `::a_cd_elsewhere_and_a_writing_filter_are_kept`,
  `::the_worktree_the_repository_the_branch_and_the_home_are_replaced_in_order`,
  `::a_session_without_a_branch_keeps_the_branch_name`,
  `::one_time_values_are_placeholders_and_numbers_are_kept`,
  `::an_uppercase_hash_is_a_placeholder`,
  `::only_the_sessions_own_branch_is_the_branch_placeholder`,
  `acp_console.rs::a_second_authors_branch_is_the_branch_placeholder`,
  `::a_reviewer_of_a_second_author_names_the_reviewed_branch`,
  `multi_author_tasks.rs::a_live_reviewer_is_briefed_for_the_next_author_without_the_quiet_clock`,
  `::a_fetch_keys_on_its_host_and_a_search_on_nothing`), and the family is
  the program and its subcommand
  (`::the_family_is_the_program_and_its_subcommand`).
- Two writes with one repository, tool name, level and key keep one row: the
  second keeps the id and `created_at` and sets the scope, and the row
  carries the key, the level, the family, the tags and the scope
  (`store.rs::learned_permissions_keep_one_row_per_repository_tool_level_and_key`).
- A fresh database holds the table with its fifteen columns
  (`store.rs::a_fresh_database_holds_the_fifteen_learned_permission_columns`).
- A repository's own row wins over another repository's `all`-scoped row of
  the same tool name, level and key, and the `all`-scoped row answers for a
  repository that holds none of its own
  (`store.rs::a_repository_row_wins_over_an_all_row_of_another_repository`).
- A console choice in one repository is asked again in a second repository;
  widening that row to `all` with `PUT` then answers the second repository's
  matching request as `learned`, without attention
  (`acp_console.rs::widening_a_row_to_all_answers_a_second_repository`).
- A repository set to `ai` asks the AI permission model first. A confident
  allow proceeds; every other outcome uses a learned approval or asks the
  console
  (`ai_permissions_decisions.rs::a_confident_allow_runs_at_once_and_reports_ai`,
  `::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`,
  `::a_review_answer_waits_for_the_console`), and the model reads the raw
  input (`::the_model_receives_the_raw_input_and_not_the_learned_key`).
- Console input reaches the agent and queues behind a running turn
  (`acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`).
- An offered command reaches the agent without the system prompt; every other
  slash prompt keeps that prompt
  (`acp_console.rs::only_available_slash_commands_skip_the_system_prompt`).
- An unknown session update leaves the session running
  (`acp_runtime.rs::an_unknown_session_update_does_not_fail_the_session`).
- A scheduler nudge arrives at the agent as a `session/prompt`
  (`acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`).
- Message chunks reach the console stream before the turn ends
  (`acp_console.rs::a_console_stream_client_sees_message_chunks_before_the_turn_ends`),
  and so do thought chunks and a tool call's progress
  (`::thought_chunks_and_tool_call_progress_reach_the_console_stream_live`).
- Each run of text is stored once, whole, where the agent wrote it — before
  the plan, the tool call or the `stop` that ended it — and no chunk is
  (`acp_console.rs::the_snapshot_after_a_turn_holds_each_run_of_text_where_it_was_written`),
  each message the agent names is stored on its own
  (`acp_runtime.rs::each_message_the_agent_names_is_stored_on_its_own`),
  and an agent that dies mid-turn keeps the run it was writing
  (`acp_runtime.rs::an_agent_that_dies_mid_turn_keeps_the_text_it_was_writing`);
  neither `GET /v1/events` nor `/v1/events/stream` carries a chunk
  (`::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- `post_tool_use` stores its output once, keeps the opening input on
  `pre_tool_use`, and is smaller than its uncompact form
  (`acp_console.rs::post_tool_use_stores_text_once_and_keeps_the_opening_input`).
- `user_prompt_submit` carries the typed text and its source
  (`acp_console.rs::console_input_is_reported_as_its_text_from_the_console`).
- A cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), and is
  refused between turns (`::cancel_with_no_turn_running_is_refused`); an
  author's review request draws one such cancel once the agent reports the
  call ended, keeps what the turn spent, and leaves the agent up for the
  verdict's prompt
  (`::an_authors_review_request_ends_its_turn_and_the_verdict_still_reaches_it`),
  and a launch's turn reports are its own: the process before reports to
  nobody but itself
  (`::a_prior_launchs_late_review_report_does_not_end_the_new_launchs_turn`);
  a burst of reports before the review call's loses none of them, and the
  cancel still follows
  (`::a_burst_of_reports_before_the_review_calls_loses_none_and_the_cancel_follows`).
- Prompt usage keeps cache writes in input and counts only cache reads as
  cached input, prefers quota, adds up one
  launch's turns, adds a resumed launch, and leaves an absent report at zero
  (`acp_console.rs::standard_prompt_usage_adds_up_a_launchs_turns_and_rolls_up`,
  `::quota_prompt_usage_takes_precedence_over_standard_usage`,
  `::a_prompt_without_usage_keeps_zero_totals_and_records_stop`,
  `::resumed_prompt_usage_adds_a_new_launch_total`).
- Context updates keep a session's current used and size figures before its
  turn ends, and ignore a reported cost
  (`acp_console.rs::context_updates_keep_the_sessions_window_current_while_it_runs`).
- A transcript's figure wins over the prompt response, moves while a turn
  runs, and adds up across launches
  (`transcript_usage.rs::a_codex_session_stores_its_rollouts_total_not_its_prompt_response`,
  `::a_claude_session_counts_each_request_once_with_its_subagents`,
  `::a_running_turns_figure_moves_before_its_stop`,
  `::two_launches_of_one_codex_session_add_up`,
  `::a_session_without_a_transcript_keeps_its_prompt_responses_figure`).
- An option is found by its category, or by its id or name where no option
  has the category — the lookup the runtime shares with discovery
  (`acp_discovery.rs::model_and_effort_name_fallbacks_enter_the_discovered_catalog`).

## Known gap

Two refusals of rules 2 and 3 are proven only at discovery, not at launch:
an agent on another protocol version, and an agent with no model option.
Discovery rejects both (007), and no pin can name a model of a rejected
agent (011), so no test launches one.

## Sources

`crates/ariadne-daemon/src/acp.rs` (the runtime),
`crates/ariadne-daemon/src/acp_transport.rs` (the agent's pipes as the SDK's
transport), `crates/ariadne-daemon/src/acp_calls.rs` (the methods it calls),
`crates/ariadne-daemon/src/acp_schema.rs` (Ariadne's launch types as the
SDK's),
`crates/ariadne-daemon/src/transcript.rs` (the transcript reader),
`crates/ariadne-daemon/src/launcher.rs` (the launch, liveness and kill),
`crates/ariadne-daemon/src/learned_key.rs` (the learned key and family),
`crates/ariadne-store/src/permissions.rs` (the learned rows),
`crates/ariadne-daemon/src/scheduler/mod.rs` (the prompt delivery),
`crates/ariadne-daemon/tests/it/common/acp.rs` (the scriptable stub agent).

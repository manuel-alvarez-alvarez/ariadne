---
id: acp-runtime
status: current
updated: 2026-11-26
areas: [daemon]
commits: [8163676d, e078d730, c47870a8, b8cfd6bb, 21a4d426, 90f6a584, 354c0aaa, 04dd0fed, 47b8acdf, efb3e62a, 7b19d49a, b2c18f4f, 87db0081]
tests:
  - crates/ariadne-daemon/tests/it/acp_recovery.rs
  - crates/ariadne-daemon/tests/it/place.rs
  - crates/ariadne-daemon/tests/it/doctor.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/acp_items.rs
  - crates/ariadne-daemon/src/http/classify.rs
  - crates/ariadne-daemon/tests/it/subagents.rs
  - crates/ariadne-daemon/src/acp_subagents.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_discovery.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-daemon/tests/it/transcript_usage.rs
  - crates/ariadne-daemon/tests/it/auto_switch.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs
  - crates/ariadne-daemon/tests/it/switch.rs
  - crates/ariadne-daemon/tests/it/seat_brief.rs
  - crates/ariadne-daemon/src/learned_key.rs
  - crates/ariadne-store/tests/store.rs
---

# ACP runtime

The daemon drives each agent over ACP version 1.
The daemon owns a direct child or connects to a relay in a detached multiplexer pane.
The relay command and socket protocol follow [038](038-agent-relay.md).
Every seat uses the same event ingestion, console, permission handling, and usage accounting.

The wire uses the `agent-client-protocol` SDK and its typed v1 schema.
`acp_transport` adapts direct pipes and relay streams for the SDK.
The daemon owns direct process groups; the relay owns multiplexer process groups.

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
For OpenCode — the shipped registry ids `opencode` and `opencode-v2`, and
the legacy `opencode-acp` a configured agent may still carry — it then runs the
registry entry's program as `session delete <session id>` with the probe's
launch environment and a discovery timeout, on the session id the probe's
`session/new` returned, once, after the probe ends whether it accepted the
agent or rejected its catalog. A failed close or delete is logged as a
warning and does not reject the agent or discard its catalog. A catalog
read from the store rather than a fresh `session/new` opens no session, so
it leaves nothing to delete.

## Behavior

1. The agent runs at the place selected under rule 17. A direct child runs the registry command
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
3. The model is set through `session/set_config_option` on the option of
   category `model`, or, where no option has that category, on the option
   whose id or name is `model`. The effort is set the same way, on category
   `thought_level` with `effort`, `reasoning` and `thought_level` as the
   id-or-name fallback, and only when the launch or a same-agent switch carries one.
   The value the agent answers with is read back. A model pin settled on the
   same id without its trailing context hint (`claude-fable-5-1` for
   `claude-fable-5-1[1m]`) has landed, if the option no longer offers the
   hinted id: Claude's agent lists the model that way on a resumed
   conversation (`acp_runtime.rs::a_pin_settled_on_its_id_without_the_context_hint_lands`).
   A launch pin that does not land — no matching option, a refused or failed
   call, or another value settled on — does not stop the launch: the session
   runs on whatever the agent runs, its own default or what the resumed
   conversation ran on, and a `session.pin_fallback` event names the pin, the
   value the agent runs instead and the reason. The row keeps the pin, so the
   next launch asks for it again
   (`::a_refused_model_pin_runs_on_the_agents_own_model`). Where the hinted
   id is still offered beside the bare one, the bare one is a smaller context
   window, and a pin settled there falls back the same way
   (`::a_pin_settled_on_the_bare_id_beside_the_hinted_one_falls_back`).
   A same-agent switch also calls `session/set_config_option` on the live
   conversation, after its running turn and before queued prompts
   (`switch.rs::a_same_agent_switch_keeps_the_row_and_conversation`,
   `::a_same_agent_switch_during_a_turn_precedes_queued_input`). A switch
   pin must land: a refused option fails a switch between turns with the
   agent's reason, and the session keeps its old pin
   (`::a_refused_same_agent_option_keeps_the_old_pin`).
4. A prompt the runtime sends is its own text. The first prompt of a launch
   and the first after a model switch carry the system prompt and a blank
   line ahead of that text (006 rule 2). A command never carries it: where text from any
   source starts with `/` and its first word, without `/`, equals the `name`
   of a command the agent last offered, the runtime sends the text alone,
   and the brief waits for the next turn. The first prompt of a launch is
   the one its launch file carries, if any. `user_prompt_submit`
   carries the whole as `prompt`, the text alone as `text`, the whole as its
   ACP content blocks in `content` (rule 6), a new `item_id`, and a `source`:
   `console` for console input (008), `daemon` for everything the daemon
   itself says. A daemon prompt that is a lifecycle kind, such as a resume
   nudge, also carries its `prompt_kind` (`agent_resume`,
   `orchestrator_resume`); the others carry none.
   The handoff of a model switch (006 rules 14–15) leaves out the session's
   first daemon prompt, which the new session reads in full in its own
   briefing
   (`handoff.rs::the_first_daemon_briefing_is_left_out_and_a_later_daemon_prompt_stays`),
   unless that prompt holds the handoff of an earlier switch, as the first
   prompt of a switched session does: that prompt stays whole, past the
   daemon prompt fold
   (`handoff.rs::the_first_prompt_of_a_switched_session_stays_with_its_earlier_history`);
   every prompt whose `prompt_kind` is `agent_resume` or
   `orchestrator_resume`, never found by its text
   (`handoff.rs::resume_nudges_are_left_out_by_their_prompt_kind`); and
   every `session.error` but the last, which it cuts to its first line
   (`handoff.rs::only_the_last_error_stays_and_it_is_cut_to_its_first_line`).
5. What the agent does becomes agent events on the ingestion path (012):
   `session_start` with the agent's own session id, `user_prompt_submit`,
   tool calls as `pre_tool_use`, `tool_call_update`, `post_tool_use` and
   `tool_call_abandoned` (rule 7), `plan` with the entries
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
   `agent_thought` or `agent_message` `{session_id, item_id, message_id,
   content, text}` — before whatever ended it. `content` holds each ACP
   content block of the run, in order. A plain text chunk, with no field but
   `type` and `text`, goes on with the plain text block before it, so a
   streamed text is one block. `text` is the run's text alone. `message_id`
   is the run's `messageId`, and is absent where the agent sent none. A
   content block larger than 1 MB is stored as a placeholder
   `{type, mime_type, size, omitted: true}`. Its size is the bytes of its
   data, its text or its embedded resource, else of the block as compact JSON
   (`acp_items.rs::a_stored_message_keeps_each_content_block_in_order_and_its_message_id`,
   `::a_content_block_over_one_megabyte_is_stored_as_a_placeholder`). A
   replayed `user_message_chunk` run is stored the same way, as
   `user_prompt_submit`. A turn that speaks around a tool call stores its
   text before the call and its text after it as two events, and the last
   run comes before `stop`; a turn that fails stores its last run before the
   error. Text that arrives between turns is not stored. While the
   turn runs, each chunk goes live to the session's console stream alone
   (008) — `agent_message_chunk` and `agent_thought_chunk`, each
   `{session_id, item_id, text}`, with the `item_id` that the run's stored
   whole carries
   (`acp_items.rs::each_live_chunk_and_its_stored_whole_carry_one_item_id`).
   No chunk is stored, and none reaches `/v1/events` or `/v1/events/stream`.
7. Tool call updates are merged per `toolCallId`: every field an update sets
   replaces the one on record, `content` included. `pre_tool_use` carries
   the call as it opened, including its input. The daemon stores a
   non-terminal update of a running call as `tool_call_update`
   `{session_id, tool_call_id, acp}`, with the call merged so far, when
   `Timeouts::tool_progress` (5 seconds) has passed since the call's last
   stored event: its `pre_tool_use` or its last stored `tool_call_update`.
   `GET /v1/events` lists it, and the console reads it from the store
   (`acp_items.rs::a_running_calls_progress_is_stored_once_its_interval_has_passed`).
   Inside that interval the update goes live to the console stream alone, in
   the same shape, and is not stored
   (`::a_running_calls_progress_inside_its_interval_is_not_stored`). So a
   call stores at most one update in each interval, and a call that ends
   within its first interval stores none. `post_tool_use`, on a `completed`
   or `failed` update, carries `tool_name`, `tool_input` and the merged call
   under `acp`, `rawOutput` included. `acp` drops only `rawInput`, which
   `tool_input` holds
   (`acp_console.rs::post_tool_use_keeps_the_input_and_the_raw_output`).
   When a turn ends with calls still open, the daemon stores one
   `tool_call_abandoned` `{session_id, tool_call_id, reason, acp}` for each,
   in the order they opened, after the last run and before the `stop`.
   `reason` is `cancelled` where the turn's stop reason is `cancelled`, and
   `turn_ended` otherwise
   (`acp_items.rs::a_turn_that_ends_with_open_calls_abandons_each`,
   `::a_cancelled_turn_abandons_its_open_call_as_cancelled`). A call still
   open when the agent ends — after a failed turn, a death or a kill whose
   grace ran out — is stored as `session_ended`, before the `session.error`
   and the `session_end`
   (`::an_agent_that_ends_mid_call_abandons_it_as_session_ended`). A console
   snapshot taken while a turn runs carries each open call, merged so far,
   as a live `tool_call_update` after the stored events
   (`::a_console_opened_during_a_call_reads_the_open_call`). Neither
   `tool_call_update` nor `tool_call_abandoned` moves the session's status
   or attention
   (`classify.rs::tests::stored_progress_and_an_abandoned_call_move_nothing`).
   The metrics read neither, and neither is an ended tool: only
   `post_tool_use` reports `TurnReport::ToolEnded`. A child's calls carry
   its `subagent` field on both (rule 15).
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
   one.
   A cancel answers every open permission request `cancelled`, and an agent
   reads that as the user refusing the call. So the first daemon prompt
   after a turn that ended `cancelled` starts with the cancelled-turn note
   (006 rule 8): the calls were stopped, and nobody refused them. A prompt
   typed at the console and a slash command carry no note.
9. The ACP permission mode is the repository's (002), read at launch: a
   task's sessions take its repository's, an orchestrator its goal's first
   repository's, and a PR session its request's repository's. `auto` selects the
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
     every program in it is `tail`, `head`, `wc`, `cat`, `sort` or `uniq`,
     and every argument after the program is a value-free flag or an accepted
     count. `tail` and `head` accept `-40`, or a number immediately after
     `-n`, `-c`, `--lines` or `--bytes`. A pipe remains where a filter
     redirects, substitutes, names a file or a program (`-o`, `--output`,
     `--compress-program`, `-T` or `--files0-from`), has a bare operand or
     has an option with `=`.
   - The placeholders, in every string value. The repository path becomes
     `<REPO>`, the worktree `<WORKTREE>` and the session's branch
     `<BRANCH>`, the longest first, then the home directory `<HOME>`. Then
     a `/tmp/…` or `/private/tmp/…` path becomes `<TMP>`, a whole word of 7
     to 64 hex characters in either case, with a letter and a digit,
     `<HASH>`, and an Ariadne id (`0` and 25 of `[0-9a-z]`, a whole word)
     `<ID>`. Numbers stay. An
     orchestrator has no branch. A column agent's
     branch is its task's, shared by every column of the task (030).
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
   other tool is the tool name. A family row for `Bash` answers only one
   simple command and its stripped output-filter tail. It does not answer a
   command with `&&`, `||`, `;`, a newline, command substitution, or a pipe
   that the key did not strip. It does not answer a command with an unquoted
   `>` or `<`, a `NAME=value` assignment before its program, including one
   `env` takes, or an option that selects a program or an output file. Only
   these programs can use a Bash family row; every other program asks again:
   - Git rejects `-c`, `--config-env` and `--exec-path` before its subcommand.
     It rejects `-O` and `--open-files-in-pager` for `grep`, `--output` for
     `diff`, `log` and `format-patch`, `-o` for `format-patch`, `--upload-pack`,
     `--receive-pack` and `-u` for `fetch`, `clone` and `ls-remote`,
     `--receive-pack` for `push`, `-x` and `--exec` for `rebase`, and `-x` and
     `--extcmd` for `difftool`.
   - Cargo rejects `--config`; Go rejects `-exec` and `-toolexec`; Make rejects
     `NAME=value`, `--eval`, `-f`, `--file` and `--makefile`; npm, pnpm and Yarn
     reject `--script-shell` and `--node-options`.
   - Find rejects `-exec`, `-execdir`, `-ok`, `-okdir`, `-delete`, `-fprint`,
     `-fprint0`, `-fprintf` and `-fls`. Ripgrep rejects `--pre`. Sort rejects
     `-o`, `--output`, `--compress-program`, `-T` and `--files0-from`.
   A listed one-letter short option is rejected in a one-hyphen option bundle
   or with an attached value. A long option is rejected as its full name, or as
   a prefix of at least three letters, with or without `=value`. Go rejects its
   two options with either one or two hyphens.
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
   never auto-allows. The AI permission model reads the raw input, not the key.
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
    in its error or response is `limit`, when `data.errorKind` is
    `rate_limit`, or when its message or the detail line of its data
    contains one configured exhausted pattern without regard to case. `session.error`
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
15. The runtime understands subagents: `initialize` advertises
    `clientCapabilities.subagents: {}`, read and written as raw JSON, since
    the SDK keeps the draft behind its `unstable_subagents` feature. An agent
    announces a child on its own session and sends the child's updates under
    the child's session id. Three shapes announce one: the draft protocol's
    `subagent_update` (the child is `sessionId`; `title`, `description`,
    `capabilities` and `state` are patches, and an omitted one keeps its
    value), and the adapters' `subagent_spawned` (`subagentSessionId`,
    `name`, `task`, `capabilities`) and `subagent_state_update`
    (`subagentSessionId`, `state`), which codex-acp and claude-agent-acp
    send. Each folds into one stored event, `subagent_update`, on the
    parent session: `session_id` (the session it came on), `subagent {id,
    title, description}`, `state` and `capabilities {cancel}`. `state` is
    `running`, `completed`, `failed`, `cancelled` or null: a draft idle
    state is `cancelled` on a `cancelled` stop reason, `failed` on a
    `refusal` or an `error`, and `completed` otherwise; a draft
    `requires_action` is `running`; an adapter's `disconnected` is
    `failed`; anything else is null. `cancel` is true only where the child's
    capabilities hold a `cancel` object. The first stored update for a
    child id is its announcement. An update under an announced child's
    session id is stored on the parent session under its own kind, with a
    `subagent {id, title}` field; so is the live chunk or tool update it
    sends, and a permission request the child asks. A child's text is a run
    of its own (rule 6), and its tool calls are merged apart from the
    parent's (rule 7). A child's `usage_update` is dropped: the window is
    the session's own agent's. A child's `available_commands_update` goes
    live to the console as `available_commands_update` with the `subagent`
    field, is never stored (rule 5), and never replaces the session's own
    list. An update under any other session id is
    the parent's own, and an unknown kind is still let by. A child's event
    never sets the row's agent session id.
16. A child the agent announced may be cancelled by its own session id,
    independently of the parent's own turn: ACP `session/cancel`, sent with
    the child's id rather than read off the parent's running turn (008
    rule 36). It is refused where `subagent_id` names a child the agent
    never announced, where the registry's latest update for it is
    `completed`, `failed` or `cancelled`, or where that update's
    capabilities never held a `cancel` object. The registry a cancel
    reads is the same one rule 15 folds updates into, so a later
    announcement's capabilities or state apply at once. The parent's own
    turn, running or not, is untouched either way.

17. At every first launch and resume, select herdr, then tmux, then a direct child.
    Herdr is found when its executable is on the daemon's search path and its server answers a ping.
    A herdr server that does not answer counts as not found, so automatic selection uses tmux or a direct child.
    Every wait on herdr or tmux lasts the probe time of the running daemon's timeouts, 5 seconds by default.
    `session_place` can force `none`, `tmux`, or `herdr`.
    A missing forced multiplexer fails the launch or resume with its name and reason.
    Record the selected place on the session row before starting its process.
    A resume selects again; it does not preserve the previous place.
    Before starting that process, stop any old relay or pane for the session.
    An automatic resume can fall back when the previous multiplexer is unavailable.
    Reconnect preserves an existing relay's stored place and never runs selection again.
    `ariadne doctor` reports selection and its reason, including an unavailable forced place.
18. With tmux selected, start `ariadned relay` in a detached pane on a private server for the daemon home.
    Pass the working directory, command arguments, and environment without shell interpretation.
    Send ACP through the relay socket, never pane keystrokes or pane text.
    Pane discovery works in UTF-8 and non-UTF-8 daemon locales.
    Pane discovery names its tmux targets and needs no current client or session.
    It returns no panes when no server runs, or when the server has no relay session.
    It finds a pane directly after that pane starts.
    A tmux error other than a missing server or session fails with tmux's reason.
    A server that closes with its last session is asked again, and the listing logs one warning when the server exited during it, in the session listing or the pane listing, never two for one listing.
    A server that exits during the pane listing yields no panes, including none collected before.
    A server that keeps failing is an error.
    A daemon stop closes its connection without stopping the relay, pane, or agent.
    A session kill stops the relay's agent process group before removing its pane.

19. A failed session reconnect logs its reason and leaves the row for the liveness sweep (009, rule 20).
    Stale sockets and missing, unreadable, or mismatched launch metadata do not abort daemon startup.
    Other sessions continue reconnecting after that failure.
    Persist numbered daemon inputs before sending them to the relay.
    On connect, resend only numbers above the relay greeting's accepted input number.
    Store each output frame before acknowledging it to the relay.
    Reconstruct ACP setup from retained responses without initializing the live agent again.
    Restore the pending prompt, buffered text, permission state, and completed usage.
    Restore subagent registrations and available commands from earlier turns.
    Keep the active turn's identity and delivery metadata across reconnect.
    Preserve the launch's original transcript baseline when reading usage after reconnect.
    Retain processed frames to reconstruct buffered text during replay.
    Identify normalized effects by session, launch, source, and ordinal.
    Replaying an effect keeps its existing event identity and usage source.
    Mark a permission request processed only after its answer reaches the durable input ledger.
20. With herdr selected, start `ariadned relay` in a herdr pane through the herdr socket API.
    The socket is `HERDR_SOCKET_PATH`, then the session socket for `HERDR_SESSION`, then `~/.config/herdr/herdr.sock`.
    Each request is one line of JSON on a new connection, and each answer is one line.
    Start a pane with `workspace.create`, then `layout.apply` on the new tab with the relay as the pane's argv command.
    Name only the tab, never both the tab and its workspace.
    Pass the working directory and environment as fields, without shell interpretation.
    Label the pane with the launch, the session and a tag of the daemon home.
    List panes with `pane.list`, and keep only the panes whose label carries this home's tag.
    Stop a pane after its relay with `pane.close`.
    A daemon stop, a new daemon's reconnect and a kill follow rule 18, with herdr in place of tmux.
    The herdr agent-state detection is not used.

## Acceptance criteria

- Selection order, direct fallback, and forced failures:
  `place.rs::automatic_selection_uses_tmux_before_a_direct_child`,
  `::automatic_selection_uses_a_direct_child_without_multiplexers`, and
  `::a_missing_forced_multiplexer_refuses_launch`.
- Herdr is selected before tmux, and a server that does not answer is not found:
  `place.rs::automatic_selection_uses_herdr_before_tmux` and
  `::a_herdr_server_that_does_not_answer_is_not_found`.
- Doctor reports herdr as the selected place with its reason:
  `place.rs::doctor_reports_herdr_as_the_selected_place_with_its_reason` and
  `doctor.rs::doctor_reports_herdr_as_the_selected_place_with_its_reason`.
- A herdr pane starts through `workspace.create` and `layout.apply`, lists by home, and stops through `pane.close`:
  `place.rs::a_herdr_relay_pane_can_be_found_and_stopped_through_the_socket_api`.
- A herdr session runs ACP through its relay, and a kill stops the agent, relay and pane:
  `acp_runtime.rs::a_herdr_session_runs_acp_through_its_relay_and_a_kill_stops_its_pane`.
- A daemon stop leaves the herdr pane, and a new daemon reconnects with the row live:
  `acp_runtime.rs::a_daemon_stop_leaves_the_herdr_pane_and_a_new_daemon_reconnects_to_it`.
- A forced herdr with no herdr fails the launch and the doctor report with a reason:
  `acp_runtime.rs::a_missing_forced_place_refuses_a_resume_with_its_reason` and
  `doctor.rs::doctor_reports_a_missing_forced_place_and_its_reason`.
- Forced settings parse through daemon configuration:
  `config.rs::tests::the_daemon_accepts_each_forced_session_place`.
- Doctor explains a missing forced location:
  `doctor.rs::doctor_reports_a_missing_forced_place_and_its_reason`.
- A detached pane starts, lists, and stops:
  `place.rs::a_detached_relay_pane_can_be_found_and_stopped`.
- Pane discovery returns an empty list with no tmux server, and with a server that has no session:
  `place.rs::listing_panes_without_a_tmux_server_returns_an_empty_list` and
  `::listing_panes_on_a_server_without_a_session_returns_an_empty_list`.
- A pane listing directly after a pane starts finds it, and a tmux that cannot run fails the listing:
  `place.rs::a_pane_listing_directly_after_the_start_finds_the_pane` and
  `::a_tmux_that_cannot_run_fails_the_pane_listing`, which also covers the one warning when the server exits during the session listing, and
  `::a_server_that_exits_during_the_pane_listing_returns_no_panes_and_warns`.
- A server with no current target needs no pane query:
  `place.rs::a_tmux_server_without_a_current_target_has_no_live_panes`.
- Pane discovery also works with a non-UTF-8 tmux client locale:
  `place.rs::a_detached_relay_pane_is_discoverable_in_the_c_locale`.
- Arguments, environment, and working directory reach the agent without shell interpretation:
  `place.rs::a_relay_pane_preserves_arguments_environment_and_working_directory`.
- ACP setup, console input, and kill use the relay:
  `acp_runtime.rs::a_tmux_session_runs_acp_through_its_relay`.
- Resume selects again when the old multiplexer is unavailable, stops its old relay,
  and stores the new place:
  `acp_runtime.rs::an_automatic_resume_reselects_after_the_old_multiplexer_disappears`.
- A forced setting still applies to resume and reports an unavailable location:
  `acp_runtime.rs::a_missing_forced_place_refuses_a_resume_with_its_reason`.
- Permission handling through tmux preserves the selected option and event kinds:
  `acp_runtime.rs::a_tmux_permission_answer_reaches_the_agent_once`.
- Daemon disconnection preserves the session row and permits more ACP traffic to the same agent:
  `acp_runtime.rs::daemon_disconnection_leaves_the_pane_relay_and_agent_alive`.
- Reconnect precedes liveness and keeps the stored place despite changed selection:
  `acp_runtime.rs::reconnect_keeps_the_live_relay_place_when_the_forced_setting_changes`.
- A stale socket leaves its row to the sweep while another relay reconnects:
  `acp_runtime.rs::a_stale_relay_socket_does_not_block_other_reconnects_or_the_liveness_sweep`.
- A socket left behind without a tmux server also permits startup:
  `acp_runtime.rs::a_stale_relay_socket_without_a_tmux_server_is_left_for_the_liveness_sweep`.
- Invalid launch metadata also isolates the failed session:
  `acp_runtime.rs::missing_relay_launch_metadata_does_not_block_other_reconnects_or_the_liveness_sweep`,
  `::unreadable_relay_launch_metadata_does_not_block_other_reconnects_or_the_liveness_sweep`, and
  `::mismatched_relay_launch_metadata_does_not_block_other_reconnects_or_the_liveness_sweep`.
- Offline output and usage survive without repeating the prompt or an accepted permission answer:
  `acp_runtime.rs::reconnect_restores_offline_text_usage_and_an_answered_permission_without_repeating_input`.
- Repeated normalized effects keep one event and one usage record:
  `acp_recovery.rs::replaying_one_relay_effect_keeps_one_event_and_one_usage_record`.
- An unanswered permission returns to the console:
  `acp_runtime.rs::an_unanswered_permission_returns_to_the_console_after_reconnect`.
- Resume starts a new pane and retains the stored conversation:
  `acp_runtime.rs::a_tmux_resume_replaces_the_old_pane_and_keeps_the_conversation`.
- A missing pane metadata file does not prevent live relay recovery:
  `acp_runtime.rs::reconnect_finds_a_live_relay_without_the_pane_metadata_file`.
- Subagents from completed turns retain their names after reconnect:
  `acp_runtime.rs::reconnect_keeps_a_subagent_announced_in_an_earlier_turn`.
- The console retains the command list from completed turns:
  `acp_runtime.rs::reconnect_restores_commands_offered_in_a_completed_turn`.
- Turn identity preserves the question raised by the current turn:
  `acp_runtime.rs::reconnect_keeps_the_question_raised_by_the_current_turn_open`.
- Transcript usage retains the resumed launch's initial baseline:
  `transcript_usage.rs::reconnect_keeps_the_resumed_launchs_original_transcript_baseline`.
- Recovery distinguishes unanswered prompts, stored permission answers, and unprocessed prompt responses:
  `acp_recovery.rs::recovery_preserves_an_unanswered_turn_and_a_sent_permission_answer` and
  `::a_received_prompt_response_stays_pending_until_its_effects_are_stored`.
- The socket client restores unprocessed output and resends only unaccepted input numbers:
  `place.rs::reconnect_resends_only_unaccepted_inputs_and_restores_pending_output`.

- A model-read probe for `opencode`, the shipped id, closes and deletes the
  session id returned by `session/new`, once
  (`acp_discovery.rs::an_opencode_probe_closes_and_deletes_its_catalog_session`).
- The legacy `opencode-acp` id gets the same cleanup
  (`acp_discovery.rs::a_legacy_opencode_acp_probe_also_deletes_its_catalog_session`).
- The `opencode-v2` id gets the same close and delete, in that order
  (`acp_discovery.rs::an_opencode_v2_probe_also_closes_and_deletes_its_catalog_session`).
- A catalog rejected for want of a model option still deletes the session
  `session/new` returned
  (`acp_discovery.rs::a_rejected_opencode_catalog_still_deletes_its_session`).
- A failed OpenCode delete keeps the agent ready with its catalog, and does
  not reject it
  (`acp_discovery.rs::a_failed_opencode_delete_keeps_the_agent_ready_with_its_catalog`).
- A cached catalog, read at an earlier probe of the same agent version, opens
  no session, so a later probe runs no deletion
  (`acp_discovery.rs::a_cached_opencode_catalog_probe_creates_no_session_and_runs_no_deletion`).
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
  (`acp.rs::tests::common_error_detail_fields_and_long_json_are_readable`).
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
- A column's agent runs end to end — the handshake in order, the worktree, the
  `ariadne` MCP server, the model and effort pins, the briefing as the first
  prompt, the events in the store, the captured
  agent session id, and the agent still up after the turn
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio`).
- An orchestrator seat runs the same way, with the ariadne MCP server in
  `session/new` (`acp_runtime.rs::an_orchestrator_runs_on_the_registry_agent`).
- Codex, Claude, and configured message signals classify exhausted prompts,
  a pattern also in the detail of the error's data, while another error
  remains plain. The stub preserves configured error data
  (`auto_switch.rs::a_codex_exhaustion_switches_to_another_agent_at_the_same_rank`,
  `acp.rs::tests::exhaustion_signals_are_classified_and_a_plain_error_is_not`,
  `auto_switch.rs::a_plain_error_does_not_switch`).
- The review column's agent runs the same way, in the task's one worktree
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
  `2>&1` and an output filter with only inert flags and numbers, and replaces
  one-time values in order
  (`learned_key.rs::the_four_forms_of_one_rebase_give_one_key`,
  `::two_complete_step_inputs_with_different_shas_give_one_key`,
  `::two_edits_of_one_file_give_one_key`,
  `::two_step_returns_with_different_bodies_give_one_key`,
  `::a_cd_into_the_worktree_and_an_output_filter_are_dropped`,
  `::a_cd_elsewhere_and_a_writing_filter_are_kept`,
  `::the_worktree_the_repository_the_branch_and_the_home_are_replaced_in_order`,
  `::a_session_without_a_branch_keeps_the_branch_name`,
  `::one_time_values_are_placeholders_and_numbers_are_kept`,
  `::an_uppercase_hash_is_a_placeholder`,
  `::only_the_sessions_own_branch_is_the_branch_placeholder`,
  `::a_fetch_keys_on_its_host_and_a_search_on_nothing`,
  `::a_filter_with_an_output_or_program_option_stays_in_the_key`), and the
  family is the program and its subcommand
  (`::the_family_is_the_program_and_its_subcommand`,
  `::a_family_covers_one_command_and_safe_output_filters`).
- A family row asks for a chained Bash command, a redirect, unsafe program
  setup or an unlisted program rather than answering it
  (`acp_console.rs::a_family_row_does_not_answer_a_chained_command`).
- A command row answers `tail` and `head` output filters, but asks again for
  filters that can write a file or run a program
  (`acp_console.rs::a_command_row_answers_only_safe_output_filters`).
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
  `::a_deny_without_a_rejecting_option_waits_for_the_console`), and the model reads the raw
  input (`::the_model_receives_the_raw_input_and_not_the_learned_key`).
- Console input reaches the agent and queues behind a running turn
  (`acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`).
- An offered command, and every prompt after the first, reaches the agent as
  its own text alone
  (`acp_console.rs::only_available_slash_commands_skip_the_system_prompt`,
  `::console_input_is_reported_as_its_text_from_the_console`). The seat brief
  rides the first turn and the first after a model switch, on every agent
  (`seat_brief.rs::an_agent_gets_the_seat_brief_on_the_first_turn_and_after_a_switch`,
  `::a_claude_agent_gets_the_seat_brief_first_and_each_later_turn_word_for_word`).
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
  (`acp_console.rs::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- `post_tool_use` keeps the input as `tool_input` and the raw output, and
  stores the input once
  (`acp_console.rs::post_tool_use_keeps_the_input_and_the_raw_output`).
- A stored message, thought and prompt carry their content blocks in order,
  the message carries its `message_id`, and a block over 1 MB is a placeholder
  (`acp_items.rs::a_stored_message_keeps_each_content_block_in_order_and_its_message_id`,
  `::a_content_block_over_one_megabyte_is_stored_as_a_placeholder`).
- Each live chunk carries the `item_id` of its stored whole
  (`acp_items.rs::each_live_chunk_and_its_stored_whole_carry_one_item_id`).
- A running call's progress is stored once its interval has passed, and goes
  live alone inside it
  (`acp_items.rs::a_running_calls_progress_is_stored_once_its_interval_has_passed`,
  `::a_running_calls_progress_inside_its_interval_is_not_stored`).
- A call that a turn leaves open is abandoned as `turn_ended`, `cancelled` or
  `session_ended`, and the session still goes idle on the `stop`
  (`acp_items.rs::a_turn_that_ends_with_open_calls_abandons_each`,
  `::a_cancelled_turn_abandons_its_open_call_as_cancelled`,
  `::an_agent_that_ends_mid_call_abandons_it_as_session_ended`). Neither
  new kind moves the status or the attention
  (`classify.rs::tests::stored_progress_and_an_abandoned_call_move_nothing`).
- A console opened during a call reads the open call in its snapshot
  (`acp_items.rs::a_console_opened_during_a_call_reads_the_open_call`).
- `user_prompt_submit` carries the typed text and its source
  (`acp_console.rs::console_input_is_reported_as_its_text_from_the_console`).
- A cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), and is
  refused between turns (`::cancel_with_no_turn_running_is_refused`).
  The next daemon prompt after a cancelled turn starts with the
  cancelled-turn note, and the one after that does not
  (`acp_runtime.rs::the_prompt_after_a_cancelled_turn_says_nobody_refused_its_calls`).
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
- `initialize` advertises `subagents: {}`
  (`ariadne-daemon::acp::tests::initialize_tells_the_agent_what_the_daemon_supports`,
  `subagents.rs::a_codex_child_is_folded_onto_the_parent_session`).
- codex-acp's shape: the announcement and the end fold into
  `subagent_update`, and the child's text and tool calls are stored on the
  parent with the child named, its text a run apart from the parent's
  (`subagents.rs::a_codex_child_is_folded_onto_the_parent_session`).
- claude-agent-acp's shape: a child's permission request carries the
  child, a child announced on a child's session names that session, and a
  cancelled end folds as `cancelled`
  (`subagents.rs::a_claude_child_asks_for_permission_under_its_own_name`).
- The draft shape: a patch keeps what it omits, an idle state reads as how
  the work ended, and a `cancel` capability reads as true
  (`subagents.rs::a_draft_subagent_update_patches_what_it_names`,
  `ariadne-daemon::acp_subagents::tests::an_idle_child_reads_as_how_its_work_ended`).
- A later announcement's concrete `capabilities` object replaces the
  earlier one, even to clear a `cancel` the child had advertised
  (`ariadne-daemon::acp_subagents::tests::a_later_concrete_capabilities_object_replaces_an_earlier_cancel`).
- A child's command list reaches the console stream named, is not
  stored, and leaves the session's own list in the snapshot
  (`subagents.rs::a_childs_commands_reach_the_console_named_and_leave_the_parents_alone`).
- An agent that announces nothing stores no `subagent_update` and no
  `subagent` field, an update under an unknown session id is the parent's
  own, and an unknown kind is let by
  (`subagents.rs::an_agent_that_announces_nothing_stores_what_it_did_before`,
  `ariadne-daemon::acp_subagents::tests::only_an_announced_child_is_tagged`).
- Cancelling an announced, cancellable child sends `session/cancel` with
  the child's own id and leaves the session's own turn, already ended, as
  it is (`acp_console.rs::cancelling_an_announced_subagent_sends_session_cancel_with_its_own_id`).
- Cancelling an unknown child, one whose work already ended, or one that
  never advertised `cancel`, is refused and sends nothing
  (`acp_console.rs::cancelling_an_unknown_subagent_is_refused`,
  `::cancelling_an_ended_subagent_is_refused`,
  `::cancelling_a_subagent_that_does_not_advertise_cancel_is_refused`,
  `ariadne-daemon::acp_subagents::tests::a_child_may_be_cancelled_only_while_running_and_advertising_it`).

## Known gap

An agent on another protocol version (rule 2) is refused only at discovery,
not at launch: discovery rejects it (007), and no pin can name a model of a
rejected agent (011), so no test launches one. Discovery rejects an agent
with no model option the same way, so the fallback of rule 3 for a missing
option is proven only by the refused pin, which takes the same path.

## Sources

`crates/ariadne-daemon/src/acp.rs` (the runtime),
`crates/ariadne-daemon/src/http/classify.rs` (what each stored kind moves),
`crates/ariadne-daemon/src/timeouts.rs` (`tool_progress`),
`crates/ariadne-daemon/src/acp_transport.rs` (the agent's pipes as the SDK's
transport), `crates/ariadne-daemon/src/acp_calls.rs` (the methods it calls),
`crates/ariadne-daemon/src/acp_schema.rs` (Ariadne's launch types as the
SDK's), `crates/ariadne-daemon/src/acp_subagents.rs` (the subagent
updates),
`crates/ariadne-daemon/src/transcript.rs` (the transcript reader),
`crates/ariadne-daemon/src/launcher.rs` (the launch, liveness and kill),
`crates/ariadne-daemon/src/learned_key.rs` (the learned key and family),
`crates/ariadne-store/src/permissions.rs` (the learned rows),
`crates/ariadne-daemon/src/scheduler/mod.rs` (the prompt delivery),
`crates/ariadne-daemon/tests/it/common/acp.rs` (the scriptable stub agent).

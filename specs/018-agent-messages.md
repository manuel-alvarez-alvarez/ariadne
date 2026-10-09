---
id: agent-messages
status: current
updated: 2026-10-09
areas: [core, store, api, daemon, mcp, cli, ui]
commits: [1b09ac10]
tests:
  - crates/ariadne-daemon/tests/it/agent_messages.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/src/acp_transport.rs
---

# Agent messages

One channel for everything the agents say to each other, and one transport
that puts it in front of them.

## Scope

In: what a message is, who it can be for, how a message reaches the agent it
was sent to, and how long an agent stays around to receive one.

Out: how a column ends and the next begins (030), the states a step call
moves a task through (001), and the wording of the text a message arrives in
(006).

## Behavior

1. Everything one agent says to another is a **message**. There is one kind,
   `message`, and the daemon reads nothing off it: a review's outcome is a
   step call (030), not a message, so the channel holds what the agents said
   and nothing the daemon acts on.
2. A message has exactly one recipient: whether it has been seen is a
   question about one reader, and one row with several of them could not
   answer it.
3. A recipient is the orchestrator of the goal, or one staffed agent of a task
   named by the column it staffs, which `get_task` lists. An agent has no
   name, so the column is the address; the orchestrator needs none, since the
   goal has one. A loose session has no goal or staffed agent and is not a
   recipient (020).
4. One verb (`send_message`), for asking and for answering. There is no
   `answer` kind and no `reply`: an answer is a message to whoever asked,
   addressed the way the question was, so nothing threads and no row points
   at another.
5. A message asks or answers, and carries nothing else — no confirmation,
   no thanks, and nothing about what the sender is going to do next. Every
   message arrives at its recipient as a turn (8), so a courtesy costs the
   recipient a turn: that is what the tool's own description bans, and it is
   banned there once rather than in each seat's playbook (006).
6. A message is about one task, or about the goal itself. The goal's channel
   is the orchestrator's inbox, and everything said about a task is on that
   task. A message about a task carries the goal it belongs to as well, so
   every reader of a goal's channel — the transport and the API alike — asks
   for the goal's own messages rather than for every message of the goal:
   what two columns' agents say to each other is on their task, and is
   nobody else's to read.
7. One rule the daemon holds the channel to, and no more — it is a
   conversation, and the daemon is not in it: a recipient that the task does
   not staff is refused.
8. The daemon delivers a message by handing it to the recipient's ACP agent
   as a `session/prompt` (009, 021), so it arrives as a turn. There is no
   inbox to poll. A message to an agent mid-turn is queued by the runtime
   behind that turn. `delivered_at` says the agent has the text. It is
   stamped when the prompt goes out or when a read returns the message, and
   never when the prompt is queued. A queued message is unstamped, so a read
   returns it: an agent that waits for an answer inside one turn still gets
   it.
   `delivered_at` is the one gate on every way a message reaches an agent,
   and a message that carries it is never handed over again — across a
   resume of the recipient, a relaunch, or a restart of the daemon. There
   are two ways, and each one stamps what it hands over:
   - the prompt above. The driver claims the message right before it writes
     the `session/prompt`: one write that stamps the row only where it is
     unstamped. Where a read took the message first, the claim fails and the
     driver skips the prompt. A message is queued once for each launch,
     however many scheduler passes hand it. A prompt counts as written once
     the agent's stdin took its whole line. A prompt that was never written
     gives its claim back when its launch ends — a failed write, a closed
     connection or a kill — so the message reaches the next launch. A written
     prompt keeps the stamp, an error answer included: the agent can read it;
   - a default `read_messages` (013), which answers with the undelivered
     messages addressed to the calling session's own agent and claims them.
     It returns only the rows it claimed, so a message the driver claimed in
     the same instant is left out.
     The daemon narrows that read to the caller rather than to what the
     caller asked for: a read that stamps is a delivery, and no agent may
     spend another's. A delivering session must be of the channel's own goal:
     a stamp is spent once and cannot be given back, and the orchestrator is
     narrowed by its seat alone — every goal has one — so naming another
     goal's task would take delivery of that goal's orchestrator's messages.
     The task scope check cannot say so, since it exempts the orchestrator.
     `read_messages` with `all` reads the whole thread of the task or the
     goal instead, delivered messages included, and stamps nothing.
9. A message to the agent of a column that is not current waits: the agent
   is idle until its column comes round (030), and the transport hands the
   message to it then. Nothing about a message starts a session. Waking an
   agent is the lifecycle's business (009), and a message is not a reason to
   put one back on a task nobody is working on.
10. Every agent of a task stays up until the task is over. The agent of a
    column the task has left sits idle rather than being killed: the current
    column's agent may still have something it needs from it, and an agent
    that is gone can be told nothing.
11. What a column's agent hands the next column is not a message: the reason
    of its `complete_step` or `fail_step` call travels in the next agent's
    briefing (030). Where a step is judged and the judgment is text, it is a
    step reason; where it is a question, it is a message.
12. The MCP surface is two tools every seat has: `send_message` and
    `read_messages` (013).

## Acceptance criteria

- Agents write to each other without leaving the task and without the step
  moving (`agent_messages.rs::agents_write_to_each_other_without_leaving_the_task`).
- The message is handed to the recipient's agent as a prompt, names the
  sender by its seat, its agent id, the task it is of and its skills, says
  how to answer, carries no id of the message itself, and is stamped
  delivered once the prompt went out
  (`agent_messages.rs::a_message_is_handed_to_the_agent_it_was_sent_to`). Two
  tasks staffed on the same skills are still told apart, since each relay
  also names its own task and agent
  (`::two_tasks_on_the_same_skills_are_named_apart_in_their_relay_to_the_orchestrator`).
- An agent can write to the orchestrator, and it reaches the orchestrator's
  agent
  (`agent_messages.rs::an_agent_writes_to_the_orchestrator_and_it_reaches_its_agent`).
- A recipient the task does not staff is refused and nothing is written
  (`agent_messages.rs::a_message_to_an_agent_the_task_does_not_staff_is_refused`).
- A message is delivered once, and the stamp says which have gone. The claim
  answers true once and false after
  (`store.rs::a_message_is_delivered_once_and_the_stamp_says_so`).
- A message to an agent mid-turn stays unstamped in the queue, and a read
  returns it stamped. Its queued prompt is skipped after the turn
  (`agent_messages.rs::a_message_to_an_agent_mid_turn_reaches_it_through_a_read`).
- A message queued behind a turn is stamped when its prompt goes out, and a
  read after that returns nothing
  (`agent_messages.rs::a_message_queued_behind_a_turn_is_stamped_when_its_prompt_goes_out`).
- A message is queued once across scheduler passes, and one prompt carries
  it (`agent_messages.rs::a_message_is_queued_once_across_scheduler_passes`).
- A read mid-turn returns the queued messages oldest first, and no prompt
  carries them after the turn
  (`agent_messages.rs::messages_keep_their_order_across_a_read_and_the_queue`).
- A message whose prompt the agent answered with an error stays stamped
  (`agent_messages.rs::a_message_the_agent_answered_with_an_error_stays_delivered`).
- A claimed message whose prompt the agent's stdin never took gives its
  claim back, and reaches the agent after the relaunch
  (`agent_messages.rs::an_unwritten_prompt_gives_its_message_back_for_the_relaunch`).
  A prompt counts as written once its whole line is taken
  (`acp_transport.rs::a_prompt_counts_once_its_whole_line_is_written`).
- A message handed to an agent as a prompt is gone from what a default
  `read_messages` gives that agent, and a read of the whole thread holds it
  (`agent_messages.rs::a_message_handed_over_as_a_prompt_is_absent_from_a_default_read`).
- A message a read hands over is stamped by that read, and is never handed
  over as a prompt afterwards — through a resume of its recipient and a
  restart of the daemon
  (`agent_messages.rs::a_message_a_read_hands_over_is_never_handed_over_as_a_prompt`).
- A message to the agent of a column that is not current waits for its
  column (`agent_messages.rs::a_message_to_an_idle_column_waits_for_its_column`).
- A delivering read is refused a channel of another goal, and spends no stamp
  on it
  (`agent_messages.rs::a_delivering_read_is_refused_a_channel_of_another_goal`).
- A goal's channel holds none of what its tasks said
  (`agent_messages.rs::a_goals_channel_holds_none_of_what_its_tasks_said`).
- A message names the column it is for
  (`tools.rs::a_message_names_the_column_it_is_for`),
  the orchestrator is addressed by name
  (`::the_orchestrator_is_addressed_by_name_and_needs_no_agent_id`), and a
  message to nobody is refused with the addresses that would work
  (`::a_message_to_nobody_is_refused_with_the_addresses_that_would_work`).

## Sources

`crates/ariadne-core/src/lib.rs` (`MessageKind`),
`crates/ariadne-store/src/messages.rs`,
`crates/ariadne-daemon/src/scheduler/messages.rs` (the transport),
`crates/ariadne-daemon/src/acp.rs` (the claim before the prompt),
`crates/ariadne-daemon/src/acp_transport.rs` (the write witness),
`crates/ariadne-daemon/src/http/channel.rs` (`send`, `read_channel`),
`crates/ariadne-cli/src/commands/mcp/tools.rs`.

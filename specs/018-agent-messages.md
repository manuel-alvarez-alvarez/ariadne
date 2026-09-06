---
id: agent-messages
status: current
updated: 2026-09-06
areas: [core, store, api, daemon, mcp, cli, ui]
commits: [1b09ac10]
tests:
  - crates/ariadne-daemon/tests/agent_messages.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
---

# Agent messages

One channel for everything the agents say to each other, and one transport
that puts it in front of them.

## Scope

In: what a message is, who it can be for, what the six kinds mean, how a
message reaches the agent it was sent to, and how long an agent stays around
to receive one.

Out: what a round is (004), the states a verdict moves a task through (001),
and the wording of the text a message arrives in (006).

## Behavior

1. Everything one agent says to another is a **message**. There is no second
   table for verdicts: a verdict is a message whose kind closes a round, which
   is what makes "the reviewer asked the author something" possible at all.
2. Six kinds, and the kind is what the daemon reads:
   - `question` — the sender needs it answered before it can go on;
   - `answer` — the answer to one;
   - `review_request` — the author asking a reviewer to look;
   - `approve`, `request_changes` — a reviewer's verdict on the round;
   - `note` — anything worth saying that nobody has to answer.
3. A message has exactly one recipient, so a review request that goes to three
   reviewers is three messages: whether it has been seen is a question about
   one reader, and one row with three of them could not answer it.
4. A recipient is the orchestrator of the goal, or one staffed agent of a task
   named by the id `get_task` lists. An agent has no name, so the id is the
   address; the orchestrator needs none, since a goal has one.
5. An answer names only the message it answers. Where it goes is who asked,
   which the daemon reads off that message rather than trusting the answer to
   address it again.
6. A message is about one task, or about the goal itself. The goal's channel
   is the orchestrator's inbox, and everything said about a task is on that
   task.
7. Two rules the daemon holds the channel to, and no more — it is a
   conversation, and the daemon is not in it:
   - a recipient that the task does not staff is refused;
   - a verdict comes from a reviewer of that task, on a task that is
     `under_review`, and one reviewer votes once a round.
8. The daemon delivers a message by typing it into the recipient's pane and
   submitting it, so it arrives as a turn. There is no inbox to poll.
   `delivered_at` says which have gone; a pane that is busy is not typed into,
   so a message to an agent mid-turn waits for the pass after it finishes.
9. Nothing about a message starts a session. Waking an agent is the
   lifecycle's business (009), and a message is not a reason to put one back
   on a task nobody is working on.
10. Every agent of a task stays up until the task is over. A reviewer that has
    voted sits idle rather than being killed: the author may have something to
    ask it, and an agent that is gone can be asked nothing.
11. `request_review` writes one `review_request` per reviewer, carrying the
    summary the author asked with, so the channel holds the whole
    conversation rather than the half of it that happened to be typed.
12. The MCP surface is four tools every seat has: `ask`, `tell`, `reply` and
    `read_messages` (013).

## Acceptance criteria

- A reviewer asks the author and the author answers, without either leaving
  the task and without the round moving
  (`agent_messages.rs::a_reviewer_asks_the_author_and_the_author_answers_it`).
- The message is typed into the recipient's pane, names the sender by its
  skills, carries the id an answer names, and is stamped delivered
  (`agent_messages.rs::a_message_is_typed_into_the_pane_it_was_sent_to`).
- An agent can write to the orchestrator, and it reaches its pane
  (`agent_messages.rs::an_agent_writes_to_the_orchestrator_and_it_reaches_its_pane`).
- A recipient the task does not staff is refused and nothing is written
  (`agent_messages.rs::a_message_to_an_agent_the_task_does_not_staff_is_refused`).
- Only a reviewer of the task can send a verdict
  (`agent_messages.rs::only_a_reviewer_of_the_task_can_send_a_verdict`), only
  while a round is open (`::a_verdict_outside_a_round_is_refused`), and one
  per reviewer per round
  (`store.rs::one_verdict_per_reviewer_per_round`) — a question in the same
  round is not a second one (same test).
- A review request reaches every reviewer with the author's summary
  (`agent_messages.rs::a_review_request_reaches_every_reviewer_as_a_message`).
- A message is delivered once, and the stamp says which have gone
  (`store.rs::a_message_is_delivered_once_and_the_stamp_says_so`).
- A reviewer that voted is left where it is
  (`agent_messages.rs::a_reviewer_that_voted_is_left_where_it_is`).
- Asking names the agent and answering names only the message
  (`tools.rs::asking_names_the_agent_and_answering_names_only_the_message`),
  the orchestrator is addressed by name
  (`::the_orchestrator_is_addressed_by_name_and_needs_no_agent_id`), and a
  message to nobody is refused with the addresses that would work
  (`::a_message_to_nobody_is_refused_with_the_addresses_that_would_work`).
- A verdict is a message to the author of the kind that closes a round
  (`tools.rs::a_verdict_is_a_message_to_the_author_of_the_kind_that_closes_a_round`).

## Sources

`crates/ariadne-core/src/lib.rs` (`MessageKind`),
`crates/ariadne-store/src/messages.rs`,
`crates/ariadne-daemon/src/scheduler/messages.rs` (the transport),
`crates/ariadne-daemon/src/http/landing.rs` (`send`),
`crates/ariadne-cli/src/commands/mcp/tools.rs`.

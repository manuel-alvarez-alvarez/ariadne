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

Out: what a review is (004), the states a verdict moves a task through (001),
and the wording of the text a message arrives in (006).

## Behavior

1. Everything one agent says to another is a **message**. There is no second
   table for verdicts: a verdict is a message whose kind settles a review, so
   one channel holds the whole of what happened to a task.
2. Four kinds, and the kind is what the daemon reads:
   - `message` — one agent telling another what it needs from it;
   - `review_request` — the author asking a reviewer to look;
   - `approve`, `request_changes` — a reviewer's verdict on the review.
3. A message has exactly one recipient, so a review request that goes to three
   reviewers is three messages: whether it has been seen is a question about
   one reader, and one row with three of them could not answer it.
4. A recipient is the orchestrator of the goal, or one staffed agent of a task
   named by the id `get_task` lists. An agent has no name, so the id is the
   address; the orchestrator needs none, since a goal has one.
5. Nothing is answered. There is no question, no answer and no reply — one
   verb (`tell`) and one kind, and the recipient acts on what it is told.
   Every message arrives in a pane as a turn (8), so a channel that invites
   one back spends two turns saying nothing: a question earns an answer, which
   earns an acknowledgement, which earns a thank you. What is delivered says
   so too — act on it, do not answer it — and the seat playbooks (006) say it
   where each seat is told to use the channel.
6. An agent that cannot go on has the ways out its seat already gives it: an
   author calls `fail_task` with the reason, and a reviewer requests changes
   naming what blocks it. Both move the task, which writing at each other does
   not.
7. A message is about one task, or about the goal itself. The goal's channel
   is the orchestrator's inbox, and everything said about a task is on that
   task.
8. Two rules the daemon holds the channel to, and no more — it is a
   conversation, and the daemon is not in it:
   - a recipient that the task does not staff is refused;
   - a verdict comes from a reviewer of that task, on a task that is
     `under_review`, and one reviewer votes once on each review it is asked
     for (004).
9. The daemon delivers a message by typing it into the recipient's pane and
   submitting it, so it arrives as a turn. There is no inbox to poll.
   `delivered_at` says which have gone; a pane that is busy is not typed into,
   so a message to an agent mid-turn waits for the pass after it finishes.
10. Nothing about a message starts a session. Waking an agent is the
    lifecycle's business (009), and a message is not a reason to put one back
    on a task nobody is working on.
11. Every agent of a task stays up until the task is over. A reviewer that has
    voted sits idle rather than being killed: the author may still have
    something it needs from it, and an agent that is gone can be told nothing.
12. `request_review` writes one `review_request` per reviewer, carrying the
    summary the author asked with, so the channel holds the whole of the
    review rather than the half of it that happened to be typed.
13. The MCP surface is two tools every seat has: `tell` and `read_messages`
    (013).

## Acceptance criteria

- Agents write to each other without leaving the task and without the review
  moving (`agent_messages.rs::agents_write_to_each_other_without_leaving_the_task`).
- The message is typed into the recipient's pane, names the sender by its
  skills, carries no id to answer on, and is stamped delivered
  (`agent_messages.rs::a_message_is_typed_into_the_pane_it_was_sent_to`).
- An agent can write to the orchestrator, and it reaches its pane
  (`agent_messages.rs::an_agent_writes_to_the_orchestrator_and_it_reaches_its_pane`).
- A recipient the task does not staff is refused and nothing is written
  (`agent_messages.rs::a_message_to_an_agent_the_task_does_not_staff_is_refused`).
- Only a reviewer of the task can send a verdict
  (`agent_messages.rs::only_a_reviewer_of_the_task_can_send_a_verdict`), only
  while a review is open (`::a_verdict_outside_a_review_is_refused`), and one
  per reviewer per review asked for
  (`::only_one_verdict_per_reviewer_per_review_is_taken`); what a verdict
  belongs to is that request
  (`store.rs::a_verdict_belongs_to_the_review_that_was_asked_for`), and a
  question in between is not a second one (same test).
- A review request reaches every reviewer with the author's summary
  (`agent_messages.rs::a_review_request_reaches_every_reviewer_as_a_message`).
- A message is delivered once, and the stamp says which have gone
  (`store.rs::a_message_is_delivered_once_and_the_stamp_says_so`).
- A reviewer that voted is left where it is
  (`agent_messages.rs::a_reviewer_that_voted_is_left_where_it_is`).
- A message names the agent it is for
  (`tools.rs::a_message_names_the_agent_it_is_for`),
  the orchestrator is addressed by name
  (`::the_orchestrator_is_addressed_by_name_and_needs_no_agent_id`), and a
  message to nobody is refused with the addresses that would work
  (`::a_message_to_nobody_is_refused_with_the_addresses_that_would_work`).
- A verdict is a message to the author of the kind that settles a review
  (`tools.rs::a_verdict_is_a_message_to_the_author_of_the_kind_that_closes_a_round`).

## Sources

`crates/ariadne-core/src/lib.rs` (`MessageKind`),
`crates/ariadne-store/src/messages.rs`,
`crates/ariadne-daemon/src/scheduler/messages.rs` (the transport),
`crates/ariadne-daemon/src/http/landing.rs` (`send`),
`crates/ariadne-cli/src/commands/mcp/tools.rs`.

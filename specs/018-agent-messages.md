---
id: agent-messages
status: current
updated: 2026-09-12
areas: [core, store, api, daemon, mcp, cli, ui]
commits: [1b09ac10]
tests:
  - crates/ariadne-daemon/tests/agent_messages.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/tests/multi_author_tasks.rs
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
   - `message` — one agent asking another something, or answering it;
   - `review_request` — the author asking a reviewer to look;
   - `approve`, `request_changes` — a reviewer's verdict on the review.
3. A message has exactly one recipient, so a review request that goes to three
   reviewers is three messages: whether it has been seen is a question about
   one reader, and one row with three of them could not answer it.
4. A recipient is the orchestrator of the goal, or one staffed agent of a task
   named by the id `get_task` lists. An agent has no name, so the id is the
   address; the orchestrator needs none, since an orchestrated goal has one.
   A message addressed to an unorchestrated goal's orchestrator is refused:
   the goal has no orchestrator, and the user answers in the console
   (`acp_session_adoption.rs::a_message_to_an_unorchestrated_goals_orchestrator_is_refused`).
5. One verb (`send_message`) and one kind, for asking and for answering.
   There is no `answer` kind and no `reply`: an answer is a message to
   whoever asked, addressed the way the question was, so nothing threads and
   no row points at another.
6. A message asks or answers, and carries nothing else — no acknowledgement,
   no thanks, and nothing about what the sender is going to do next. Every
   message arrives at its recipient as a turn (9), so a courtesy costs the recipient
   a turn: that is what the tool's own description bans, and it is banned
   there once rather than in each seat's playbook (006).
7. A message is about one task, or about the goal itself. The goal's channel
   is the orchestrator's inbox, and everything said about a task is on that
   task.
8. Two rules the daemon holds the channel to, and no more — it is a
   conversation, and the daemon is not in it:
   - a recipient that the task does not staff is refused;
   - a verdict comes from a reviewer of that task, on a task that is
     `under_review`, and one reviewer votes once on each review it is asked
     for (004). On a task staffed with several authors the review a verdict
     belongs to is the one its address names — the author whose change it
     judges — and a verdict to an author that has not asked is refused.
9. The daemon delivers a message by handing it to the recipient's ACP agent
   as a `session/prompt` (009, 021), so it arrives as a turn. There is no
   inbox to poll. A message to an agent mid-turn is queued by the runtime
   behind that turn. `delivered_at` is stamped when the runtime takes the
   text, so a message still undelivered is one waiting for a live agent to
   take it.
10. Nothing about a message starts a session. Waking an agent is the
    lifecycle's business (009), and a message is not a reason to put one back
    on a task nobody is working on.
11. Every agent of a task stays up until the task is over. A reviewer that has
    voted sits idle rather than being killed: the author may still have
    something it needs from it, and an agent that is gone can be told nothing.
12. `request_review` writes one `review_request` per reviewer, carrying the
    summary the author asked with, so the channel holds the whole of the
    review rather than the half of it that happened to be said. On a task
    staffed with several authors whose pick is still open, that row is not
    handed to the reviewer as a bare message: the reviewer's full
    briefing carries it — the summary with the author and its branch, after
    its worktree has moved there (004) — and stamps it delivered.
13. The MCP surface is two tools every seat has: `send_message` and
    `read_messages` (013).

## Acceptance criteria

- Agents write to each other without leaving the task and without the review
  moving (`agent_messages.rs::agents_write_to_each_other_without_leaving_the_task`).
- The message is handed to the recipient's agent as a prompt, names the
  sender by its skills, carries no id to answer on, and is stamped delivered
  (`agent_messages.rs::a_message_is_handed_to_the_agent_it_was_sent_to`).
- An agent can write to the orchestrator, and it reaches the orchestrator's
  agent
  (`agent_messages.rs::an_agent_writes_to_the_orchestrator_and_it_reaches_its_agent`).
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
- On a contested task, a review request reaches a live reviewer only as its
  briefing
  (`multi_author_tasks.rs::a_contested_review_request_reaches_a_live_reviewer_only_as_its_briefing`).
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

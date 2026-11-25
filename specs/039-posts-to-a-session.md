---
id: posts-to-a-session
status: current
updated: 2026-11-25
areas: [api, daemon, mcp]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/posts.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/mcp.rs
---

# Posts to a session

One agent, or the orchestrator, puts text straight into the live session of
another, on one host, and nothing is stored.

## Scope

In: what a post is, who may send and receive one, what the recipient reads,
and each refusal.

Out: the stored message channel (018), posts to and from a task on another
host, and the session rules, prompts and skills.

## Behavior

1. A **post** is text for the live session of one recipient. The daemon
   queues it as a prompt in that session and writes no row in the `messages`
   table. `send_message` and `read_messages` (018) work as they did.
2. The MCP tool is `post_to_session` (013). The orchestrator seat and the
   agent seat list it. The reviewer and keeper seats do not.
3. It takes `to` (`orchestrator` or a column id), an optional `task_id` and a
   `body`. With no `task_id`, it takes the task of the session. The daemon
   route is `POST /v1/tasks/{id}/posts` with `{"to", "body"}`. The header
   `x-ariadne-session` names the sender.
4. The prompt names the sender: `your orchestrator`, or the column, the task
   id and the task title. It fences the body as data (006 rules 18 and 19).
   It tells the recipient to answer a question with `post_to_session` to the
   address of the sender.
5. A post to a recipient in a turn waits behind that turn and arrives as the
   next turn.
6. A post never answers a pending permission or a pending question of the
   recipient, and does not clear the attention of the recipient.
7. The daemon refuses a post when the recipient's column is not the current
   column of the task, and says so.
8. The daemon refuses a post when the recipient has no live session, and says
   so.
9. The daemon refuses a post to a column that the task does not staff, to a
   task of another goal, and to a task on another host. Each refusal says why.
10. A refusal for a wrong address names each address that works. Each refusal
    is a bad request, and the tool returns it as bad parameters with the
    sentence of the daemon (013 rule 7).
11. The tool description permits a question, an answer or a blocker report
    only. It tells the sender to end its turn after a question, because the
    answer arrives as a new turn.

## Acceptance criteria

- A post reaches the live session as a prompt that names the sender, fences
  the body and names the answer address, and writes no row
  (`posts.rs::a_post_reaches_the_live_session_as_a_prompt_and_writes_no_row`).
- The orchestrator sends and receives a post
  (`posts.rs::the_orchestrator_posts_and_is_posted_to`).
- A post to a column that is not current, to a column the task does not
  staff, to a task of another goal, and to a recipient with no live session
  is refused with its reason
  (`posts.rs::a_post_that_cannot_land_is_refused_with_the_reason`,
  `::a_post_to_a_column_without_a_live_session_is_refused`).
- The tool posts to the route of the task, and its description permits a
  question, an answer or a blocker report and tells the sender to end the turn
  (`tools.rs::post_to_session_posts_to_the_posts_route_and_says_to_end_the_turn`).
- The seats list the tool as rule 2 says
  (`mcp.rs::the_agent_seat_lists_its_step_tools_and_nothing_of_a_review`).

## Known gap

A task on another host is refused. Posts across hosts are a later task.

## Sources

`crates/ariadne-daemon/src/http/posts.rs`,
`crates/ariadne-daemon/src/agents/prompts.rs` (`post_briefing`),
`crates/ariadne-cli/src/commands/mcp/tools.rs`.

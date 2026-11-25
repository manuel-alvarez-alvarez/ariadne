---
id: agent-messages
status: superseded
updated: 2026-11-25
superseded_by: 039
areas: [core, store, api, daemon, mcp, cli, ui]
commits: [1b09ac10]
tests:
  - crates/ariadne-daemon/tests/it/posts.rs
  - crates/ariadne-store/tests/message_migration.rs
  - crates/ariadne-daemon/src/acp.rs
---

# Agent messages

Superseded by [039](039-posts-to-a-session.md). This file remains only so
that links in history resolve.

## Scope

Posts to a live session now carry what one agent says to another. Ariadne
stores no agent message.

## Behavior

1. See [039](039-posts-to-a-session.md) for what one agent says to another.
2. Ariadne holds no agent message: no table, no route, no event, no metric,
   no relay between hosts and no CLI command.
3. Migration 0025 drops the `messages` table and keeps every other row.
   It builds again, without that table, each trigger that named it.
4. A relay turn file that an earlier release wrote for an agent message
   still loads, with no delivery to claim.

## Acceptance criteria

1. A post reaches the live session as a prompt and writes no row
   (`crates/ariadne-daemon/tests/it/posts.rs::a_post_reaches_the_live_session_as_a_prompt_and_writes_no_row`).
2. A database of the last release loses its messages and no other row
   (`crates/ariadne-store/tests/message_migration.rs::an_upgrade_drops_the_messages_and_keeps_every_other_row`).
3. A goal and a task still delete after the upgrade
   (`crates/ariadne-store/tests/message_migration.rs::a_goal_and_a_task_still_delete_after_the_upgrade`).
4. A turn file of an agent message still loads, and a turn file keeps a
   delivery this build knows
   (`acp::tests::a_turn_file_of_an_agent_message_still_loads`,
   `::a_turn_file_keeps_a_known_delivery`).

## Sources

`crates/ariadne-store/migrations/0025_drop_messages.sql`.

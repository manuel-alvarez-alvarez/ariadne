---
id: scheduler-attention-and-watchdogs
status: current
updated: 2026-09-04
areas: [daemon]
commits: [f68b8ec1, 506e9d76, 7add2a61]
tests:
  - crates/ariadne-daemon/tests/scheduler_attention.rs
  - crates/ariadne-daemon/tests/scheduler_tmux_outage.rs
  - crates/ariadne-daemon/tests/events.rs
---

# Scheduler, attention and watchdogs

The loop that keeps the world matching the plan, and the one clock that
decides an agent has stopped working.

## Scope

In: the reconciliation loop, what each entity wants by status, the quiet
watchdog and its timeline, attention reasons, the sweeps over every session,
and behaviour under a tmux outage.

Out: what a resumed agent is told (006), and the compaction a hand-off owes
(010).

## Behavior

1. The scheduler is an event-driven reconciliation loop: HTTP handlers send
   events after writes, and a periodic tick reconciles everything, so crashes,
   missed events and dead panes self-heal.
2. Every rule is idempotent — read the state, compare with what is wanted, act
   — so a pass that arrives late does what the state says now, never a replay
   of what it missed.
3. A goal in `planning` wants one live planner; a goal past it wants that
   planner compacted and then let go. A task wants an engineer from `ready` to
   the merge, the reviewers a round is waiting on, and the cleanup its ending
   owes (001, 004).
4. One clock governs a quiet agent: how long since the session was heard from
   at all. On that clock sit a nudge, then the user, then the pane killed and
   the agent put back on its feet — at 180 s, 600 s and 1800 s
   (`QUIET_NUDGE_SECS`, `QUIET_FLAG_SECS`, `QUIET_RELAUNCH_SECS`), reconciled
   on a 5 s tick, with a 30 s grace before a starting session is swept.
5. What the nudge *is* the pane decides, so the composer is read before one is
   spent: an instruction still sitting unsent gets the Enter alone, and an
   agent mid-turn is not nudged at all.
6. An agent is nudged once for the situation it went quiet in, not once per
   pass.
7. Attention on a session means a human must act, and it is raised only while
   the work that session was started for is still its own to do. A reviewer
   that has voted, an engineer whose task is under review and a planner whose
   goal has left planning are agents nobody is waiting on, whatever their pane
   shows.
8. A session waiting on a person is never nudged and never relaunched: the
   quiet is the point.
9. An agent that reported an error is left alone rather than nudged over it.
10. A pane that has vanished under work that is still active is flagged
    disconnected; one nobody is waiting on is not raised at all.
11. Attention is cleared by the thing that answers it: resuming the session,
    typing into the pane, or the work moving on. A superseded session drops its
    attention when its replacement starts, and a prompt flag never outlives the
    session it was raised on.
12. When tmux cannot be reached, a pass neither spawns nor fails anything: a
    silent agent whose pane cannot be read is left for the next pass.
13. A task that could never be started fails with the reason on it, and a
    planner that can never be started gives up with exactly one alarm.
14. A goal whose tasks have all landed is completed; a session that outlived
    its completed goal is killed.
15. A pass that has three agents to nudge does not wait on the keystrokes:
    delivery happens off the loop.

## Acceptance criteria

- An idle planner, reviewer or engineer past the threshold is raised on its
  session (`scheduler_attention.rs::a_planner_idle_past_the_threshold_is_raised_on_its_session`,
  `::a_reviewer_idle_past_the_threshold_is_raised_on_its_session`,
  `::an_engineer_stall_flags_the_task_and_its_session`).
- An idle agent is nudged once for the situation it went quiet in
  (`::an_idle_agent_is_nudged_once_for_the_situation_it_went_quiet_in`), a
  composer still holding its instruction gets the Enter alone
  (`::a_composer_still_holding_its_instruction_gets_the_enter_alone`), and an
  agent mid-turn is not nudged (`::an_agent_in_the_middle_of_a_turn_is_not_nudged`).
- A session waiting on a person is never nudged or relaunched
  (`::a_session_waiting_on_a_person_is_never_nudged`,
  `::a_running_agent_waiting_on_a_person_is_never_relaunched`), and an agent
  that reported an error is left alone (`::an_agent_that_reported_an_error_is_left_alone`).
- A vanished pane with work still active is flagged disconnected
  (`::a_vanished_pane_with_work_still_active_is_flagged_disconnected`); one
  nobody waits on is not raised (`::a_vanished_pane_nobody_is_waiting_on_is_not_raised`).
- Resuming clears attention (`::resuming_a_session_clears_its_attention`), a
  superseded session drops it (`::a_superseded_session_drops_its_attention_when_the_replacement_starts`),
  and a stale prompt flag from before the daemon started is swept up
  (`::a_stale_prompt_flag_from_before_the_daemon_started_is_swept_up`).
- An agent that reports nothing is flagged and then relaunched
  (`::an_agent_that_reports_nothing_is_flagged_and_then_relaunched`), while one
  that keeps reporting is left alone (`::a_running_agent_that_keeps_reporting_is_left_alone`).
- Under a tmux outage nothing is spawned and nothing fails
  (`scheduler_tmux_outage.rs::reconciliation_with_tmux_unavailable_neither_spawns_nor_fails_the_task`,
  `::a_silent_agent_whose_pane_cannot_be_read_is_left_for_the_next_pass`).
- A goal whose tasks all landed is completed
  (`::a_goal_whose_tasks_all_landed_is_completed`), and a session outliving its
  completed goal is killed (`::a_session_that_outlived_its_completed_goal_is_killed`).
- A pass with three agents to nudge does not wait on the keystrokes
  (`::a_pass_with_three_agents_to_nudge_does_not_wait_on_the_keystrokes`).

## Sources

`crates/ariadne-daemon/src/scheduler/` (`mod`, `goals`, `tasks`, `sweeps`,
`quiet`, `delivery`), `crates/ariadne-daemon/src/attention.rs`.

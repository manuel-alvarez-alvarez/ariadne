---
id: scheduler-attention-and-watchdogs
status: current
updated: 2026-09-11
areas: [daemon]
commits: [f68b8ec1, 506e9d76, 7add2a61, a69b953f, 29e6d84e]
tests:
  - crates/ariadne-daemon/tests/scheduler_attention.rs
  - crates/ariadne-daemon/tests/events.rs
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/src/scheduler/mod.rs
---

# Scheduler, attention and watchdogs

The loop that keeps the world matching the plan, and the one clock that
decides an agent has stopped working.

## Scope

In: the reconciliation loop, what each entity wants by status, how the
scheduler hands an agent a prompt, the quiet watchdog and its timeline,
attention reasons, the sweeps over every session, and what a launch that
dies on arrival costs.

Out: what a resumed agent is told (006), how a message is carried (018), and
the ACP runtime that takes a prompt (021).

## Behavior

1. The scheduler is an event-driven reconciliation loop. HTTP handlers send
   events after writes, and a tick every 5 s (`TICK_SECS`) reconciles
   everything, so crashes, missed events and dead agent processes self-heal.
2. Every rule is idempotent — read the state, compare it with what is wanted,
   act — so a pass that arrives late does what the state says now, never a
   replay of what it missed.
3. A goal wants one live orchestrator for its whole life, `planning` and
   `active` alike. Finalizing the plan is a hand-off, not an ending.
4. The orchestrator is the agent the daemon tells when a task needs a
   decision: a task that failed, or a goal with nothing left running. It is
   told once per situation. Running work is what it delegated, and it is not
   woken for that.
5. A task wants an author from `ready` to the merge, the reviewers a review
   is waiting on, and the cleanup its ending owes (001, 004).
6. Everything the scheduler says to an agent — a nudge, a review briefing, an
   agent message — is handed to the session's ACP agent as a
   `session/prompt` (021). The runtime sends it at once to an agent between
   turns and queues it behind a running turn.
7. A pass never waits on a delivery: the runtime queues each prompt, so a
   pass with several agents to nudge hands them all their prompt at once.
8. A prompt the runtime refuses, because no agent runs for the session, gives
   the nudge it was spent on back, so the next pass sends it again.
9. One clock governs a quiet agent: how long since the session was heard
   from at all. On that clock sit a nudge at 180 s, the user at 600 s, and at
   1800 s the agent killed and put back on its feet (`QUIET_NUDGE_SECS`,
   `QUIET_FLAG_SECS`, `QUIET_RELAUNCH_SECS`).
10. The flag clears 600 s because it is spent whatever the agent is doing,
    and the landing briefing sends an author to sleep up to five minutes at a
    time. The build fails if the three thresholds fall out of that order.
11. Only an idle agent is nudged. An agent inside a turn is left to the
    thresholds behind the nudge, since a nudge would only queue behind the
    turn it is in.
12. An agent is nudged once for the situation it went quiet in, not once per
    pass. A new task status or a new review is a new situation.
13. The agent that comes back from a relaunch is the one the row belongs to
    from then on: the exit the killed one still has to report changes
    nothing (008, 012).
14. Relaunches of one session are bounded by the spawn-retry budget of 3
    (`SPAWN_RETRY_BUDGET`). A task whose agent wedges again after every
    relaunch fails.
15. Attention on a session means a human must act. It is raised only while
    the work that session was started for is still its own to do.
16. A reviewer that has voted, an author whose task is under review and an
    orchestrator of a finished goal are agents nobody is waiting on, and
    none of them raises attention.
17. A session waiting on a person is never nudged and never relaunched: the
    quiet is the point.
18. A flag raised for the user (`waiting_user`) is not an agent waiting on
    one. A wedged agent carrying it keeps the flag and is still relaunched.
19. An agent that reported an error is left alone rather than nudged over
    it.
20. The liveness sweep asks the ACP runtime whether each live session's
    agent runs, and the runtime answers for certain. A session whose agent
    is gone is retired.
21. A session still `starting` is left alone for 30 s (`START_GRACE_SECS`),
    since its row exists before its agent does. A start older than that is a
    launch that is not coming, and it is swept.
22. An agent that vanished while its work is still active is flagged
    `disconnected`. One nobody is waiting on is retired and not raised.
23. The author of an active task with no live session is resumed. When even
    that cannot start, its session is flagged `disconnected`.
24. Attention is cleared by the thing that answers it: resuming the session,
    input on its console (008), or the work moving on. A superseded session
    drops its attention when its replacement starts.
25. A prompt flag never outlives the session it was raised on, and a stale
    one from before the daemon started is swept up on the first pass.
26. A task that could never be started fails with the reason on it after the
    spawn-retry budget. An orchestrator that can never be started gives up
    with exactly one alarm, and taking the alarm down starts the count again.
27. A launch that works and an agent that runs are not the same thing. A
    launch that ends before its agent was ever heard from died on arrival,
    and it spends an attempt like a launch that never came up.
28. The runtime's own report of the agent's end (`session_end`,
    `session.error`) is not word from the agent, so it does not stamp the
    session's activity. Otherwise a death on arrival would read as an agent
    that spoke.
29. The attempts are given back when an agent reports, not when a launch
    returns. A goal whose orchestrator dies on arrival every time is left
    with one alarm and nothing started again. A task whose author or
    reviewer does fails, saying its agent stopped as soon as it started.
30. A goal whose tasks have all landed wakes its orchestrator, which decides
    whether the goal is met. A session that outlived its completed goal is
    killed on every pass.

## Acceptance criteria

- The orchestrator of a goal under way stays up
  (`scheduler_attention.rs::an_idle_orchestrator_stays_up_for_the_whole_goal`),
  is woken once for a failed task
  (`::a_failed_task_wakes_the_orchestrator_once`) and for a goal with nothing
  left running (`::a_goal_whose_tasks_all_landed_wakes_its_orchestrator`),
  and is left alone while its tasks run
  (`::a_goal_whose_tasks_are_running_leaves_its_orchestrator_alone`).
- A nudge to an idle agent arrives as a `session/prompt`
  (`acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`).
- A pass with three agents to nudge hands all three their prompt at once
  (`scheduler_attention.rs::a_pass_with_three_agents_to_nudge_does_not_wait_on_the_deliveries`).
- An idle orchestrator, reviewer or author past the threshold is raised on
  its session
  (`scheduler_attention.rs::an_orchestrator_idle_past_the_threshold_is_raised_on_its_session`,
  `::a_reviewer_idle_past_the_threshold_is_raised_on_its_session`,
  `::an_author_stall_flags_the_task_and_its_session`).
- An idle agent is nudged once for the situation it went quiet in
  (`::an_idle_agent_is_nudged_once_for_the_situation_it_went_quiet_in`), and
  an agent mid-turn is not nudged
  (`::an_agent_in_the_middle_of_a_turn_is_not_nudged`).
- The three thresholds keep their order and the flag's floor, or the daemon
  does not build (the `const` assertions in `scheduler/mod.rs`).
- An agent that reports nothing is flagged and then relaunched
  (`::an_agent_that_reports_nothing_is_flagged_and_then_relaunched`), one
  that keeps reporting is left alone
  (`::a_running_agent_that_keeps_reporting_is_left_alone`), and one that
  wedges after every relaunch fails its task
  (`::an_agent_that_wedges_after_every_relaunch_fails_its_task`).
- A reviewer that voted and an orchestrator of a finished goal raise no
  attention (`events.rs::a_reviewer_that_already_voted_raises_no_attention`,
  `::an_orchestrator_of_a_finished_goal_raises_no_attention`).
- A session waiting on a person is never nudged or relaunched
  (`scheduler_attention.rs::a_session_waiting_on_a_person_is_never_nudged`,
  `::a_running_agent_waiting_on_a_person_is_never_relaunched`), and an agent
  that reported an error is left alone
  (`::an_agent_that_reported_an_error_is_left_alone`).
- A wedged agent flagged for the user keeps the flag and is relaunched
  (`::a_wedged_agent_flagged_for_the_user_keeps_the_flag_and_is_relaunched`),
  and a published task still says the merge is the user's after its author
  is resumed
  (`::a_published_task_still_says_the_merge_is_the_users_after_its_author_is_resumed`).
- A dead agent is reaped and its session retired
  (`acp_runtime.rs::a_dead_acp_agent_is_reaped_and_its_session_retired`), and
  a starting session is swept only once its grace window has run out
  (`scheduler_attention.rs::a_starting_session_is_swept_only_once_its_grace_window_has_run_out`).
- A vanished agent with work still active is flagged disconnected
  (`::a_vanished_agent_with_work_still_active_is_flagged_disconnected`), one
  nobody waits on is not raised
  (`::a_vanished_agent_nobody_is_waiting_on_is_not_raised`), and an author
  that cannot be resumed is flagged disconnected
  (`::an_author_that_cannot_be_resumed_is_flagged_disconnected`).
- Resuming clears attention (`::resuming_a_session_clears_its_attention`), a
  superseded session drops it
  (`::a_superseded_session_drops_its_attention_when_the_replacement_starts`),
  and the sweep lets go of a blocked agent only once its work moved on
  (`::the_sweep_lets_go_of_a_blocked_agent_only_once_its_work_moved_on`).
- A prompt flag does not outlive its session
  (`::a_prompt_flag_does_not_outlive_the_session_it_was_raised_on`), and a
  stale one from before the daemon started is swept up
  (`::a_stale_prompt_flag_from_before_the_daemon_started_is_swept_up`).
- A task that could never be started fails with the reason on it
  (`::a_task_that_could_never_be_started_fails_with_the_reason_on_it`), and an
  orchestrator that can never be started gives up with one alarm
  (`::an_orchestrator_that_can_never_be_started_gives_up_with_one_alarm`).
- An orchestrator that dies the moment it starts is given up on, with one
  alarm and nothing started again
  (`::an_orchestrator_that_dies_the_moment_it_starts_is_given_up_on`), and a
  task whose agent does fails with the reason on it
  (`::a_task_whose_agent_dies_the_moment_it_starts_fails_with_the_reason_on_it`).
- A session outliving its completed goal is killed
  (`::a_session_that_outlived_its_completed_goal_is_killed`).

## Known gap

- No test pins that a prompt the runtime refuses gives its nudge back.

## Sources

`crates/ariadne-daemon/src/scheduler/` (`mod`, `goals`, `tasks`, `sweeps`,
`quiet`, `messages`), `crates/ariadne-daemon/src/attention.rs`,
`crates/ariadne-daemon/src/http/events.rs`.

---
id: models-effort-and-pins
status: current
updated: 2026-09-06
areas: [core, api, daemon, cli]
commits: [090c5158, e94647fd, d94042f4, c42ebeee, 305ad2fb, a69b953f, 03f9c8b7]
tests:
  - crates/ariadne-daemon/tests/models.rs
  - crates/ariadne-daemon/tests/pins.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - ui/src/features/models/models-page.test.tsx
  - ui/src/features/models/pin-picker.test.tsx
---

# Models, effort and pins

What an agent runs on: which CLI, which model of it, how hard it reasons, and
who gets to choose at each level.

## Scope

In: the model catalog and what it describes, which of it the user allows, the
`<agent>[:<model>]` spelling, effort levels, where a pin may be set (the goal's
orchestrator, each agent a task staffs), and how a running session keeps what
it started on.

Out: how each CLI is handed the choice (007), and how the orchestrator decides
(003).

## Behavior

1. A model is spelled `<agent>[:<model>]`: the agent CLI that runs it, and
   optionally one model of that CLI. The agent alone runs that CLI's own
   default model. A model naming no CLI is a usage error, since nothing says
   which CLI would run it.
2. An effort says how deeply a model reasons, and belongs to the model it runs
   at: a pin naming a model and no effort runs at the CLI's own default, and
   an effort set on its own is run at the model already pinned.
3. The catalog describes each curated model as its agent runs it: tier, a cost
   and a speed band, what task shapes it is and is not a fit for
   (`best_for` / `avoid_for`), and what each of its efforts buys. Each agent is
   also offered on its own default model.
4. A goal carries the orchestrator's pin; every other pin sits on the agent
   the task staffs (017), one per author and per reviewer.
5. A pin is written on the agent when the task is staffed, and a session
   freezes it at its first launch: a re-pin steers the next spawn, never the
   conversation already running.
6. `default` hands a pin back: `--model default` runs the agent CLI's own
   default model, `--effort default` runs it at the CLI's own effort.
7. The orchestrator sizes each agent it staffs from the catalog and the user
   has the last word, on a task that is still `pending` or `ready`.
8. A model is stored as typed, whatever the catalog lists, so a CLI that
   gained a model since the release still runs. An effort, by contrast, is
   checked against the model it would run at.
9. Each entry of the catalog can be turned off, and every model is on until it
   is. The catalog itself is code and discovery, so what is stored is the
   subtraction: an entry the CLIs grow arrives usable, and the exceptions are
   the rows.
10. A model turned off stays in the catalog, marked off, and is out of use: a
    pin naming it is refused, the orchestrator is not offered it at all
    (`list_models`), and the desktop app's picker leaves it out. Where a slot
    is already pinned to it the picker still shows it, since a field has to be
    able to say what it holds.
11. Work already staffed on it is not disturbed. A pin is the snapshot a row
    was created with, so a task keeps running, and an effort moved on its own
    still moves.
12. The last entry left on cannot be turned off: a daemon that can staff
    nothing is not a state to leave a user in.

## Acceptance criteria

- Every curated model is listed as its agent runs it
  (`models.rs::every_curated_model_is_listed_as_its_agent_runs_it`), carries its
  efforts and default (`::a_curated_model_carries_its_efforts_and_its_default`),
  and each agent is offered on its own default model
  (`::each_agent_is_offered_on_its_own_default_model`).
- A goal plans on the pin it was created with
  (`pins.rs::a_goal_created_with_an_agent_and_a_model_plans_on_them`), and a
  task staffs each agent on its own
  (`::a_task_staffs_each_agent_on_its_own_pin`,
  `store.rs::an_agent_is_written_on_the_pin_it_was_given_and_auto_where_it_was_given_none`).
- A session keeps what it started on for every seat
  (`resume.rs::a_resumed_author_stays_on_the_model_its_session_started_on`,
  `::a_running_reviewer_keeps_the_model_its_session_started_on`,
  `::an_orchestrator_respawn_stays_on_the_goals_pin`).
- An agent alone pins with no model of its own
  (`pins.rs::an_agent_alone_pins_it_with_no_model_of_its_own`,
  `resume.rs::a_pin_of_no_model_stays_the_agents_own_default`).
- A model naming no agent is refused by name
  (`pins.rs::a_model_naming_no_agent_is_refused_by_name`), and a model is stored
  as typed (`::a_model_is_stored_as_typed_whatever_the_catalogs_list`).
- An effort is checked against the model it runs at
  (`pins.rs::an_effort_is_checked_against_the_model_it_runs_at`), and an effort
  of its own is run at the model already pinned
  (`::an_effort_of_its_own_is_run_at_the_model_already_pinned`).
- An edit moves the pin and `default` hands it back to auto
  (`pins.rs::an_edit_moves_the_pin_and_default_hands_it_back_to_auto`).
- A model turned off stays in the catalog and out of use
  (`models.rs::a_model_turned_off_stays_in_the_catalog_and_out_of_use`), and
  what is stored is the subtraction from a catalog nothing else holds
  (`store.rs::a_model_is_available_until_it_is_turned_off`).
- It cannot be staffed on, at a goal, an author, a reviewer or an edit, while
  work already on it keeps running
  (`pins.rs::a_model_that_is_turned_off_cannot_be_staffed_on`), and the
  orchestrator is not offered it
  (`tools.rs::the_catalog_an_agent_sees_holds_only_the_models_it_can_be_staffed_on`).
- An id the catalog does not carry cannot be turned off
  (`models.rs::a_model_the_catalog_does_not_carry_cannot_be_turned_off`), and
  neither can the last one left on
  (`::the_last_model_left_on_cannot_be_turned_off`).
- The desktop app lists the catalog with a switch apiece
  (`models-page.test.tsx`), sending the id in the body so one named with a
  slash still travels, and says why where the daemon refuses; the picker
  leaves an entry that is off out unless it is the one pinned
  (`pin-picker.test.tsx::leaves out a model that is turned off, unless it is the one pinned`).

## Sources

`crates/ariadne-core/src/models.rs` (the catalog),
`crates/ariadne-daemon/src/http/catalog.rs`,
`crates/ariadne-daemon/src/http/pins.rs`,
`crates/ariadne-store/src/models.rs`,
`crates/ariadne-store/src/task_agents.rs`,
`ui/src/features/models/`.

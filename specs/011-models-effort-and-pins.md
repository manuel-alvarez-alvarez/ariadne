---
id: models-effort-and-pins
status: current
updated: 2026-09-04
areas: [core, api, daemon, cli]
commits: [090c5158, e94647fd, d94042f4, c42ebeee, 305ad2fb]
tests:
  - crates/ariadne-daemon/tests/models.rs
  - crates/ariadne-daemon/tests/pins.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/adapters.rs
---

# Models, effort and pins

What an agent runs on: which CLI, which model of it, how hard it reasons, and
who gets to choose at each level.

## Scope

In: the model catalog and what it describes, the `<agent>[:<model>]`
spelling, effort levels, where a pin may be set (profile, goal, task,
reviewer slot), and how a pin outlives a profile edit.

Out: how each CLI is handed the choice (007), and how the planner decides
(003).

## Behavior

1. A model is spelled `<agent>[:<model>]`: the agent CLI that runs it, and
   optionally one model of that CLI. The agent alone runs that CLI's own
   default model. A model naming no CLI is a usage error, since nothing says
   which CLI would run it.
2. An effort says how deeply a model reasons, and belongs to the model it runs
   at: a pin naming a model and no effort runs at the CLI's own default, and
   only a pin left on the profile's own model keeps the profile's effort.
3. The catalog describes each curated model as its agent runs it: tier, a cost
   and a speed band, what task shapes it is and is not a fit for
   (`best_for` / `avoid_for`), and what each of its efforts buys. Each agent is
   also offered on its own default model.
4. A profile carries a pin of its own; a goal carries the planner's; a task
   carries its engineer's and one per reviewer slot. A reviewer slot is spelled
   `<profile>[=<model>][@<effort>]`.
5. A pin is written in place of the profile's at creation, so it outlives any
   later edit of that profile: what a session runs on is decided when the work
   is created, not when it starts.
6. `default` hands a pin back — `--model default` to the profile's own,
   `--effort default` to the CLI's own.
7. The planner sizes each slot it assigns from the catalog and the user has the
   last word, on a task that is still `pending` or `ready`.
8. A model is stored as typed, whatever the catalog lists, so a CLI that
   gained a model since the release still runs. An effort, by contrast, is
   checked against the model it would run at.

## Acceptance criteria

- Every curated model is listed as its agent runs it
  (`models.rs::every_curated_model_is_listed_as_its_agent_runs_it`), carries its
  efforts and default (`::a_curated_model_carries_its_efforts_and_its_default`),
  and each agent is offered on its own default model
  (`::each_agent_is_offered_on_its_own_default_model`).
- A task and a goal carry the pins their profiles no longer have
  (`pins.rs::a_task_carries_the_pins_its_profiles_no_longer_have`,
  `::a_goal_carries_the_planner_pin_its_profile_no_longer_has`), and a pin
  outlives a profile edit for every role
  (`resume.rs::an_engineers_pin_outlives_a_profile_edit`,
  `::a_reviewers_pin_outlives_a_profile_edit`,
  `::a_planner_respawn_stays_on_the_goals_pin`).
- An agent alone pins with no model of its own
  (`pins.rs::an_agent_alone_pins_it_with_no_model_of_its_own`,
  `resume.rs::a_pin_of_no_model_stays_the_agents_own_default`).
- A model naming no agent is refused by name
  (`pins.rs::a_model_naming_no_agent_is_refused_by_name`), and a model is stored
  as typed (`::a_model_is_stored_as_typed_whatever_the_catalogs_list`).
- An effort is checked against the model it runs at
  (`pins.rs::an_effort_is_checked_against_the_model_it_runs_at`), moves with the
  model (`::a_task_pins_the_effort_beside_the_model_and_moves_with_it`), and an
  override takes the profile's effort only on the profile's model
  (`store.rs::an_override_takes_the_profiles_effort_only_on_the_profiles_model`).
- An edit moves the pins and `default` hands them back
  (`pins.rs::an_edit_moves_the_pins_and_default_hands_them_back`).

## Sources

`crates/ariadne-core/src/models.rs` (the catalog),
`crates/ariadne-daemon/src/http/catalog.rs`,
`crates/ariadne-daemon/src/http/pins.rs`, `crates/ariadne-store/src/profiles.rs`.

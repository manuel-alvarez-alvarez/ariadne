---
id: models-effort-and-pins
status: current
updated: 2026-09-11
areas: [core, api, daemon, cli]
commits: [090c5158, e94647fd, d94042f4, c42ebeee, 305ad2fb, a69b953f, 03f9c8b7]
tests:
  - crates/ariadne-core/src/models.rs
  - crates/ariadne-daemon/tests/models.rs
  - crates/ariadne-daemon/tests/acp_discovery.rs
  - crates/ariadne-daemon/tests/pins.rs
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - ui/src/features/agents/agents-page.test.tsx
  - ui/src/features/models/model-ref.test.ts
  - ui/src/features/models/pin-picker.test.tsx
---

# Models, effort and pins

What an agent runs on: which registry agent, which model of it, how hard it
reasons, and who gets to choose at each level.

## Scope

In: the ACP agent registry and what discovery reads off each agent, the model
catalog, which of it the user allows, the `<agent>:<model>` spelling, effort
levels, where a pin may be set (the goal's orchestrator, each agent a task
staffs), and how a running session keeps what it started on.

Out: how a launch hands the pin to the agent (007, 021), and how the
orchestrator decides (003).

## Behavior

1. A model is spelled `<agent>:<model>`: the id of an agent in the ACP
   registry, and one model of that agent. The first `:` splits the two, so a
   model id with colons of its own survives whole.
2. Both halves are required. A model is required wherever an agent is
   pinned, and no agent default stands in for one. A bare agent, a string
   with no `:`, an empty agent half, and an empty or whitespace-only model
   half are each refused, and the refusal says a model is required.
3. A request with no model, an empty or whitespace-only one, or the word
   `default` as a model is refused, and the refusal says a model is required.
4. The agent half must name an agent the registry holds. Any other agent is
   refused by name, and the refusal points at the ACP registry.
5. A pin is stored whole, as typed, in the row's `model` column. The launch
   splits it again at the first `:`, and hands the agent the model half.
6. An effort says how deeply a model reasons, and belongs to the model it
   runs at. A pin naming a model and no effort runs at the agent's own
   default. An effort set on its own is run at the model already pinned.
7. `default` is a word for efforts alone: `--effort default` clears the effort
   back to the agent's own. `default` written as a model is refused.
8. An effort is checked against the model it would run at. A model the
   catalog lists takes its listed efforts and nothing else; one that lists
   none takes none. A model the catalog does not list takes any effort that
   is not blank, since only its agent knows the list.
9. An edit that moves the model moves the pin whole: an effort that belonged
   to the model left behind does not travel with it.
10. A goal carries the orchestrator's pin. Every other pin sits on the agent
    the task staffs (017), one per author and per reviewer.
11. A session freezes its pin at its first launch: a re-pin steers the next
    spawn, never the conversation already running.
12. The registry holds three built-in agents — `claude-code-acp` (command
    `claude-code-acp`), `codex-acp` (`codex acp`) and `opencode-acp`
    (`opencode acp`) — plus every entry configured under `acp_agents`. Each
    entry's id is the agent half of every pin on it.
13. A registry id is any word without the `:` delimiter. An id that spells a
    CLI's own name is an agent like any other. The first entry with an id
    keeps it — built-ins first, then configuration order. A later entry with
    a taken id, or an entry whose id carries `:`, stays listed, rejected with
    the reason, and is never resolved.
14. The daemon probes every registry entry at startup and caches the result.
    `POST /v1/acp-agents/refresh` replaces that cache on demand. A probe
    speaks ACP version 1 over standard input and output, creates a session,
    and verifies prompting before it accepts the agent.
15. An agent without version 1, `session/new`, `session/prompt`, or a usable
    model option is rejected with its reason. A missing thought level,
    session listing or session loading marks the agent degraded — no
    efforts, no adoption, or no restart resume respectively.
16. The catalog is what discovery found, and nothing else: no model is listed
    by code. An accepted agent's model option becomes its models, each listed
    as `<agent>:<model>` with `agent_id` beside it; a model option with no
    choices offers the one value it holds. The choices of its thought-level
    option become each model's efforts, in the order offered, with the one it
    runs at flagged as the default. An option whose id or name says model or
    effort counts as one when its category does not.
17. A discovered model carries only what the agent said about it: its
    description, and tier `unknown`. An agent discovery has not accepted
    offers no model. No agent is listed bare.
18. Each entry of the catalog can be turned off, and every model is on until
    it is. What is stored is the subtraction from discovery, so a model an
    agent gains arrives usable.
19. A model turned off stays in the catalog, marked off, and is out of use: a
    pin naming it is refused, the orchestrator is not offered it
    (`list_models`), and the desktop app's picker leaves it out. Where a slot
    is already pinned to it the picker still shows it.
20. Work already staffed on a model turned off is not disturbed: a task keeps
    running, and an effort moved on its own still moves.
21. An id the catalog does not carry cannot be turned off, and the last entry
    left on cannot be turned off either.
22. The desktop app checks the shape of a pin before a submit, and leaves
    which agents exist to the daemon.

## Acceptance criteria

- A model is the agent and then the model, split at the first colon
  (`models.rs (core)::a_model_is_the_agent_and_then_the_model`).
- A model naming no agent, one with no agent before the colon, and one with
  no model after it are each refused
  (`models.rs (core)::a_model_naming_no_agent_is_refused_with_the_form_it_wanted`,
  `::a_colon_with_no_agent_before_it_is_refused`,
  `::a_colon_with_no_model_after_it_is_refused`).
- A request with no model, an empty or whitespace-only one, or `default` as a
  model is refused, and the refusal says a model is required
  (`pins.rs::a_request_with_no_model_is_refused_because_a_model_is_required`).
- A bare agent parses nowhere
  (`pins.rs::a_bare_agent_is_refused_wherever_a_model_is_written`).
- A model naming no agent, or an agent the registry does not hold, is refused
  by name (`pins.rs::a_model_naming_no_agent_is_refused_by_name`).
- A model is stored as typed, and the launch hands the agent its model half
  (`pins.rs::a_model_is_stored_as_typed_whatever_the_catalogs_list`,
  `::a_goal_created_with_a_model_plans_on_it`), and the schema names agents
  by registry id alone
  (`store.rs::the_schema_names_agents_by_registry_id_alone`,
  `::an_agent_is_written_on_the_pin_it_was_given_whole`).
- An effort is checked against the model it runs at
  (`pins.rs::an_effort_is_checked_against_the_model_it_runs_at`,
  `models.rs (core)::a_known_model_takes_its_own_efforts_and_nothing_else`,
  `::a_model_with_no_effort_control_takes_none`), and a model the catalog
  does not list takes any effort but a blank one
  (`models.rs (core)::an_unlisted_model_takes_any_effort_but_a_blank_one`,
  `pins.rs::a_discovered_models_effort_choices_bound_its_pin`).
- An effort of its own is run at the model already pinned, and `default`
  clears it (`pins.rs::an_effort_of_its_own_is_run_at_the_model_already_pinned`).
- An edit moves the pin whole (`pins.rs::an_edit_moves_the_pin_whole`).
- A task staffs each agent on its own pin
  (`pins.rs::a_task_staffs_each_agent_on_its_own_pin`), and a discovered
  catalog id pins agents through the API and reaches the registry command
  (`pins.rs::a_discovered_catalog_id_pins_agents_through_the_api`).
- A session keeps what it started on for every seat
  (`resume.rs::a_resumed_author_stays_on_the_model_its_session_started_on`,
  `::a_running_reviewer_keeps_the_model_its_session_started_on`,
  `::an_orchestrator_respawn_stays_on_the_goals_pin`).
- The registry lists its three built-ins and a configured agent
  (`acp_discovery.rs::the_api_lists_the_three_known_agents_and_one_user_agent`),
  and refreshes its cache on demand (`::discovery_refreshes_on_demand`).
- A registry id that spells a CLI's name is an agent like any other
  (`acp_discovery.rs::a_registry_id_that_spells_a_cli_name_is_an_agent_like_any_other`);
  a taken id is rejected and the first holder keeps the command
  (`::a_registry_id_already_taken_is_rejected`), and so is an id with the
  catalog delimiter (`::a_registry_id_with_the_catalog_delimiter_is_rejected`).
- Discovery rejects every missing required capability
  (`acp_discovery.rs::every_required_acp_capability_is_enforced`,
  `::an_agent_without_a_model_option_is_rejected_and_doctor_shows_why`), and
  every optional gap has its own degradation flag
  (`::every_optional_capability_gap_sets_its_degraded_flag`).
- Every discovered model is listed as its agent runs it, with its efforts and
  default (`models.rs::every_discovered_model_is_listed_as_its_agent_runs_it`,
  `acp_discovery.rs::model_and_effort_name_fallbacks_enter_the_discovered_catalog`),
  a model option with no choices offers its current value
  (`models.rs::a_model_option_with_no_choices_offers_its_current_value`), an
  agent discovery has not accepted offers nothing
  (`::an_agent_discovery_has_not_accepted_offers_no_model`), and no bare-agent
  entry is listed (`::no_bare_agent_entry_is_listed`).
- A model turned off stays in the catalog and out of use
  (`models.rs::a_model_turned_off_stays_in_the_catalog_and_out_of_use`,
  `store.rs::a_model_is_available_until_it_is_turned_off`).
- It cannot be staffed on, while work already on it keeps running
  (`pins.rs::a_model_that_is_turned_off_cannot_be_staffed_on`,
  `::a_discovered_model_turned_off_cannot_be_staffed_on`), and the
  orchestrator is not offered it
  (`tools.rs::the_catalog_an_agent_sees_holds_only_the_models_it_can_be_staffed_on`).
- An id the catalog does not carry cannot be turned off
  (`models.rs::a_model_the_catalog_does_not_carry_cannot_be_turned_off`), and
  neither can the last one left on
  (`::the_last_model_left_on_cannot_be_turned_off`).
- The desktop app lists each agent's models in its tab, sends the id in the
  body, and says why where the daemon refuses
  (`agents-page.test.tsx::puts each model in the tab of the agent that runs it`,
  `::sends the id in the body, so a model named with a slash still travels`,
  `::says why, where the daemon refuses to turn a model off`); the picker
  leaves an entry that is off out unless it is the one pinned, and takes free
  text for a model the catalog does not list
  (`pin-picker.test.tsx::leaves out a model that is turned off, unless it is the one pinned`,
  `::takes free text for a model of a known agent the catalog does not list`);
  and the pin's shape is checked on the field
  (`model-ref.test.ts::refuses one half on its own by showing where the other goes`).

## Sources

`crates/ariadne-core/src/models.rs`,
`crates/ariadne-daemon/src/acp_discovery.rs`,
`crates/ariadne-daemon/src/http/catalog.rs`,
`crates/ariadne-daemon/src/http/pins.rs`,
`crates/ariadne-store/src/models.rs`,
`crates/ariadne-store/src/task_agents.rs`,
`ui/src/features/models/`, `ui/src/features/agents/`.

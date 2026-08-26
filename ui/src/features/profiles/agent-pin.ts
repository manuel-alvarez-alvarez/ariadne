/**
 * The agent a goal or a task pins to one of its slots, as the forms that
 * assign it spell it.
 *
 * A pin is agent-first: what the daemon spawns is an agent CLI, and a model
 * only narrows which model that CLI is asked for — so every slot (the planner,
 * the engineer, each reviewer) chooses an agent and then, optionally, a model
 * scoped to it. Choosing no agent is a choice of its own: the slot runs on
 * whatever its profile does, agent and model both, which is what
 * {@link PROFILE_AGENT_KIND} stands for. It is spelled like the daemon's "put
 * it back on the profile's own" sentinel because on an update that is exactly
 * what it becomes.
 *
 * The profile form has an agent select of its own, with a different unset
 * choice — "auto", the first installed CLI — so it keeps its own vocabulary in
 * `profile-form-values.ts`.
 */

import type { AgentKind } from "@/api"

import { AGENT_KINDS, agentKindLabel } from "./profile-labels"

/** The choice standing for "no pin at all": the profile's own agent and model. */
export const PROFILE_AGENT_KIND = "default"

/** Every value an agent select on a goal or task form offers. */
export const AGENT_PIN_CHOICES = [PROFILE_AGENT_KIND, ...AGENT_KINDS] as const

export type AgentPin = (typeof AGENT_PIN_CHOICES)[number]

/** Those choices as a select's options, in the order the daemon probes the CLIs. */
export const AGENT_PIN_OPTIONS = AGENT_PIN_CHOICES.map((value) => ({
  label: value === PROFILE_AGENT_KIND ? "Profile's own" : agentKindLabel(value),
  value,
}))

/**
 * The CLI a pin names, or undefined where it names none — which is what scopes
 * the model catalog beside it, and what gates the box while nothing is pinned.
 */
export function pinnedAgentKind(pin: AgentPin): AgentKind | undefined {
  return pin === PROFILE_AGENT_KIND ? undefined : pin
}

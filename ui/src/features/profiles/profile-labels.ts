/**
 * How a profile's fields are spelled on screen.
 *
 * The daemon's vocabulary is snake_case (`claude_code`) and two of its fields
 * carry meaning when they are *unset*: no agent kind means "resolve the first
 * installed CLI at spawn time", no model means "whatever that CLI defaults to".
 * Both get a name here — `auto` and `default`, the same words the CLI uses —
 * rather than rendering as an empty cell.
 *
 * The names of the roles and the agent CLIs themselves are not this feature's:
 * they come from {@link import("@/lib/labels")}, which the sessions screens
 * read too.
 */

import type { AgentKind, PromptKind, Role } from "@/api"
import { AGENT_KIND_LABELS, ROLE_LABELS } from "@/lib/labels"

/** Roles, in the order the orchestration runs them. */
export const ROLES = [
  "planner",
  "engineer",
  "reviewer",
  "integrator",
] as const satisfies readonly Role[]

/** Agent CLIs, in the order the daemon probes them when resolving `auto`. */
export const AGENT_KINDS = [
  "claude_code",
  "codex",
  "opencode",
] as const satisfies readonly AgentKind[]

/** Shown where a profile has no agent kind pinned. */
export const AUTO_AGENT_LABEL = "auto"
/** Shown where a profile has no model pinned. */
export const DEFAULT_MODEL_LABEL = "default"

export function roleLabel(role: Role): string {
  return ROLE_LABELS[role]
}

export function agentKindLabel(kind: AgentKind | null | undefined): string {
  return kind ? AGENT_KIND_LABELS[kind] : AUTO_AGENT_LABEL
}

export function modelLabel(model: string | null | undefined): string {
  return model ? model : DEFAULT_MODEL_LABEL
}

/**
 * The briefing prompts, named for the screen.
 *
 * A total record over the generated enum, so a prompt kind added to the daemon
 * fails to compile here until it is given a name. Which kinds a profile
 * actually has is the daemon's answer, not this map's: `GET
 * /v1/profiles/{id}/prompts` returns exactly the ones its role owns.
 */
export const PROMPT_KIND_LABELS: Record<PromptKind, string> = {
  planner_briefing: "Planner briefing",
  planner_resume: "Planner resume",
  engineer_briefing: "Engineer briefing",
  engineer_resume: "Engineer resume",
  changes_requested: "Changes requested",
  reviewer_briefing: "Reviewer briefing",
  reviewer_resume: "Reviewer resume",
  integration_instructions: "Integration instructions",
  integration_resume: "Integration resume",
  integration_merged: "Integration merged",
  message_delivery: "Message delivery",
}

/**
 * Every briefing kind, in the order above.
 *
 * The labels map is what proves this list total; this one exists because a zod
 * enum needs a tuple, not a record's keys.
 */
export const PROMPT_KINDS = [
  "planner_briefing",
  "planner_resume",
  "engineer_briefing",
  "engineer_resume",
  "changes_requested",
  "reviewer_briefing",
  "reviewer_resume",
  "integration_instructions",
  "integration_resume",
  "integration_merged",
  "message_delivery",
] as const satisfies readonly PromptKind[]

/** When each briefing is sent, one line under the editor that holds it. */
export const PROMPT_KIND_HINTS: Record<PromptKind, string> = {
  planner_briefing: "Starts the planner on a goal.",
  planner_resume: "Nudges the planner when the goal stops moving.",
  engineer_briefing: "Starts the engineer on a task.",
  engineer_resume: "Picks the engineer up again: a session that ended, or one gone quiet.",
  changes_requested:
    "Resumes the engineer with a round of requested changes, from the reviewers or from a published request.",
  reviewer_briefing: "Starts a reviewer on a task under review.",
  reviewer_resume: "Picks a reviewer up again: a new round, or one it has gone quiet in.",
  integration_instructions: "Starts the integrator on a task its reviewers approved.",
  integration_resume:
    "Picks the integrator up again: a task still to land, or a published request to push the revision to.",
  integration_merged: "Wakes the integrator once a human has merged the published request.",
  message_delivery: "Wakes any agent with a message addressed to it.",
}

export function promptKindLabel(kind: PromptKind): string {
  return PROMPT_KIND_LABELS[kind]
}

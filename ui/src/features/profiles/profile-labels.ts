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
  engineer_briefing: "Engineer briefing",
  changes_requested: "Changes requested",
  merge_instructions: "Merge instructions",
  reviewer_briefing: "Reviewer briefing",
  reviewer_resume: "Reviewer resume",
}

/**
 * Every briefing kind, in the order above.
 *
 * The labels map is what proves this list total; this one exists because a zod
 * enum needs a tuple, not a record's keys.
 */
export const PROMPT_KINDS = [
  "planner_briefing",
  "engineer_briefing",
  "changes_requested",
  "merge_instructions",
  "reviewer_briefing",
  "reviewer_resume",
] as const satisfies readonly PromptKind[]

/** When each briefing is sent, one line under the editor that holds it. */
export const PROMPT_KIND_HINTS: Record<PromptKind, string> = {
  planner_briefing: "Starts the planner on a goal.",
  engineer_briefing: "Starts the engineer on a task.",
  changes_requested: "Resumes the engineer with a reviewer's requested changes.",
  merge_instructions: "Resumes the engineer once the task has enough approvals.",
  reviewer_briefing: "Starts a reviewer on a task under review.",
  reviewer_resume: "Resumes a reviewer after the engineer pushed changes.",
}

export function promptKindLabel(kind: PromptKind): string {
  return PROMPT_KIND_LABELS[kind]
}

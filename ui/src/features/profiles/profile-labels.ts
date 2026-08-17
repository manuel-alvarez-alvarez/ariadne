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

import type { AgentKind, Role } from "@/api"
import { AGENT_KIND_LABELS, ROLE_LABELS } from "@/lib/labels"

/** Roles, in the order the orchestration runs them. */
export const ROLES = ["planner", "engineer", "reviewer"] as const satisfies readonly Role[]

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

/**
 * How a profile's fields are spelled on screen.
 *
 * The daemon's vocabulary is snake_case (`claude_code`) and two of its fields
 * carry meaning when they are *unset*: no agent kind means "resolve the first
 * installed CLI at spawn time", no model means "whatever that CLI defaults to".
 * Both get a name here — `auto` and `default`, the same words the CLI uses —
 * rather than rendering as an empty cell.
 *
 * `ROLE_LABELS` and `AGENT_KIND_LABELS` are total records over the generated
 * enums, so a new role or agent CLI in the daemon fails to compile here until
 * it is named.
 */

import type { AgentKind, Role } from "@/api"

/** Roles, in the order the orchestration runs them. */
export const ROLES = ["planner", "engineer", "reviewer"] as const satisfies readonly Role[]

/** Agent CLIs, in the order the daemon probes them when resolving `auto`. */
export const AGENT_KINDS = [
  "claude_code",
  "codex",
  "opencode",
] as const satisfies readonly AgentKind[]

export const ROLE_LABELS: Record<Role, string> = {
  planner: "Planner",
  engineer: "Engineer",
  reviewer: "Reviewer",
}

export const AGENT_KIND_LABELS: Record<AgentKind, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
}

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

const TIMESTAMP_FORMAT = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
})

/**
 * The daemon's RFC 3339 timestamps in the viewer's locale and time zone. An
 * unparsable value is passed through rather than shown as "Invalid Date".
 */
export function formatTimestamp(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : TIMESTAMP_FORMAT.format(date)
}

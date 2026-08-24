/**
 * The daemon's vocabulary, spelled for the screen.
 *
 * Roles and agent kinds come off the wire in snake_case and are read in three
 * unrelated places — the profiles table, the session lists, the message
 * threads — which is how the same two maps came to be written three times.
 * They live here now, next to `@/lib/time` and `@/lib/ids`, for the reason
 * those do: this is app-wide vocabulary, not any one feature's.
 *
 * Both maps are total records over the generated enums, so a new role or agent
 * CLI in the daemon fails to compile here until it is given a name.
 */

import type { AgentKind, AuthorRole, Role } from "@/api"

export const ROLE_LABELS: Record<Role, string> = {
  planner: "Planner",
  engineer: "Engineer",
  reviewer: "Reviewer",
  integrator: "Integrator",
}

/**
 * A message author is a role plus the three speakers that run no session: the
 * person at the keyboard, the daemon itself, and the forge whose reviewers it
 * relays off a published pull or merge request.
 */
export const AUTHOR_ROLE_LABELS: Record<AuthorRole, string> = {
  ...ROLE_LABELS,
  user: "You",
  system: "System",
  forge: "Forge",
}

export const AGENT_KIND_LABELS: Record<AgentKind, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
}

/**
 * How a session is spelled out on screen: the labels and the status badge.
 *
 * The label maps are declared as total records over the generated enums, so a
 * new role, agent kind or status in the daemon fails to compile here until it
 * is given a label. Timestamps come from `@/lib/time`, which every feature
 * shares.
 */

import type { AgentKind, Role, SessionStatus } from "@/api"
import { StatusBadge } from "@/components/status-badge"

/**
 * Intentionally exported: the session filters of the navigation work being
 * done in parallel read them, even though nothing in this file does.
 */
export const SESSION_STATUSES: readonly SessionStatus[] = [
  "starting",
  "running",
  "idle",
  "exited",
  "failed",
]

/** Intentionally exported, for the same reason as {@link SESSION_STATUSES}. */
export const ROLES: readonly Role[] = ["planner", "engineer", "reviewer"]

/**
 * Mirrors `SessionStatus::is_live` in `ariadne-core`: a session with a tmux
 * pane that may still produce output. Everything the UI treats as "live" —
 * the pulsing badge, the kill action, the terminal expecting more — keys off
 * this and nothing else.
 */
export function isLiveStatus(status: SessionStatus): boolean {
  return status === "starting" || status === "running" || status === "idle"
}

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

interface SessionStatusMeta {
  label: string
  /** Dot colour; the live statuses are the saturated end of the scale. */
  dot: string
}

export const SESSION_STATUS_META: Record<SessionStatus, SessionStatusMeta> = {
  starting: { label: "Starting", dot: "bg-amber-500" },
  running: { label: "Running", dot: "bg-emerald-500" },
  idle: { label: "Idle", dot: "bg-sky-500" },
  exited: { label: "Exited", dot: "bg-muted-foreground/60" },
  failed: { label: "Failed", dot: "bg-destructive" },
}

export function sessionStatusLabel(status: SessionStatus): string {
  return SESSION_STATUS_META[status].label
}

export function SessionStatusBadge({
  status,
  className,
}: {
  status: SessionStatus
  className?: string
}) {
  const live = isLiveStatus(status)
  const meta = SESSION_STATUS_META[status]
  return (
    <StatusBadge
      box="outlined"
      label={meta.label}
      tone={live ? "text-foreground" : "text-muted-foreground"}
      dot={meta.dot}
      pulse={live}
      className={className}
    />
  )
}

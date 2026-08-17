/**
 * How a session is spelled out on screen: the labels and the status badge.
 *
 * The status labels are declared as a total record over the generated enum, so
 * a new status in the daemon fails to compile here until it is given one. The
 * role and agent-kind names are app-wide rather than this feature's and come
 * from `@/lib/labels`, the way timestamps come from `@/lib/time`.
 */

import type { Role, SessionStatus } from "@/api"
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

interface SessionStatusMeta {
  label: string
  /**
   * Dot colour, from the status ramp in `index.css`; the live statuses are the
   * saturated end of the scale. An idle session is the one waiting on you, so
   * it takes the accent rather than a colour it would share with a warning.
   */
  dot: string
}

export const SESSION_STATUS_META: Record<SessionStatus, SessionStatusMeta> = {
  starting: { label: "Starting", dot: "bg-status-ready" },
  running: { label: "Running", dot: "bg-status-done" },
  idle: { label: "Idle", dot: "bg-status-active" },
  exited: { label: "Exited", dot: "bg-muted-foreground/60" },
  failed: { label: "Failed", dot: "bg-status-danger" },
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

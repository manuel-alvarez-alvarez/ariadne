/**
 * How a session is spelled out on screen: labels, the status badge, and the
 * timestamp formatting the list and the detail view share.
 *
 * The label maps are declared as total records over the generated enums, so a
 * new role, agent kind or status in the daemon fails to compile here until it
 * is given a label.
 */

import type { AgentKind, Role, SessionStatus } from "@/api"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"

export const SESSION_STATUSES: readonly SessionStatus[] = [
  "starting",
  "running",
  "idle",
  "exited",
  "failed",
]

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

/** Dot colour per status; the live ones are the saturated end of the scale. */
const STATUS_TONE: Record<SessionStatus, string> = {
  starting: "bg-amber-500",
  running: "bg-emerald-500",
  idle: "bg-sky-500",
  exited: "bg-muted-foreground/60",
  failed: "bg-destructive",
}

export function SessionStatusBadge({
  status,
  className,
}: {
  status: SessionStatus
  className?: string
}) {
  const live = isLiveStatus(status)
  return (
    <Badge variant="outline" className={cn("gap-1.5", !live && "text-muted-foreground", className)}>
      <span
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          STATUS_TONE[status],
          live && "animate-pulse",
        )}
        aria-hidden
      />
      {status}
    </Badge>
  )
}

/** Absolute local time, for tooltips and the metadata table. */
export function formatTimestamp(iso: string | null | undefined): string {
  if (!iso) return "—"
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleString()
}

/**
 * Compact age, e.g. `12s`, `4m`, `3h`, `2d`. Pass `now` from
 * {@link import("./use-now").useNow} so it refreshes on its own.
 */
export function formatAge(iso: string | null | undefined, now: number): string {
  if (!iso) return "—"
  const at = new Date(iso).getTime()
  if (Number.isNaN(at)) return iso
  const seconds = Math.max(0, Math.round((now - at) / 1000))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  return `${Math.round(hours / 24)}d`
}

/**
 * How a session is spelled out on screen: the labels, the status badge, and
 * the badge for why it is waiting on a person.
 *
 * The status labels are declared as a total record over the generated enum, so
 * a new status in the daemon fails to compile here until it is given one — and
 * {@link SESSION_ATTENTION_META} is the same total record over the attention
 * reasons. Both are declared here rather than at their call sites because the
 * reason is shown in four places (the board's attention strip, the sessions
 * table, the session panel, and the CLI's `ariadne attention`, which mirrors
 * this wording) and they have to agree. The role and agent-kind names are
 * app-wide rather than this feature's and come from `@/lib/labels`, the way
 * timestamps come from `@/lib/time`.
 */

import type { AttentionReason, SessionDto, SessionStatus } from "@/api"
import { StatusBadge } from "@/components/status-badge"

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

/**
 * Why a session is waiting on a person: the `attention_reason` the daemon
 * raised for it, and nothing else. Named rather than used bare because the
 * attention strip has task reasons of its own under the same word.
 */
export type SessionAttention = AttentionReason

interface SessionAttentionMeta {
  label: string
  /** What the reason means; shown on hover and on focus. */
  hint: string
  /** Badge classes, from the status ramp in `index.css`: it carries dark mode. */
  badge: string
}

/**
 * The warm half of the ramp throughout — every one of these is something gone
 * wrong or something waiting — with the one that ended the agent's work
 * (`agent_error`) and the one that lost it (`disconnected`) on the danger
 * step, and the three it can still be talked out of on the warn step.
 *
 * `crates/ariadne-cli/src/commands/attention.rs` spells the same reasons for
 * the terminal; the wording there is these labels, lowercased.
 */
export const SESSION_ATTENTION_META: Record<SessionAttention, SessionAttentionMeta> = {
  waiting_permission: {
    label: "Waiting for permission",
    hint: "The agent is blocked on a permission or approval prompt.",
    badge: "bg-status-warn-soft text-status-warn-fg",
  },
  waiting_input: {
    label: "Waiting for input",
    hint: "The agent asked a question and is idle until it is answered.",
    badge: "bg-status-warn-soft text-status-warn-fg",
  },
  agent_error: {
    label: "Agent error",
    hint: "The agent reported an error.",
    badge: "bg-status-danger-soft text-status-danger-fg",
  },
  disconnected: {
    label: "Disconnected",
    hint: "The agent's terminal is gone while its work is still active.",
    badge: "bg-status-danger-soft text-status-danger-fg",
  },
  stalled: {
    label: "Stalled",
    hint: "No activity for too long.",
    badge: "bg-status-warn-soft text-status-warn-fg",
  },
}

/**
 * Why a session wants the user, and nothing when it does not.
 *
 * The stored reason is the whole rule. A dead session raises no reason of its
 * own on purpose: the daemon flags the agent it still owes work to as
 * `disconnected` and leaves the rest alone, so a reviewer that exited after
 * voting is finished, not stuck, and reading `status` here would put it back
 * on the list the daemon kept it off. Kept identical in `attention.rs`.
 */
export function sessionAttention(session: SessionDto): SessionAttention | null {
  return session.attention_reason ?? null
}

/**
 * The reason pill. Deliberately separate from {@link SessionStatusBadge}: a
 * session blocked on a permission prompt is still *running*, so the two say
 * different things and are shown side by side.
 */
export function SessionAttentionBadge({
  attention,
  className,
}: {
  attention: SessionAttention
  className?: string
}) {
  const meta = SESSION_ATTENTION_META[attention]
  return (
    <StatusBadge
      box="badge"
      label={meta.label}
      tone={meta.badge}
      hint={meta.hint}
      className={className}
    />
  )
}

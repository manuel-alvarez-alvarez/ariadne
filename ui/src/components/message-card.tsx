/**
 * One message of a conversation, wherever a conversation is shown.
 *
 * The goal thread and the task thread render the same `MessageDto` and were
 * drawing two different cards for it, each with its own role→colour map. This
 * is the task thread's anatomy, which is the denser of the two: a role pill, a
 * timestamp pushed to the right, and the body as markdown under them.
 *
 * One tint per role, from the status ramp in `index.css`: the planner keeps
 * the violet of a goal in planning, the engineer the accent of work in
 * progress, the reviewer the teal that the warm steps of the ramp are reserved
 * against — never those warm steps, which mean something is wrong.
 */

import type { ReactNode } from "react"

import type { AuthorRole, MessageDto } from "@/api"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { AUTHOR_ROLE_LABELS } from "@/lib/labels"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"

const ROLE_TONES: Record<AuthorRole, { badge: string; card: string }> = {
  planner: {
    badge: "bg-status-review-soft text-status-review-fg",
    card: "border-status-review/25",
  },
  engineer: {
    badge: "bg-status-active-soft text-status-active-fg",
    card: "border-status-active/25",
  },
  reviewer: {
    badge: "bg-status-ready-soft text-status-ready-fg",
    card: "border-status-ready/25",
  },
  user: {
    badge: "bg-foreground/10 text-foreground",
    card: "border-foreground/25",
  },
  system: {
    badge: "bg-muted text-muted-foreground",
    card: "border-border",
  },
}

export function MessageCard({
  message,
  source,
}: {
  message: MessageDto
  /**
   * More about who spoke, next to the role pill: both threads link the
   * session that posted the message, when there is one — the user's own
   * messages come from no session.
   */
  source?: ReactNode
}) {
  const role = ROLE_TONES[message.author_role]
  return (
    <article className={cn("rounded-lg border border-l-2 bg-card px-3 py-2", role.card)}>
      <header className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
        <StatusBadge size="sm" label={AUTHOR_ROLE_LABELS[message.author_role]} tone={role.badge} />
        {source}
        <time
          className="ml-auto text-muted-foreground"
          dateTime={message.created_at}
          title={formatAbsolute(message.created_at)}
        >
          {formatRelative(message.created_at)}
        </time>
      </header>
      <Markdown>{message.body}</Markdown>
    </article>
  )
}

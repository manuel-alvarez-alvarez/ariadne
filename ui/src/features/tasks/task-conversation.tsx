/**
 * The task thread, read-only — the `task messages` equivalent: what the
 * planner, engineer and reviewers said while working the task. New messages
 * arrive through the event stream: `message_created` invalidates this query,
 * so a message an agent posts shows up without a refresh.
 */

import { useQuery } from "@tanstack/react-query"

import type { AuthorRole, MessageDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { Skeleton } from "@/components/ui/skeleton"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { taskMessagesQueryOptions } from "./queries"
import { SessionLink } from "./task-sessions"

/**
 * One tint per role, from the status ramp in `index.css` and the same one the
 * goal thread gives that role: the planner violet, the engineer the accent,
 * the reviewer teal — never the warm steps, which mean something is wrong.
 */
const ROLE_META: Record<AuthorRole, { label: string; badge: string; card: string }> = {
  planner: {
    label: "Planner",
    badge: "bg-status-review-soft text-status-review-fg",
    card: "border-status-review/25",
  },
  engineer: {
    label: "Engineer",
    badge: "bg-status-active-soft text-status-active-fg",
    card: "border-status-active/25",
  },
  reviewer: {
    label: "Reviewer",
    badge: "bg-status-ready-soft text-status-ready-fg",
    card: "border-status-ready/25",
  },
  user: {
    label: "You",
    badge: "bg-foreground/10 text-foreground",
    card: "border-foreground/25",
  },
  system: {
    label: "System",
    badge: "bg-muted text-muted-foreground",
    card: "border-border",
  },
}

export function TaskConversation({ taskId }: { taskId: string }) {
  const messages = useQuery(taskMessagesQueryOptions(taskId))

  if (messages.isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-16 w-full" />
      </div>
    )
  }

  if (messages.error) {
    return (
      <ErrorState
        title="Could not load the conversation"
        error={messages.error}
        onRetry={() => void messages.refetch()}
      />
    )
  }

  if (messages.data.length === 0) {
    return <EmptyState emphasis="quiet" title="Nothing has been said on this task yet." />
  }

  return (
    <ol className="space-y-3">
      {messages.data.map((message) => (
        <li key={message.id}>
          <MessageCard message={message} />
        </li>
      ))}
    </ol>
  )
}

function MessageCard({ message }: { message: MessageDto }) {
  const role = ROLE_META[message.author_role]
  return (
    <article className={cn("rounded-lg border border-l-2 bg-card px-3 py-2", role.card)}>
      <header className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
        <span className={cn("rounded-full px-1.5 py-0.5 font-medium", role.badge)}>
          {role.label}
        </span>
        {message.author_session_id && <SessionLink sessionId={message.author_session_id} />}
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

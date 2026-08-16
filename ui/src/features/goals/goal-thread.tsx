/**
 * The goal-level conversation, read-only: what the planner, the agents and
 * the daemon said about this goal. New messages arrive over the event stream
 * (`message_created` invalidates this exact key), so the thread appends
 * itself without polling.
 */

import { useQuery } from "@tanstack/react-query"

import type { AuthorRole, MessageDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { goalMessagesQueryOptions } from "./queries"

const ROLE_LABELS: Record<AuthorRole, string> = {
  planner: "Planner",
  engineer: "Engineer",
  reviewer: "Reviewer",
  user: "You",
  system: "System",
}

/**
 * Who said it has to be readable at a glance, so each role gets its own tint,
 * from the status ramp: the planner keeps the violet of a goal in planning,
 * the engineer the accent of work in progress, the reviewer the teal that the
 * warm steps of the ramp are reserved against. The ramp's tints carry dark
 * mode, taken down to a fraction here because these back a whole row of prose.
 */
const ROLE_CLASSES: Record<AuthorRole, string> = {
  planner: "border-l-status-review/60 bg-status-review-soft/35",
  engineer: "border-l-status-active/60 bg-status-active-soft/35",
  reviewer: "border-l-status-ready/60 bg-status-ready-soft/35",
  user: "border-l-foreground/50 bg-foreground/[0.04]",
  system: "border-l-border bg-muted/40",
}

export function GoalThread({ goalId, className }: { goalId: string; className?: string }) {
  const messages = useQuery(goalMessagesQueryOptions(goalId))

  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>Planner thread</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {messages.error ? (
          <ErrorState
            showIcon
            title="Could not load the conversation"
            error={messages.error}
            onRetry={() => void messages.refetch()}
          />
        ) : null}

        {messages.isPending ? (
          <div className="flex flex-col gap-2">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ) : null}

        {messages.data ? <MessageList messages={messages.data} /> : null}
      </CardContent>
    </Card>
  )
}

function MessageList({ messages }: { messages: MessageDto[] }) {
  if (messages.length === 0) {
    return <EmptyState emphasis="quiet" title="Nothing in the thread yet." />
  }

  // The list does not scroll on its own: it grows, and the panel around it
  // is the one scroll container.
  return (
    <div className="flex flex-col gap-3">
      {messages.map((message) => (
        <article
          key={message.id}
          className={cn("rounded-md border-l-2 px-3 py-2", ROLE_CLASSES[message.author_role])}
        >
          <header className="flex items-baseline justify-between gap-2">
            <span className="text-sm font-medium">{ROLE_LABELS[message.author_role]}</span>
            <time
              className="text-xs text-muted-foreground"
              dateTime={message.created_at}
              title={formatAbsolute(message.created_at)}
            >
              {formatRelative(message.created_at)}
            </time>
          </header>
          <Markdown className="mt-1">{message.body}</Markdown>
        </article>
      ))}
    </div>
  )
}

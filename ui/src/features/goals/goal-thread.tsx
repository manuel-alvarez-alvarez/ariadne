/**
 * The goal-level conversation, read-only: what the planner, the agents and
 * the daemon said about this goal. New messages arrive over the event stream
 * (`message_created` invalidates this exact key), so the thread appends
 * itself without polling.
 *
 * Each message is the shared {@link MessageCard}, the same one the task thread
 * draws; what is this surface's own is the card the thread sits in.
 */

import { useQuery } from "@tanstack/react-query"

import type { MessageDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { MessageCard } from "@/components/message-card"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { goalMessagesQueryOptions } from "./queries"

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
            title="Could not load conversation"
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
    return <EmptyState emphasis="quiet" title="Nothing in the thread yet" />
  }

  // The list does not scroll on its own: it grows, and the panel around it
  // is the one scroll container.
  return (
    <div className="flex flex-col gap-3">
      {messages.map((message) => (
        <MessageCard key={message.id} message={message} />
      ))}
    </div>
  )
}

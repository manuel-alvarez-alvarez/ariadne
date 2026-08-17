/**
 * The task thread, read-only — the `task messages` equivalent: what the
 * planner, engineer and reviewers said while working the task. New messages
 * arrive through the event stream: `message_created` invalidates this query,
 * so a message an agent posts shows up without a refresh.
 *
 * The card each message is drawn as is shared with the goal thread; what this
 * surface adds to it is the link to the session that posted the message.
 */

import { useQuery } from "@tanstack/react-query"

import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { MessageCard } from "@/components/message-card"
import { Skeleton } from "@/components/ui/skeleton"
import { taskMessagesQueryOptions } from "./queries"
import { SessionLink } from "./task-sessions"

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
        title="Could not load conversation"
        error={messages.error}
        onRetry={() => void messages.refetch()}
      />
    )
  }

  if (messages.data.length === 0) {
    return <EmptyState emphasis="quiet" title="Nothing has been said on this task yet" />
  }

  return (
    <ol className="space-y-3">
      {messages.data.map((message) => (
        <li key={message.id}>
          <MessageCard
            message={message}
            source={
              message.author_session_id ? (
                <SessionLink sessionId={message.author_session_id} />
              ) : null
            }
          />
        </li>
      ))}
    </ol>
  )
}

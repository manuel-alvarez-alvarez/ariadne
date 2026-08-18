/**
 * The task thread — the `task messages` equivalent: what the planner, engineer
 * and reviewers said while working the task, with a compose box under it, the
 * `task msg` half. New messages arrive through the event stream:
 * `message_created` invalidates this query, so a message an agent posts shows
 * up without a refresh; a sent one is appended by its own mutation and does
 * not wait for the event.
 *
 * The compose box is there whatever the task's status: the daemon takes a
 * post on a terminal task too (`http/tasks.rs post_message` checks only that
 * the task exists), where it waits in the thread like any `task msg` would.
 *
 * The card each message is drawn as is shared with the goal thread; what this
 * surface adds to it is the link to the session that posted the message.
 */

import { useQuery } from "@tanstack/react-query"

import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { MessageCard } from "@/components/message-card"
import { MessageComposer } from "@/components/message-composer"
import { Skeleton } from "@/components/ui/skeleton"
import { taskMessagesQueryOptions, usePostTaskMessage } from "./queries"
import { SessionLink } from "./task-sessions"

export function TaskConversation({ taskId }: { taskId: string }) {
  const messages = useQuery(taskMessagesQueryOptions(taskId))
  const post = usePostTaskMessage(taskId)

  return (
    <div className="flex flex-col gap-3">
      {messages.isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      ) : null}

      {messages.error ? (
        <ErrorState
          title="Could not load conversation"
          error={messages.error}
          onRetry={() => void messages.refetch()}
        />
      ) : null}

      {messages.data ? (
        messages.data.length === 0 ? (
          <EmptyState emphasis="quiet" title="Nothing has been said on this task yet" />
        ) : (
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
      ) : null}

      <MessageComposer
        post={post}
        label="Message the task conversation"
        placeholder="Write to the agents on this task…"
      />
    </div>
  )
}

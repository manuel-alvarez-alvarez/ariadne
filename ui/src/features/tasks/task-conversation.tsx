/**
 * The task thread — the `task thread` equivalent: what the planner, engineer
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
 * The compose box may address anyone working the task — its engineer, its
 * reviewers and the planner that wrote it —
 * which is the set the daemon
 * resolves `to` against (`http/recipients.rs`), read off the task and its goal
 * so the picker offers nobody the daemon would refuse.
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
import { goalQueryOptions } from "@/features/goals/queries"
import { useAddressees } from "@/features/profiles/addressees"
import { taskMessagesQueryOptions, taskQueryOptions, usePostTaskMessage } from "./queries"
import { SessionLink } from "./task-sessions"

export function TaskConversation({ taskId }: { taskId: string }) {
  const messages = useQuery(taskMessagesQueryOptions(taskId))
  const task = useQuery(taskQueryOptions(taskId))
  // The planner is the task's goal's, so who may be addressed is known only
  // once the task itself is: until then the picker is one name shorter.
  const goal = useQuery({
    ...goalQueryOptions(task.data?.goal_id ?? ""),
    enabled: Boolean(task.data),
  })
  const post = usePostTaskMessage(taskId)
  // The daemon's own list, in its order (`http/recipients.rs`): engineer,
  // reviewers, the planner that wrote it.
  const addressees = useAddressees([
    ...(task.data
      ? [task.data.engineer_profile_id, ...task.data.reviewers.map((slot) => slot.profile_id)]
      : []),
    ...(goal.data ? [goal.data.planner_profile_id] : []),
  ])

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
        addressees={addressees}
      />
    </div>
  )
}

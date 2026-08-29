/**
 * The task thread — the `task thread` equivalent: what the planner, engineer
 * and reviewers said while working the task, with a compose box under it, the
 * `task msg` half. New messages arrive through the event stream:
 * `message_created` invalidates this query, so a message an agent posts shows
 * up without a refresh; a sent one is appended by its own mutation and does
 * not wait for the event.
 *
 * The compose box may address anyone working the task — its engineer, its
 * reviewers and the planner that wrote it — which is the set the daemon
 * resolves `to` against (`http/recipients.rs`), read off the task and its goal
 * so the picker offers nobody the daemon would refuse. On a task that is over
 * nobody is working it any more, and the box says that instead of taking a
 * message no session will be started to read.
 *
 * How the thread itself behaves — opening on its newest message, following it,
 * counting what arrives while the reader is further up — is {@link ThreadView},
 * which the goal thread draws too; what this surface adds is the link from a
 * message to the session that posted it.
 */

import { useQuery } from "@tanstack/react-query"

import { ErrorState } from "@/components/error-state"
import { ThreadView } from "@/components/thread-view"
import { Skeleton } from "@/components/ui/skeleton"
import { goalQueryOptions } from "@/features/goals/queries"
import { useAddressees } from "@/features/profiles/addressees"
import { useComposerRequest } from "@/routes/paths"
import { taskMessagesQueryOptions, taskQueryOptions, usePostTaskMessage } from "./queries"
import { isTerminalTaskStatus, TASK_STATUS_META } from "./status"
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
  // Set when the panel was opened to answer an agent that is waiting on the
  // user — from the attention list, which knows which one asked.
  const opened = useComposerRequest()
  // The daemon's own list, in its order (`http/recipients.rs`): engineer,
  // reviewers, the planner that wrote it.
  const addressees = useAddressees([
    ...(task.data
      ? [task.data.engineer_profile_id, ...task.data.reviewers.map((slot) => slot.profile_id)]
      : []),
    ...(goal.data ? [goal.data.planner_profile_id] : []),
  ])
  const closed =
    task.data && isTerminalTaskStatus(task.data.status)
      ? `${TASK_STATUS_META[task.data.status].label}: no agent is left to read this.`
      : undefined

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

      <ThreadView
        threadKey={`task:${taskId}`}
        messages={messages.data}
        post={post}
        label="Message the task conversation"
        placeholder="Write to the agents on this task…"
        addressees={addressees}
        autoFocus={opened.focus}
        presetTo={opened.to}
        emptyTitle="Nothing has been said on this task yet"
        closedHint={closed}
        source={(message) =>
          message.author_session_id ? <SessionLink sessionId={message.author_session_id} /> : null
        }
      />
    </div>
  )
}

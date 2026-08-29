/**
 * The goal-level conversation: what the planner, the agents and the daemon
 * said about this goal, with a compose box under it for the user's side. New
 * messages arrive over the event stream (`message_created` invalidates this
 * exact key), so the thread appends itself without polling; a sent one is
 * appended by its own mutation and does not wait for the event.
 *
 * The compose box may address the goal's planner, and only it: that is who
 * works in this thread (`http/recipients.rs`), and the engineers and reviewers
 * are addressed in the task threads they work in instead. On a goal that is
 * over there is no planner left to address at all, and the box says so rather
 * than taking a message nothing will ever read.
 *
 * How the thread itself behaves — opening on its newest message, following it,
 * counting what arrives while the reader is further up — is {@link ThreadView},
 * which the task thread draws too. What is this surface's own is the card the
 * thread sits in, and the link from a message to the session that said it.
 */

import { useQuery } from "@tanstack/react-query"

import { ErrorState } from "@/components/error-state"
import { ThreadView } from "@/components/thread-view"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { useAddressees } from "@/features/profiles/addressees"
import { SessionLink } from "@/features/tasks/task-sessions"
import { useComposerRequest } from "@/routes/paths"
import { goalMessagesQueryOptions, goalQueryOptions, usePostGoalMessage } from "./queries"
import { GOAL_STATUS_META, isTerminalGoalStatus } from "./status"

export function GoalThread({ goalId, className }: { goalId: string; className?: string }) {
  const messages = useQuery(goalMessagesQueryOptions(goalId))
  const goal = useQuery(goalQueryOptions(goalId))
  const post = usePostGoalMessage(goalId)
  // Set when the panel was opened to answer the planner, which is what a
  // `waiting_user` planner session's row on the attention list does.
  const opened = useComposerRequest()
  const addressees = useAddressees(goal.data ? [goal.data.planner_profile_id] : [])
  const closed =
    goal.data && isTerminalGoalStatus(goal.data.status)
      ? `${GOAL_STATUS_META[goal.data.status].label}: no planner is left to read this.`
      : undefined

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

        <ThreadView
          threadKey={`goal:${goalId}`}
          messages={messages.data}
          post={post}
          label="Message the planner thread"
          placeholder="Write to the planner thread…"
          addressees={addressees}
          autoFocus={opened.focus}
          presetTo={opened.to}
          emptyTitle="Nothing in the thread yet"
          closedHint={closed}
          // The user's own messages come from no session; everything else was
          // said by an agent this links to.
          source={(message) =>
            message.author_session_id ? <SessionLink sessionId={message.author_session_id} /> : null
          }
          // The thread lives inside the card, so it is the card the box has to
          // cover as it scrolls over the messages.
          surface="card"
        />
      </CardContent>
    </Card>
  )
}

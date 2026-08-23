/**
 * The goal-level conversation: what the planner, the agents and the daemon
 * said about this goal, with a compose box under it for the user's side. New
 * messages arrive over the event stream (`message_created` invalidates this
 * exact key), so the thread appends itself without polling; a sent one is
 * appended by its own mutation and does not wait for the event.
 *
 * The compose box is there whatever the goal's status: the daemon takes a
 * post on a terminal goal too (`http/goals.rs post_message` checks only that
 * the goal exists), where it waits in the thread like any message would.
 *
 * The compose box may address the goal's planner, and only it: that is who
 * works in this thread (`http/recipients.rs`), and the engineers and reviewers
 * are addressed in the task threads they work in instead.
 *
 * Each message is the shared {@link MessageCard}, the same one the task thread
 * draws, and — as there — the session that posted it is a way into that
 * session: a planner message carries the `author_session_id` of the agent that
 * said it. What is this surface's own is the card the thread sits in.
 */

import { useQuery } from "@tanstack/react-query"

import type { MessageDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { MessageCard } from "@/components/message-card"
import { MessageComposer } from "@/components/message-composer"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { useAddressees } from "@/features/profiles/addressees"
import { SessionLink } from "@/features/tasks/task-sessions"
import { goalMessagesQueryOptions, goalQueryOptions, usePostGoalMessage } from "./queries"

export function GoalThread({ goalId, className }: { goalId: string; className?: string }) {
  const messages = useQuery(goalMessagesQueryOptions(goalId))
  const goal = useQuery(goalQueryOptions(goalId))
  const post = usePostGoalMessage(goalId)
  const addressees = useAddressees(goal.data ? [goal.data.planner_profile_id] : [])

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

        <MessageComposer
          post={post}
          label="Message the planner thread"
          placeholder="Write to the planner thread…"
          addressees={addressees}
          // The box lives inside the card, so it is the card it has to cover.
          className="bg-card"
        />
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
        <MessageCard
          key={message.id}
          message={message}
          // The user's own messages come from no session; everything else was
          // said by an agent this links to.
          source={
            message.author_session_id ? <SessionLink sessionId={message.author_session_id} /> : null
          }
        />
      ))}
    </div>
  )
}

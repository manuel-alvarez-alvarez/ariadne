/** The messages agents exchanged while they worked on one task. */

import { useQuery } from "@tanstack/react-query"

import type { MessageDto, TaskDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"

import { taskMessagesQueryOptions } from "./queries"

export function TaskMessages({ task }: { task: TaskDto }) {
  const messages = useQuery(taskMessagesQueryOptions(task.id))

  if (messages.isPending) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
    )
  }

  if (messages.error) {
    return (
      <ErrorState
        title="Could not load messages"
        error={messages.error}
        onRetry={() => void messages.refetch()}
      />
    )
  }

  if (messages.data.length === 0) {
    return <EmptyState emphasis="quiet" title="The agents have said nothing yet" />
  }

  return (
    <ol className="space-y-4">
      {messages.data.map((message) => (
        <MessageRow key={message.id} message={message} task={task} />
      ))}
    </ol>
  )
}

function MessageRow({ message, task }: { message: MessageDto; task: TaskDto }) {
  return (
    <li className="border-b pb-4 last:border-b-0">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
        <span className="font-medium">
          {partyLabel(task, message.from_actor, message.from_agent_id)}
        </span>
        <span aria-hidden="true" className="text-muted-foreground">
          →
        </span>
        <span className="font-medium">
          {partyLabel(task, message.to_actor, message.to_agent_id)}
        </span>
        <When at={message.created_at} className="ml-auto text-xs text-muted-foreground" />
      </div>
      <Markdown className="mt-2">{message.body}</Markdown>
    </li>
  )
}

/** Match `task messages`: an agent is named for the skills it works with. */
function partyLabel(task: TaskDto, actor: string, agentId: string | null | undefined): string {
  if (!agentId) return actor
  const agent = task.agents.find((candidate) => candidate.id === agentId)
  if (!agent) return agentId
  return agent.skills.length > 0 ? agent.skills.join(", ") : "no skills"
}

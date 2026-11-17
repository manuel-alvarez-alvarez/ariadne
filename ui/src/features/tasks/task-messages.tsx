/** The messages agents exchanged while they worked on one task. */

import { useQuery } from "@tanstack/react-query"
import { type FormEvent, useState } from "react"

import type { MessageDto, TaskDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState, RefreshWarning } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Textarea } from "@/components/ui/textarea"
import { When } from "@/components/when"
import { describeError } from "@/lib/format"

import { usePanelHost } from "@/routes/paths"
import { taskMessagesQueryOptions, useSendTaskMessage } from "./queries"

export function TaskMessages({ task }: { task: TaskDto }) {
  return (
    <div className="space-y-4">
      <MessageList task={task} />
      <MessageComposer task={task} />
    </div>
  )
}

/** The user writing to one of the task's agents, or to the orchestrator. */
function MessageComposer({ task }: { task: TaskDto }) {
  const send = useSendTaskMessage(usePanelHost(), task.id)
  const [text, setText] = useState("")
  const [to, setTo] = useState("orchestrator")

  function submit(event: FormEvent) {
    event.preventDefault()
    const body = text.trim()
    if (!body) return
    const agent = to === "orchestrator" ? undefined : to
    send.mutate(
      { body, to_actor: agent ? "agent" : "orchestrator", to_agent_id: agent },
      { onSuccess: () => setText("") },
    )
  }

  return (
    <form onSubmit={submit} className="space-y-2">
      <Textarea
        aria-label="Message"
        placeholder="Write to an agent or the orchestrator"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <div className="flex items-center gap-2">
        <select
          aria-label="To"
          value={to}
          onChange={(event) => setTo(event.target.value)}
          className="h-8 rounded-lg border border-input bg-transparent px-2 text-sm"
        >
          <option value="orchestrator">orchestrator</option>
          {task.agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {partyLabel(task, "agent", agent.id)}
            </option>
          ))}
        </select>
        <Button type="submit" size="sm" pending={send.isPending} disabled={!text.trim()}>
          Send
        </Button>
        {send.error ? (
          <span role="alert" className="text-sm text-destructive">
            {describeError(send.error)}
          </span>
        ) : null}
      </div>
    </form>
  )
}

function MessageList({ task }: { task: TaskDto }) {
  const messages = useQuery(taskMessagesQueryOptions(usePanelHost(), task.id))

  if (messages.isPending) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
    )
  }

  if (messages.error && !messages.data) {
    return (
      <ErrorState
        title="Could not load messages"
        error={messages.error}
        onRetry={() => void messages.refetch()}
      />
    )
  }

  if (messages.data.length === 0) {
    return (
      <div className="space-y-4">
        {messages.error ? (
          <RefreshWarning
            what="messages"
            error={messages.error}
            onRetry={() => void messages.refetch()}
          />
        ) : null}
        <EmptyState emphasis="quiet" title="The agents have said nothing yet" />
      </div>
    )
  }

  return (
    <div className="space-y-4">
      {messages.error ? (
        <RefreshWarning
          what="messages"
          error={messages.error}
          onRetry={() => void messages.refetch()}
        />
      ) : null}
      <ol className="space-y-4">
        {messages.data.map((message) => (
          <MessageRow key={message.id} message={message} task={task} />
        ))}
      </ol>
    </div>
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

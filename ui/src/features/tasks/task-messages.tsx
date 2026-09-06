/**
 * What the agents of a task have said to each other — the `task messages`
 * equivalent.
 *
 * One channel carries all of it: the questions and their answers, the author's
 * review requests, and the verdicts that close a round. So the tab is grouped
 * by round rather than by kind — a round is the unit of work the conversation
 * happened inside — newest first, and inside a round in the order it was said.
 *
 * An agent has no name of its own, so both ends of a message are named by the
 * skills they work with: "code-review" said this to "coding". The orchestrator
 * is named by what it is, since a goal has one.
 */

import { useQuery } from "@tanstack/react-query"
import {
  CheckCircle2Icon,
  CircleHelpIcon,
  CornerUpLeftIcon,
  EyeIcon,
  MessageSquareIcon,
  MessageSquareWarningIcon,
} from "lucide-react"
import { useMemo } from "react"

import type { Actor, MessageDto, MessageKind, TaskAgentDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"

import { plural } from "@/lib/format"
import { taskMessagesQueryOptions, taskQueryOptions } from "./queries"
import { SessionLink } from "./task-sessions"

/**
 * How each kind reads. The two verdicts keep the colours a verdict has always
 * had — they are the ones that move the task — and the rest are quiet, because
 * a conversation is not a status.
 */
const KIND_META: Record<
  MessageKind,
  { label: string; badge: string; icon: typeof CheckCircle2Icon }
> = {
  approve: {
    label: "Approved",
    badge: "bg-status-done-soft text-status-done-fg",
    icon: CheckCircle2Icon,
  },
  request_changes: {
    label: "Changes requested",
    badge: "bg-status-warn-soft text-status-warn-fg",
    icon: MessageSquareWarningIcon,
  },
  review_request: {
    label: "Review requested",
    badge: "bg-muted text-muted-foreground",
    icon: EyeIcon,
  },
  question: {
    label: "Question",
    badge: "bg-muted text-muted-foreground",
    icon: CircleHelpIcon,
  },
  answer: {
    label: "Answer",
    badge: "bg-muted text-muted-foreground",
    icon: CornerUpLeftIcon,
  },
  note: {
    label: "Note",
    badge: "bg-muted text-muted-foreground",
    icon: MessageSquareIcon,
  },
}

export function TaskMessages({ taskId }: { taskId: string }) {
  const messages = useQuery(taskMessagesQueryOptions(taskId))
  // The task the messages belong to, for the skills each end works with.
  // Already in the cache: the panel around this read it first.
  const task = useQuery(taskQueryOptions(taskId))
  const agents = task.data?.agents ?? []
  const rounds = useMemo(() => groupByRound(messages.data ?? []), [messages.data])

  if (messages.isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    )
  }

  if (messages.error) {
    return (
      <ErrorState
        title="Could not load the messages"
        error={messages.error}
        onRetry={() => void messages.refetch()}
      />
    )
  }

  if (rounds.length === 0) {
    return <EmptyState emphasis="quiet" title="The agents have said nothing yet" />
  }

  return (
    <div className="space-y-5">
      {rounds.map(([round, entries]) => (
        <section key={round} className="space-y-2">
          <h3 className="font-heading font-medium text-sm">
            Round {round}
            <span className="ml-2 font-normal text-muted-foreground text-xs">
              {plural(entries.length, "message")}
            </span>
          </h3>
          {entries.map((message) => (
            <MessageCard key={message.id} message={message} agents={agents} />
          ))}
        </section>
      ))}
    </div>
  )
}

function MessageCard({ message, agents }: { message: MessageDto; agents: TaskAgentDto[] }) {
  const { label, badge, icon: Icon } = KIND_META[message.kind]
  return (
    <article className="rounded-lg border bg-card px-3 py-2">
      <header className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
        <StatusBadge size="sm" label={label} tone={badge} icon={<Icon className="size-3" />} />
        {/* Both ends, in the order a sentence puts them: two ULIDs are the
            same string to a reader, and the skills are not. */}
        <span className="font-medium text-foreground">
          {partyLabel(agents, message.from_actor, message.from_agent_id)}
          <span className="mx-1 text-muted-foreground">→</span>
          {partyLabel(agents, message.to_actor, message.to_agent_id)}
        </span>
        {message.from_session && <SessionLink sessionId={message.from_session} />}
        <When at={message.created_at} className="ml-auto text-muted-foreground" />
      </header>
      <Markdown>{message.body}</Markdown>
    </article>
  )
}

/**
 * One end of a message. An agent is named by the skills it works with, by its
 * id where the task staffs it no longer, and by what it is where it holds no
 * skills at all — there is one orchestrator, and it needs no id.
 */
function partyLabel(
  agents: TaskAgentDto[],
  actor: Actor,
  agentId: string | null | undefined,
): string {
  if (!agentId) return actor
  const agent = agents.find((one) => one.id === agentId)
  return agent?.skills.join(", ") || agentId
}

/** Newest round first; within a round, in the order it was said. */
function groupByRound(messages: MessageDto[]): [number, MessageDto[]][] {
  const rounds = new Map<number, MessageDto[]>()
  for (const message of messages) {
    const entries = rounds.get(message.round)
    if (entries) entries.push(message)
    else rounds.set(message.round, [message])
  }
  return [...rounds.entries()].sort(([a], [b]) => b - a)
}

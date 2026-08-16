/**
 * The task thread, read-only — the `task messages` equivalent: what the
 * planner, engineer and reviewers said while working the task. New messages
 * arrive through the event stream: `message_created` invalidates this query,
 * so a message an agent posts shows up without a refresh.
 */

import { useQuery } from "@tanstack/react-query"

import type { AuthorRole, MessageDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { describeError, formatAbsolute, formatRelative, shortId } from "./format"
import { Markdown } from "./markdown"
import { taskMessagesQueryOptions } from "./queries"

const ROLE_META: Record<AuthorRole, { label: string; badge: string; card: string }> = {
  planner: {
    label: "Planner",
    badge: "bg-violet-500/12 text-violet-700 dark:bg-violet-400/15 dark:text-violet-300",
    card: "border-violet-500/25",
  },
  engineer: {
    label: "Engineer",
    badge: "bg-blue-500/12 text-blue-700 dark:bg-blue-400/15 dark:text-blue-300",
    card: "border-blue-500/25",
  },
  reviewer: {
    label: "Reviewer",
    badge: "bg-amber-500/12 text-amber-700 dark:bg-amber-400/15 dark:text-amber-300",
    card: "border-amber-500/25",
  },
  user: {
    label: "You",
    badge: "bg-primary/12 text-foreground",
    card: "border-foreground/25",
  },
  system: {
    label: "System",
    badge: "bg-muted text-muted-foreground",
    card: "border-border",
  },
}

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
      <Alert variant="destructive">
        <AlertTitle>Could not load the conversation</AlertTitle>
        <AlertDescription>{describeError(messages.error)}</AlertDescription>
      </Alert>
    )
  }

  if (messages.data.length === 0) {
    return (
      <p className="rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
        Nothing has been said on this task yet.
      </p>
    )
  }

  return (
    <ol className="space-y-3">
      {messages.data.map((message) => (
        <li key={message.id}>
          <MessageCard message={message} />
        </li>
      ))}
    </ol>
  )
}

function MessageCard({ message }: { message: MessageDto }) {
  const role = ROLE_META[message.author_role]
  return (
    <article className={cn("rounded-lg border border-l-2 bg-card px-3 py-2", role.card)}>
      <header className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
        <span className={cn("rounded-full px-1.5 py-0.5 font-medium", role.badge)}>
          {role.label}
        </span>
        {message.author_session_id && (
          <span className="font-mono text-muted-foreground" title={message.author_session_id}>
            session {shortId(message.author_session_id)}
          </span>
        )}
        <time
          className="ml-auto text-muted-foreground"
          dateTime={message.created_at}
          title={formatAbsolute(message.created_at)}
        >
          {formatRelative(message.created_at)}
        </time>
      </header>
      <Markdown>{message.body}</Markdown>
    </article>
  )
}

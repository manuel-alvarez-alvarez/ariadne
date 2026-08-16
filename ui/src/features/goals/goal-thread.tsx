/**
 * The goal-level conversation, read-only: what the planner, the agents and
 * the daemon said about this goal. New messages arrive over the event stream
 * (`message_created` invalidates this exact key), so the thread appends
 * itself without polling.
 */

import { useQuery } from "@tanstack/react-query"
import { AlertCircleIcon } from "lucide-react"

import { ApiError, type AuthorRole, type MessageDto } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { formatAbsolute, formatRelative } from "./format"
import { Markdown } from "./markdown"
import { goalMessagesQueryOptions } from "./queries"

const ROLE_LABELS: Record<AuthorRole, string> = {
  planner: "Planner",
  engineer: "Engineer",
  reviewer: "Reviewer",
  user: "You",
  system: "System",
}

/** Who said it has to be readable at a glance, so each role gets its own tint. */
const ROLE_CLASSES: Record<AuthorRole, string> = {
  planner: "border-l-violet-500/60 bg-violet-500/[0.04]",
  engineer: "border-l-sky-500/60 bg-sky-500/[0.04]",
  reviewer: "border-l-amber-500/60 bg-amber-500/[0.04]",
  user: "border-l-primary/60 bg-primary/[0.04]",
  system: "border-l-border bg-muted/40",
}

export function GoalThread({ goalId, className }: { goalId: string; className?: string }) {
  const messages = useQuery(goalMessagesQueryOptions(goalId))
  const error = ApiError.is(messages.error) ? messages.error : null

  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>Planner thread</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error ? (
          <Alert variant="destructive">
            <AlertCircleIcon />
            <AlertTitle>Could not load the conversation</AlertTitle>
            <AlertDescription>{error.message}</AlertDescription>
            <AlertAction>
              <Button variant="outline" size="sm" onClick={() => void messages.refetch()}>
                Retry
              </Button>
            </AlertAction>
          </Alert>
        ) : null}

        {messages.isPending ? (
          <div className="flex flex-col gap-2">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ) : null}

        {messages.data ? <MessageList messages={messages.data} /> : null}
      </CardContent>
    </Card>
  )
}

function MessageList({ messages }: { messages: MessageDto[] }) {
  if (messages.length === 0) {
    return (
      <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
        Nothing in the thread yet.
      </p>
    )
  }

  // The list does not scroll on its own: it grows, and the panel around it
  // is the one scroll container.
  return (
    <div className="flex flex-col gap-3">
      {messages.map((message) => (
        <article
          key={message.id}
          className={cn("rounded-md border-l-2 px-3 py-2", ROLE_CLASSES[message.author_role])}
        >
          <header className="flex items-baseline justify-between gap-2">
            <span className="text-sm font-medium">{ROLE_LABELS[message.author_role]}</span>
            <time
              className="text-xs text-muted-foreground"
              dateTime={message.created_at}
              title={formatAbsolute(message.created_at)}
            >
              {formatRelative(message.created_at)}
            </time>
          </header>
          <Markdown className="mt-1">{message.body}</Markdown>
        </article>
      ))}
    </div>
  )
}

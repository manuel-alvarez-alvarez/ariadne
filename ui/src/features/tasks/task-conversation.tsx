/**
 * The task thread — the `task messages` / `task msg` equivalent.
 *
 * The thread is where the user talks to the agents working the task, so the
 * composer is part of the tab rather than hidden behind a dialog. New messages
 * arrive through the event stream: `message_created` invalidates this query,
 * so a message an agent posts shows up without a refresh.
 */

import { useQuery } from "@tanstack/react-query"
import { SendHorizontalIcon } from "lucide-react"
import { useState } from "react"

import type { AuthorRole, MessageDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { describeError, formatAbsolute, formatRelative, shortId } from "./format"
import { Markdown } from "./markdown"
import { taskMessagesQueryOptions, usePostTaskMessage } from "./queries"

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

  return (
    <div className="space-y-4">
      {messages.isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      ) : messages.error ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load the conversation</AlertTitle>
          <AlertDescription>{describeError(messages.error)}</AlertDescription>
        </Alert>
      ) : messages.data.length === 0 ? (
        <p className="rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          Nothing has been said on this task yet.
        </p>
      ) : (
        <ol className="space-y-3">
          {messages.data.map((message) => (
            <li key={message.id}>
              <MessageCard message={message} />
            </li>
          ))}
        </ol>
      )}

      <MessageComposer taskId={taskId} />
    </div>
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

function MessageComposer({ taskId }: { taskId: string }) {
  const [body, setBody] = useState("")
  const post = usePostTaskMessage(taskId)
  const canSend = body.trim().length > 0 && !post.isPending

  function send() {
    if (!canSend) return
    post.mutate(body.trim(), { onSuccess: () => setBody("") })
  }

  return (
    <form
      className="space-y-2"
      onSubmit={(event) => {
        event.preventDefault()
        send()
      }}
    >
      <Textarea
        value={body}
        onChange={(event) => setBody(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault()
            send()
          }
        }}
        rows={3}
        placeholder="Message the agents working this task…"
        aria-label="Message"
      />
      {post.error && (
        <Alert variant="destructive">
          <AlertTitle>Message not posted</AlertTitle>
          <AlertDescription>{describeError(post.error)}</AlertDescription>
        </Alert>
      )}
      <div className="flex items-center justify-end gap-2">
        <span className="text-xs text-muted-foreground">⌘↵ to send</span>
        <Button type="submit" disabled={!canSend}>
          <SendHorizontalIcon />
          {post.isPending ? "Sending…" : "Send"}
        </Button>
      </div>
    </form>
  )
}

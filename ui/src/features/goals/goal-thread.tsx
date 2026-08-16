/**
 * The goal-level conversation — the UI counterpart of `ariadne goal attach`.
 *
 * This is how the user talks to the planner: everything posted here lands in
 * the same thread the planner reads, and the planner's replies arrive over the
 * event stream (`message_created` invalidates this exact key), so the thread
 * appends itself without polling.
 */

import { useQuery } from "@tanstack/react-query"
import { AlertCircleIcon, SendIcon } from "lucide-react"
import { useLayoutEffect, useRef, useState } from "react"
import { toast } from "sonner"

import { ApiError, type AuthorRole, type MessageDto } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { formatAbsolute, formatRelative } from "./format"
import { Markdown } from "./markdown"
import { goalMessagesQueryOptions, usePostGoalMessage } from "./queries"

/** How far from the bottom still counts as "following the thread". */
const FOLLOW_THRESHOLD_PX = 64

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
    <Card className={cn("flex min-h-0 flex-col", className)}>
      <CardHeader>
        <CardTitle>Planner thread</CardTitle>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-col gap-3">
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

        <Composer goalId={goalId} />
      </CardContent>
    </Card>
  )
}

function MessageList({ messages }: { messages: MessageDto[] }) {
  const scroller = useRef<HTMLDivElement>(null)
  // Only follow the thread while the reader is at the end of it; scrolling back
  // to re-read something should not be yanked away by the next message.
  const following = useRef(true)

  // Before paint, so opening a long thread lands at its end without a jump.
  // The effect reads refs only; a new `messages` array is what has to trigger it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on new messages
  useLayoutEffect(() => {
    const element = scroller.current
    if (element && following.current) element.scrollTop = element.scrollHeight
  }, [messages])

  if (messages.length === 0) {
    return (
      <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
        Nothing in the thread yet. Write to the planner below.
      </p>
    )
  }

  return (
    <div
      ref={scroller}
      onScroll={(event) => {
        const { scrollTop, scrollHeight, clientHeight } = event.currentTarget
        following.current = scrollHeight - scrollTop - clientHeight < FOLLOW_THRESHOLD_PX
      }}
      className="flex max-h-[28rem] min-h-0 flex-col gap-3 overflow-y-auto pr-1"
    >
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

function Composer({ goalId }: { goalId: string }) {
  const [draft, setDraft] = useState("")
  const post = usePostGoalMessage(goalId)

  async function send() {
    const body = draft.trim()
    if (!body || post.isPending) return
    try {
      await post.mutateAsync(body)
      setDraft("")
    } catch (error) {
      toast.error("Message not sent", {
        description: ApiError.is(error) ? error.message : String(error),
      })
    }
  }

  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        void send()
      }}
    >
      <Textarea
        aria-label="Message to the planner"
        rows={3}
        value={draft}
        placeholder="Write to the planner…"
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault()
            void send()
          }
        }}
      />
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">⌘/Ctrl + Enter to send</span>
        <Button type="submit" size="sm" disabled={!draft.trim() || post.isPending}>
          <SendIcon />
          {post.isPending ? "Sending…" : "Send"}
        </Button>
      </div>
    </form>
  )
}

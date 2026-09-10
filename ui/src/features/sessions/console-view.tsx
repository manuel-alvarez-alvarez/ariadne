/**
 * The console as it renders in one of its two frames — the panel it belongs
 * to, or the dialog it was expanded into.
 *
 * What is here is the console stream, the transcript it feeds, and the input
 * line; the two frames themselves are `acp-console.tsx`'s.
 *
 * Two things this component tracks outlive any one event: the text a
 * still-open `permission_request` is answered with, and the text a still-
 * queued input is confirmed by — see `ingest` below. Both are read off a
 * ref rather than state, because they exist to answer a question the
 * transcript's *rendering* asks (`console-transcript.tsx`'s `ConsoleContext`)
 * and never need a render of their own: they always change in the same tick
 * as `events`, which does.
 */

import { Loader2Icon, MaximizeIcon, MinimizeIcon, PlugZapIcon, SendIcon } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { AgentEventDto, SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn, describeError } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { ConsoleStream, type ConsoleStreamStatus, consoleStreamUrl } from "./console-stream"
import { type ConsoleContext, ConsoleTranscript } from "./console-transcript"
import { sendConsoleInput } from "./queries"
import { isLiveStatus } from "./session-display"

/** One line of input sent and not yet confirmed by a `user_prompt_submit`. */
interface PendingInput {
  id: number
  text: string
}

export function ConsoleView({
  sessionId,
  status,
  className,
  screenClassName,
  expanded,
  autoFocus,
  onExpandedChange,
}: {
  sessionId: string
  status: SessionStatus
  className?: string
  screenClassName?: string
  /** Whether this is the expanded view rather than the panel's. */
  expanded: boolean
  /** Hand the input line the keyboard on mount; see `SessionDetailView`. */
  autoFocus?: boolean
  onExpandedChange: (expanded: boolean) => void
}) {
  const live = isLiveStatus(status)
  const baseUrl = useBaseUrl()
  const [events, setEvents] = useState<AgentEventDto[]>([])
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [error, setError] = useState<string | null>(null)
  const [pending, setPending] = useState<PendingInput[]>([])
  const [text, setText] = useState("")
  const [answeringId, setAnsweringId] = useState<string | null>(null)
  /**
   * A `permission_request` this console answered, kept apart from
   * `resolvedPermissions` below: that ref only ever learns of an answer once
   * the daemon's own `permission.replied` reaches the stream, and today the
   * daemon decides before this client can — every request is auto-approved
   * (021's known gap) — so without this the buttons would show as answered
   * only by whichever came first, never by the click that meant to.
   */
  const [localAnswers, setLocalAnswers] = useState<Record<string, string>>({})
  const nextPendingId = useRef(0)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)

  // Ids already folded into `events`, so a snapshot that overlaps the deltas
  // already applied on top of it — the always-possible race the doc comment
  // on `console.rs::stream` calls out — is not read twice.
  const seenIds = useRef(new Set<string>())
  // `user_prompt_submit`'s `prompt` field is the agent's whole system prompt
  // with the posted text appended, which is not what a person typed; this is
  // the text the console itself posted, kept for the event that confirms it.
  const resolvedPromptText = useRef(new Map<string, string>())
  // `permission_request` ids still waiting on the reply that answers them,
  // oldest first — `permission.replied` carries no id of its own to pair
  // with, only ever the next one open.
  const openPermissions = useRef<string[]>([])
  const resolvedPermissions = useRef(new Map<string, string | null>())

  // Stable across renders — everything it touches is a ref or a state
  // setter, neither of which changes identity — so the effect below can
  // depend on it without tearing the stream down on every render.
  const ingest = useCallback((batch: AgentEventDto[]) => {
    for (const event of batch) {
      if (seenIds.current.has(event.id)) continue
      seenIds.current.add(event.id)
      if (event.kind === "user_prompt_submit") {
        setPending((prev) => {
          const [first, ...rest] = prev
          if (first === undefined) return prev
          resolvedPromptText.current.set(event.id, first.text)
          return rest
        })
      } else if (event.kind === "permission_request") {
        openPermissions.current.push(event.id)
      } else if (event.kind === "permission.replied") {
        const requestId = openPermissions.current.shift()
        if (requestId !== undefined) {
          const optionId = event.payload as { option_id?: unknown }
          resolvedPermissions.current.set(
            requestId,
            typeof optionId.option_id === "string" ? optionId.option_id : null,
          )
        }
      }
    }
  }, [])

  // One stream per session and daemon. Every connection opens with a full
  // snapshot, so a reconnect replaces the transcript instead of appending to
  // it — see `console-stream.ts`.
  useEffect(() => {
    seenIds.current = new Set()
    resolvedPromptText.current = new Map()
    openPermissions.current = []
    resolvedPermissions.current = new Map()
    const stream = new ConsoleStream(consoleStreamUrl(baseUrl, sessionId), {
      onSnapshot: (snapshot) => {
        ingest(snapshot)
        setEvents(snapshot)
      },
      onEvent: (event) => {
        ingest([event])
        setEvents((prev) => [...prev, event])
      },
      onStatus: (next, why) => {
        setStreamStatus(next)
        if (why !== undefined) setError(why)
      },
    })
    stream.start()
    return () => stream.stop()
  }, [baseUrl, sessionId, ingest])

  useEffect(() => {
    if (!autoFocus || !live) return
    inputRef.current?.focus()
  }, [autoFocus, live])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" })
  }, [])

  function submit() {
    const value = text.trim()
    if (!value || !live) return
    const id = nextPendingId.current++
    setPending((prev) => [...prev, { id, text: value }])
    setText("")
    sendConsoleInput(sessionId, value).catch((cause: unknown) => {
      setPending((prev) => prev.filter((item) => item.id !== id))
      toast.error("Could not send that message", {
        id: `console-input-${sessionId}`,
        description: describeError(cause),
      })
    })
  }

  function answerPermission(event: AgentEventDto, optionId: string, label: string) {
    setAnsweringId(event.id)
    setLocalAnswers((prev) => ({ ...prev, [event.id]: optionId }))
    sendConsoleInput(sessionId, label)
      .catch((cause: unknown) => {
        setLocalAnswers((prev) => {
          const { [event.id]: _dropped, ...rest } = prev
          return rest
        })
        toast.error("Could not send the answer", {
          id: `console-input-${sessionId}`,
          description: describeError(cause),
        })
      })
      .finally(() => setAnsweringId((current) => (current === event.id ? null : current)))
  }

  const expandLabel = expanded ? "Collapse the console back into the panel" : "Expand the console"
  const transcriptContext: ConsoleContext = {
    resolvedPromptText: resolvedPromptText.current,
    // The daemon's own answer wins where both exist: it is the one that
    // actually reached the agent, and today it is also always there first
    // (see the `localAnswers` note above).
    resolvedPermissions: mergeResolved(resolvedPermissions.current, localAnswers),
    answeringId,
    onAnswerPermission: answerPermission,
  }

  return (
    <div className={cn("flex min-h-0 flex-col gap-2", expanded && "h-full", className)}>
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span role="status" className="min-w-0">
          <ConsoleStreamStatusLine status={streamStatus} error={error} />
        </span>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon-xs"
                onClick={() => onExpandedChange(!expanded)}
                aria-pressed={expanded}
                aria-label={expandLabel}
              />
            }
          >
            {expanded ? <MinimizeIcon /> : <MaximizeIcon />}
          </TooltipTrigger>
          <TooltipContent>{expandLabel}</TooltipContent>
        </Tooltip>
      </div>

      <div
        className={cn(
          "flex min-h-0 flex-col gap-2 overflow-y-auto rounded-lg border bg-card p-3 shadow-xs",
          expanded ? "flex-1" : "max-h-[28rem]",
          screenClassName,
        )}
      >
        <ConsoleTranscript events={events} context={transcriptContext} />
        {pending.map((item) => (
          <div key={item.id} className="flex justify-end">
            <div className="max-w-[85%] rounded-lg bg-primary/50 px-3 py-2 text-sm whitespace-pre-wrap text-primary-foreground">
              <p className="mb-1 flex items-center gap-1 text-xs font-medium opacity-70">
                <Loader2Icon className="size-3 animate-spin" />
                Sending…
              </p>
              {item.text}
            </div>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>

      <div className="flex items-end gap-2">
        <Textarea
          ref={inputRef}
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault()
              submit()
            }
          }}
          placeholder={live ? "Message the agent…" : "This session has ended."}
          disabled={!live}
          className="min-h-10"
        />
        <Button size="icon" onClick={submit} disabled={!live || text.trim().length === 0}>
          <SendIcon />
          <span className="sr-only">Send</span>
        </Button>
      </div>
    </div>
  )
}

function ConsoleStreamStatusLine({
  status,
  error,
}: {
  status: ConsoleStreamStatus
  error: string | null
}) {
  if (status === "reconnecting") {
    return (
      <span className="flex items-center gap-1.5 text-destructive">
        <PlugZapIcon className="size-3" />
        Lost the console stream, reconnecting…
        {error ? <span className="text-muted-foreground">({error})</span> : null}
      </span>
    )
  }
  if (status === "connecting") {
    return (
      <span className="flex items-center gap-1.5">
        <span
          className="size-1.5 shrink-0 animate-pulse rounded-full bg-muted-foreground"
          aria-hidden
        />
        Connecting to the session's console…
      </span>
    )
  }
  return (
    <span className="flex items-center gap-1.5">
      <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-status-done" aria-hidden />
      Live
    </span>
  )
}

/** The daemon's answers, with this console's own layered under them. */
function mergeResolved(
  fromDaemon: ReadonlyMap<string, string | null>,
  fromHere: Record<string, string>,
): ReadonlyMap<string, string | null> {
  if (Object.keys(fromHere).length === 0) return fromDaemon
  const merged = new Map(fromDaemon)
  for (const [id, optionId] of Object.entries(fromHere)) {
    if (!merged.has(id)) merged.set(id, optionId)
  }
  return merged
}

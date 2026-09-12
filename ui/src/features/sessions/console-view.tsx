/**
 * The console as it renders in one of its two frames — the panel it belongs
 * to, or the dialog it was expanded into.
 *
 * What is here is the console stream, the transcript it feeds, and the input
 * line; the two frames themselves are `acp-console.tsx`'s.
 *
 * The pane is one dark surface in both of the app's themes: it carries the
 * `dark` class, so every token inside it — the markdown, the buttons, the
 * diff — resolves to the dark theme's value without a variant of its own.
 *
 * The transcript is folded from the stream by `console-items.ts`, one pure
 * step per event, so a snapshot and a delta are read the same way. What the
 * view keeps beside it is what the fold cannot know: the text it posted and
 * is waiting to see confirmed, the answers it gave to permission questions
 * before the daemon's own reply came back, and whether the reader has
 * scrolled up to read — which is a measurement, kept in a ref written by the
 * scroll handler, with a state beside it only so the "Jump to latest" chip
 * can show.
 */

import { Loader2Icon, MaximizeIcon, MinimizeIcon, PlugZapIcon, SquareIcon } from "lucide-react"
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { AgentEventDto, SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn, describeError } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import {
  type ConsoleTranscriptState,
  EMPTY_TRANSCRIPT,
  foldConsoleEvent,
  foldConsoleSnapshot,
  openPermission,
} from "./console-items"
import { ConsoleStream, type ConsoleStreamStatus, consoleStreamUrl } from "./console-stream"
import { type ConsoleContext, ConsoleTranscript, PromptLine } from "./console-transcript"
import { cancelTurn, sendConsoleInput } from "./queries"
import { isLiveStatus } from "./session-display"

/** One line of input sent and not yet confirmed by a `user_prompt_submit`. */
interface PendingInput {
  id: number
  text: string
}

/**
 * Sub-pixel heights make `scrollTop` land a fraction short of the end; being
 * a pixel above the bottom still reads as "at the bottom".
 */
const FOLLOW_EPSILON = 1

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
  const [transcript, setTranscript] = useState<ConsoleTranscriptState>(EMPTY_TRANSCRIPT)
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [error, setError] = useState<string | null>(null)
  const [pending, setPending] = useState<PendingInput[]>([])
  const [text, setText] = useState("")
  const [answeringId, setAnsweringId] = useState<string | null>(null)
  /**
   * The permission questions this console answered, kept apart from the
   * daemon's own replies on the items: `permission.replied` only reaches the
   * stream once the daemon has the answer, and the buttons should show the
   * click that meant to answer before that.
   */
  const [localAnswers, setLocalAnswers] = useState<Map<string, string>>(new Map())
  const [following, setFollowing] = useState(true)
  /**
   * Whether the first snapshot has arrived. The input takes nothing before
   * it: a line posted earlier would be waiting on a snapshot that holds the
   * whole history, and a historical prompt with the same text — "continue",
   * typed twice in a session's life — is not the line's confirmation. Once
   * the history is on record, every prompt a later snapshot adds is new.
   */
  const [ready, setReady] = useState(false)
  const nextPendingId = useRef(0)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const scroller = useRef<HTMLDivElement>(null)
  /** Whether the view is pinned to the newest output. */
  const follow = useRef(true)

  // Ids already folded into the transcript, so a snapshot that overlaps the
  // deltas already applied on top of it — the always-possible race the doc
  // comment on `console.rs::stream` calls out — is not read twice.
  const seenIds = useRef(new Set<string>())

  // Stable across renders — everything it touches is a ref or a state
  // setter, neither of which changes identity — so the effect below can
  // depend on it without tearing the stream down on every render.
  const ingest = useCallback((event: AgentEventDto) => {
    if (seenIds.current.has(event.id)) return
    seenIds.current.add(event.id)
    const text = consolePromptText(event)
    if (text !== undefined) {
      setPending((prev) => prev.slice(confirmedCount(prev, [text])))
    }
    setTranscript((prev) => foldConsoleEvent(prev, event))
  }, [])

  // One stream per session and daemon. Every connection opens with a full
  // snapshot, so a reconnect replaces the transcript instead of appending to
  // it — see `console-stream.ts`.
  useEffect(() => {
    seenIds.current = new Set()
    setReady(false)
    const stream = new ConsoleStream(consoleStreamUrl(baseUrl, sessionId), {
      onSnapshot: (snapshot) => {
        // Nothing is posted before the first snapshot (see `ready`), so what
        // this snapshot holds that the console has not seen is the gap's
        // events, and among them can be the prompts confirming lines still
        // shown as pending. Which prompts those are is a matter of text, not
        // of count (see `confirmedCount`).
        const texts = snapshot.flatMap((event) =>
          seenIds.current.has(event.id) ? [] : (consolePromptText(event) ?? []),
        )
        setPending((prev) => prev.slice(confirmedCount(prev, texts)))
        seenIds.current = new Set(snapshot.map((event) => event.id))
        setTranscript(foldConsoleSnapshot(snapshot))
        setReady(true)
      },
      onEvent: ingest,
      onStatus: (next, why) => {
        setStreamStatus(next)
        if (why !== undefined) setError(why)
      },
    })
    stream.start()
    return () => stream.stop()
  }, [baseUrl, sessionId, ingest])

  useEffect(() => {
    if (!autoFocus || !live || !ready) return
    inputRef.current?.focus()
  }, [autoFocus, live, ready])

  // Keep the newest output in view — unless the reader scrolled up to read.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the transcript and the pending lines are the trigger, not an input
  useEffect(() => {
    const el = scroller.current
    if (el && follow.current) el.scrollTop = el.scrollHeight
  }, [transcript, pending])

  function onScroll() {
    const el = scroller.current
    if (!el) return
    const atBottom = el.scrollHeight - el.clientHeight - el.scrollTop <= FOLLOW_EPSILON
    follow.current = atBottom
    setFollowing(atBottom)
  }

  function jumpToLatest() {
    const el = scroller.current
    if (el) el.scrollTop = el.scrollHeight
    follow.current = true
    setFollowing(true)
  }

  function submit() {
    const value = text.trim()
    if (!value || !open) return
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

  function answerPermission(itemId: string, optionId: string, label: string) {
    setAnsweringId(itemId)
    setLocalAnswers((prev) => new Map(prev).set(itemId, optionId))
    sendConsoleInput(sessionId, label)
      .catch((cause: unknown) => {
        setLocalAnswers((prev) => {
          const next = new Map(prev)
          next.delete(itemId)
          return next
        })
        toast.error("Could not send the answer", {
          id: `console-input-${sessionId}`,
          description: describeError(cause),
        })
      })
      .finally(() => setAnsweringId((current) => (current === itemId ? null : current)))
  }

  function cancel() {
    cancelTurn(sessionId).catch((cause: unknown) => {
      toast.error("Could not stop the turn", {
        id: `console-cancel-${sessionId}`,
        description: describeError(cause),
      })
    })
  }

  /**
   * The pane's own keys, kept inside it: a digit picks a permission option
   * while the input is empty, and Escape in the input stops a running turn.
   * Both stop here so neither reaches the shell's chords nor, when the
   * console is expanded, the dialog's own Escape.
   */
  function onPaneKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.metaKey || event.ctrlKey || event.altKey) return
    if (event.key === "Escape") {
      if (!transcript.turnRunning || event.target !== inputRef.current) return
      event.preventDefault()
      event.stopPropagation()
      cancel()
      return
    }
    if (!/^[1-9]$/.test(event.key) || text.length > 0) return
    const question = openPermission(transcript.items)
    if (!question || localAnswers.has(question.id)) return
    const option = question.options[Number(event.key) - 1]
    if (!option) return
    event.preventDefault()
    event.stopPropagation()
    answerPermission(question.id, option.optionId, option.name)
  }

  const expandLabel = expanded ? "Collapse the console back into the panel" : "Expand the console"
  const transcriptContext: ConsoleContext = {
    answeringId,
    localAnswers,
    onAnswerPermission: answerPermission,
  }
  const turnRunning = live && transcript.turnRunning
  /** Whether the input takes a line: the session is live and its history is on record. */
  const open = live && ready

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the handler reads keys typed into the pane's own controls, which bubble here
    <div
      className={cn(
        "dark flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background font-mono text-xs text-foreground shadow-xs",
        expanded && "h-full",
        className,
      )}
      onKeyDown={onPaneKeyDown}
    >
      <div className="flex items-center justify-between gap-2 border-b px-3 py-1.5 text-muted-foreground">
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

      <div className={cn("relative flex min-h-0 flex-col", expanded ? "flex-1" : "max-h-[28rem]")}>
        <div
          ref={scroller}
          onScroll={onScroll}
          role="log"
          aria-label="Console transcript"
          className={cn("min-h-0 flex-1 overflow-y-auto p-3 leading-5", screenClassName)}
        >
          <ConsoleTranscript items={transcript.items} context={transcriptContext} />
          {pending.length > 0 ? (
            <div className="mt-1.5 flex flex-col gap-1.5">
              {pending.map((item) => (
                <PromptLine key={item.id} pending>
                  {item.text}
                </PromptLine>
              ))}
            </div>
          ) : null}
          {turnRunning ? (
            <span
              className="mt-1 inline-block h-4 w-2 animate-pulse bg-foreground/70"
              aria-hidden
            />
          ) : null}
        </div>
        {following ? null : (
          <Button
            size="xs"
            variant="secondary"
            onClick={jumpToLatest}
            className="absolute bottom-2 left-1/2 -translate-x-1/2 font-mono shadow-md"
          >
            Jump to latest
          </Button>
        )}
      </div>

      <div className="flex items-end gap-2 border-t px-3 py-2">
        <span className="shrink-0 select-none py-1.5 text-primary" aria-hidden>
          &gt;
        </span>
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
          placeholder={
            !live
              ? "This session has ended."
              : ready
                ? "Message the agent…"
                : "Waiting for the console…"
          }
          disabled={!open}
          rows={1}
          className="min-h-8 rounded-none border-0 bg-transparent px-0 py-1.5 font-mono text-xs shadow-none focus-visible:border-transparent focus-visible:ring-0 md:text-xs dark:bg-transparent"
        />
        {turnRunning ? (
          <Button size="xs" variant="outline" onClick={cancel} className="font-mono">
            <SquareIcon className="size-3" />
            Stop
          </Button>
        ) : null}
        <Button size="xs" onClick={submit} disabled={!open || text.trim().length === 0}>
          Send
        </Button>
      </div>
    </div>
  )
}

/**
 * The text of a `user_prompt_submit` a console posted, or `undefined` for any
 * other event. The daemon's own prompts — a nudge, a briefing — are not what
 * anyone typed, and confirm no pending line.
 */
function consolePromptText(event: AgentEventDto): string | undefined {
  if (event.kind !== "user_prompt_submit") return undefined
  const payload = event.payload
  if (typeof payload !== "object" || payload === null) return undefined
  const { source, text } = payload as { source?: unknown; text?: unknown }
  if (source === "daemon" || typeof text !== "string") return undefined
  return text
}

/**
 * How many pending lines, oldest first, these console prompts confirm.
 *
 * A line is confirmed by the prompt carrying its text, and the daemon takes
 * the lines in the order they were posted, so the confirmations are the
 * newest prompts, in the pending order: the answer is the longest run of
 * pending lines whose texts are the texts of that many newest prompts.
 *
 * Text alone cannot tell a confirmation from a historical prompt that
 * happens to read the same, which is why the prompts given here are never
 * the history: the input takes nothing before the first snapshot (see
 * `ready`), so by the time a line is pending every prompt on record is
 * seen, and what a later snapshot adds is the gap's own. Among those a
 * prompt another console posted during the gap still reads as one of ours
 * if its text is ours, and the pending line is dropped early; its own
 * confirmation then confirms nothing, which is a marker gone a moment soon.
 */
function confirmedCount(pending: PendingInput[], prompts: string[]): number {
  for (let run = Math.min(pending.length, prompts.length); run > 0; run -= 1) {
    const newest = prompts.slice(prompts.length - run)
    if (newest.every((text, index) => text === pending[index]?.text)) return run
  }
  return 0
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
      <span className="flex items-center gap-1.5 text-status-danger-fg">
        <PlugZapIcon className="size-3" />
        Lost the console stream, reconnecting…
        {error ? <span className="text-muted-foreground">({error})</span> : null}
      </span>
    )
  }
  if (status === "connecting") {
    return (
      <span className="flex items-center gap-1.5">
        <Loader2Icon className="size-3 animate-spin" aria-hidden />
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

/**
 * A session's console, drawn by the app: the console stream
 * (`console-stream.ts`) folded into blocks (`transcript.ts`), an input box
 * that posts to `/console/input`, and a cancel that posts to
 * `/console/cancel`.
 *
 * Each pane holds one stream for as long as it is on screen, so an Expand
 * into the modal and a Collapse back each close the old stream and open a
 * new one, and leaving the tab closes it. A snapshot replaces the transcript,
 * so a reconnect shows the session afresh rather than twice.
 *
 * The keyboard inside the console is the console's: Escape cancels a running
 * turn rather than closing the pane or the modal around it, and ⌘Escape
 * leaves the console for the panel's Close button or the modal's Collapse
 * button, so a plain Escape next closes what is now focused. The typed
 * chords of the shell stay silent while the input has the keyboard, since
 * it is a text field (`isTypingTarget`).
 */

import { Loader2Icon, Maximize2Icon, Minimize2Icon, PlugZapIcon, SquareIcon } from "lucide-react"
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react"

import { api, type SessionStatus, unwrap } from "@/api"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { cn, describeError } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { isLiveStatus } from "../session-display"
import { ConsoleStream, type ConsoleStreamStatus, consoleStreamUrl } from "./console-stream"
import {
  appendError,
  applyEvent,
  applySnapshot,
  type Block,
  emptyTranscript,
  isTerminal,
  oldestEventId,
  type PermissionOption,
  pendingPermission,
  prependEvents,
  type Transcript,
} from "./transcript"

export function SessionConsole({
  sessionId,
  status,
  autoFocus,
  className,
}: {
  sessionId: string
  status: SessionStatus
  /** Hand the input the keyboard as soon as it is on screen. */
  autoFocus?: boolean
  className?: string
}) {
  const [expanded, setExpanded] = useState(false)

  if (expanded) {
    return (
      <Dialog open onOpenChange={(open) => open || setExpanded(false)}>
        <DialogContent
          showCloseButton={false}
          className="h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-[calc(100vw-2rem)] p-0 sm:max-w-[calc(100vw-2rem)]"
        >
          <DialogTitle className="sr-only">Session console</DialogTitle>
          <ConsolePane
            sessionId={sessionId}
            status={status}
            autoFocus
            className="h-full"
            onCollapse={() => setExpanded(false)}
          />
        </DialogContent>
      </Dialog>
    )
  }

  return (
    <ConsolePane
      sessionId={sessionId}
      status={status}
      autoFocus={autoFocus}
      className={className}
      onExpand={() => setExpanded(true)}
    />
  )
}

/** The events an older page holds at most, the daemon's cap. */
const HISTORY_PAGE = 200

/** How near the top, in pixels, still counts as reaching the oldest block shown. */
const TOP_SLACK_PX = 8

/** Where the load of older events stands; `start` is the session's first event shown. */
type History = { state: "idle" | "loading" | "start" } | { state: "error"; message: string }

/** How near the bottom, in pixels, still counts as reading the newest block. */
const FOLLOW_SLACK_PX = 24

function ConsolePane({
  sessionId,
  status,
  autoFocus,
  className,
  onExpand,
  onCollapse,
}: {
  sessionId: string
  status: SessionStatus
  autoFocus?: boolean
  className?: string
  onExpand?: () => void
  onCollapse?: () => void
}) {
  const baseUrl = useBaseUrl()
  const [transcript, setTranscript] = useState<Transcript>(emptyTranscript)
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [streamError, setStreamError] = useState<string | null>(null)
  const [draft, setDraft] = useState("")
  const [selected, setSelected] = useState(0)
  const [answering, setAnswering] = useState<string | null>(null)
  const body = useRef<HTMLDivElement>(null)
  const log = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)
  const collapseButton = useRef<HTMLButtonElement>(null)
  const following = useRef(true)
  const [history, setHistory] = useState<History>({ state: "idle" })
  // Bumped when a snapshot or a new session replaces the transcript, so a page
  // still loading for the old one is dropped.
  const generation = useRef(0)
  const loading = useRef(false)
  const reached = useRef(false)
  const latestTranscript = useRef(transcript)
  latestTranscript.current = transcript
  /** The scroll box's size before a prepend, to keep the reader's place after it. */
  const anchor = useRef<{ height: number; top: number } | null>(null)

  const resetHistory = useCallback(() => {
    generation.current += 1
    loading.current = false
    reached.current = false
    anchor.current = null
    setHistory({ state: "idle" })
  }, [])

  const loadOlder = useCallback(async () => {
    if (loading.current || reached.current) return
    const before = oldestEventId(latestTranscript.current)
    if (before === undefined) return
    const mine = generation.current
    loading.current = true
    setHistory({ state: "loading" })
    try {
      const page = await unwrap(
        api().GET("/v1/events", {
          params: {
            query: { session: sessionId, order: "desc", before, limit: HISTORY_PAGE },
          },
        }),
      )
      if (generation.current !== mine) return
      loading.current = false
      if (page.length === 0) {
        reached.current = true
        setHistory({ state: "start" })
        return
      }
      const el = log.current
      anchor.current = el ? { height: el.scrollHeight, top: el.scrollTop } : null
      setTranscript((current) => prependEvents(current, page.slice().reverse()))
      setHistory({ state: "idle" })
    } catch (error) {
      if (generation.current !== mine) return
      loading.current = false
      setHistory({ state: "error", message: describeError(error) })
    }
  }, [sessionId])

  useEffect(() => {
    setTranscript(emptyTranscript())
    resetHistory()
    const stream = new ConsoleStream(consoleStreamUrl(baseUrl, sessionId), {
      onSnapshot: (events) => {
        resetHistory()
        setTranscript((current) => applySnapshot(current, events))
      },
      onEvent: (event) => setTranscript((current) => applyEvent(current, event)),
      onStatus: (next, error) => {
        setStreamStatus(next)
        setStreamError(error ?? null)
      },
    })
    stream.start()
    return () => stream.stop()
  }, [baseUrl, sessionId, resetHistory])

  // biome-ignore lint/correctness/useExhaustiveDependencies: the focus request is read once, on arrival
  useEffect(() => {
    if (autoFocus) field.current?.focus()
  }, [])

  // Follow the newest block, unless the reader scrolled up to read.
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new transcript is what moves the bottom
  useLayoutEffect(() => {
    const el = log.current
    if (!el) return
    const kept = anchor.current
    anchor.current = null
    if (kept) el.scrollTop = kept.top + (el.scrollHeight - kept.height)
    else if (following.current) el.scrollTop = el.scrollHeight
  }, [transcript])

  const live = isLiveStatus(status)
  const running = live && (status === "running" || transcript.running)
  const pending = live ? pendingPermission(transcript) : undefined
  const pendingId = pending?.id

  // A new question starts on its first option, and with nobody answering it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only a new question resets the pick
  useEffect(() => {
    setSelected(0)
    setAnswering(null)
  }, [pendingId])

  const fail = useCallback((what: string, error: unknown) => {
    setTranscript((current) => appendError(current, `${what}: ${describeError(error)}`))
  }, [])

  const post = useCallback(
    (text: string) =>
      unwrap(
        api().POST("/v1/sessions/{id}/console/input", {
          params: { path: { id: sessionId } },
          body: { text },
        }),
      ),
    [sessionId],
  )

  function send() {
    const text = draft
    if (text.trim().length === 0) return
    setDraft("")
    post(text).catch((error) => fail("Could not send the prompt", error))
  }

  function answer(option: PermissionOption) {
    setAnswering(option.id)
    post(option.id).catch((error) => {
      setAnswering(null)
      fail("Could not answer the permission", error)
    })
  }

  const cancel = useCallback(() => {
    unwrap(
      api().POST("/v1/sessions/{id}/console/cancel", { params: { path: { id: sessionId } } }),
    ).catch((error) => fail("Could not cancel the turn", error))
  }, [sessionId, fail])

  /** The picker's keys: Up, Down and 1-9 select, Enter answers. True when taken. */
  function pickKey(event: KeyboardEvent): boolean {
    if (!pending || answering !== null || pending.options.length === 0) return false
    const count = pending.options.length
    if (event.key === "ArrowUp") setSelected((index) => (index + count - 1) % count)
    else if (event.key === "ArrowDown") setSelected((index) => (index + 1) % count)
    else if (/^[1-9]$/.test(event.key) && Number(event.key) <= count)
      setSelected(Number(event.key) - 1)
    else if (event.key === "Enter") {
      const option = pending.options[selected]
      if (option) answer(option)
    } else return false
    event.preventDefault()
    return true
  }

  function onFieldKey(event: KeyboardEvent<HTMLTextAreaElement>) {
    // Shift+Enter is a new line, and an Enter that ends a composition is the
    // input method's: neither sends a prompt or answers a question.
    if (event.key === "Enter" && (event.shiftKey || event.nativeEvent.isComposing)) return
    // An empty input hands its keys to a waiting question, as the CLI does.
    if (draft.length === 0 && pickKey(event)) return
    if (event.key === "Enter") {
      event.preventDefault()
      send()
    }
  }

  // Escape and ⌘Escape are claimed on the console's own element, before the
  // pane or the dialog around it hears of them.
  const latest = useRef({ running, cancel, onCollapse })
  latest.current = { running, cancel, onCollapse }
  useEffect(() => {
    const el = body.current
    if (!el) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return
      event.preventDefault()
      event.stopPropagation()
      if (event.metaKey) {
        if (latest.current.onCollapse) collapseButton.current?.focus()
        else
          el.closest<HTMLElement>('[data-slot="docked-pane"]')
            ?.querySelector<HTMLElement>('[data-slot="pane-close"]')
            ?.focus()
        return
      }
      if (latest.current.running) latest.current.cancel()
    }
    el.addEventListener("keydown", onKey)
    return () => el.removeEventListener("keydown", onKey)
  }, [])

  return (
    <div
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background text-sm text-foreground shadow-xs",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2 border-b px-3 py-1.5 font-mono text-xs text-muted-foreground">
        <span role="status" className="min-w-0">
          <StreamStatusLine status={streamStatus} error={streamError} />
        </span>
        {onExpand ? (
          <Button size="icon-xs" variant="ghost" onClick={onExpand} aria-label="Expand the console">
            <Maximize2Icon />
          </Button>
        ) : null}
        {onCollapse ? (
          <Button
            ref={collapseButton}
            size="icon-xs"
            variant="ghost"
            onClick={onCollapse}
            aria-label="Collapse the console back into the panel"
          >
            <Minimize2Icon />
          </Button>
        ) : null}
      </div>
      <div ref={body} className="flex min-h-0 flex-1 flex-col">
        <HistoryLine history={history} onRetry={() => void loadOlder()} />
        <div
          ref={log}
          role="log"
          aria-label="Console transcript"
          className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3"
          onScroll={(event) => {
            const el = event.currentTarget
            following.current = el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_SLACK_PX
            if (el.scrollTop <= TOP_SLACK_PX) void loadOlder()
          }}
        >
          {transcript.blocks.map((block) => (
            <BlockView
              key={block.id}
              block={block}
              picker={
                block.type === "permission" && block.id === pendingId ? (
                  <PermissionPicker
                    options={block.options}
                    selected={selected}
                    answering={answering}
                    onSelect={setSelected}
                    onAnswer={answer}
                    onKeyDown={pickKey}
                  />
                ) : null
              }
            />
          ))}
        </div>
        <div className="flex items-end gap-2 border-t p-2">
          <div className="min-w-0 flex-1">
            <textarea
              ref={field}
              aria-label="Message the agent"
              value={draft}
              disabled={!live}
              rows={2}
              placeholder={
                live ? "Message the agent. Enter sends, Shift+Enter adds a line." : undefined
              }
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={onFieldKey}
              className="block max-h-40 min-h-10 w-full resize-none rounded-md border border-input bg-transparent px-2.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
            />
            {live ? null : (
              <p className="pt-1 text-xs text-muted-foreground">
                This session has ended. Resume it to talk to it.
              </p>
            )}
          </div>
          {running ? (
            <Button size="sm" variant="outline" onClick={cancel} aria-label="Cancel the turn">
              <SquareIcon />
              Cancel
            </Button>
          ) : null}
        </div>
      </div>
    </div>
  )
}

function HistoryLine({ history, onRetry }: { history: History; onRetry: () => void }) {
  if (history.state === "loading") {
    return (
      <div
        role="status"
        className="flex items-center gap-1.5 border-b px-3 py-1 font-mono text-xs text-muted-foreground"
      >
        <Loader2Icon className="size-3 animate-spin" aria-hidden />
        Loading older events…
      </div>
    )
  }
  if (history.state === "error") {
    return (
      <div
        role="alert"
        className="flex items-center gap-2 border-b px-3 py-1 font-mono text-xs text-status-danger-fg"
      >
        Could not load older events: {history.message}
        <Button size="xs" variant="outline" onClick={onRetry}>
          Retry
        </Button>
      </div>
    )
  }
  return null
}

function StreamStatusLine({
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
        Lost the console, reconnecting…
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

function BlockView({ block, picker }: { block: Block; picker: ReactNode }) {
  return (
    <div data-block={block.type}>
      {block.subagent ? (
        <div className="pb-0.5 font-mono text-[11px] text-muted-foreground">
          ↳ {block.subagent.title}
        </div>
      ) : null}
      <BlockBody block={block} picker={picker} />
    </div>
  )
}

function BlockBody({ block, picker }: { block: Block; picker: ReactNode }) {
  switch (block.type) {
    case "prompt":
      return (
        <div className="rounded-md border-l-2 border-primary bg-muted/50 px-3 py-2">
          {block.daemon ? (
            <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
              daemon
            </span>
          ) : null}
          <p className="whitespace-pre-wrap break-words">{block.text}</p>
        </div>
      )
    case "message":
      return <Markdown>{block.text}</Markdown>
    case "thought":
      return <Markdown className="text-muted-foreground italic">{block.text}</Markdown>
    case "plan":
      return (
        <ul className="space-y-0.5 font-mono text-xs">
          {block.entries.map((entry, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a plan's entries have no id, and are replaced whole
            <li key={index} className={cn(entry.status === "completed" && "text-muted-foreground")}>
              <span aria-hidden className="pr-1.5">
                {PLAN_MARKS[entry.status] ?? "○"}
              </span>
              {entry.content} <span className="text-muted-foreground">· {entry.status}</span>
            </li>
          ))}
        </ul>
      )
    case "tool":
      return (
        <div className="rounded-md border px-3 py-2 font-mono text-xs">
          <div className="flex flex-wrap items-center gap-x-2">
            <ToolStatus status={block.tool.status} />
            {block.tool.kind ? (
              <span className="text-muted-foreground">{block.tool.kind}</span>
            ) : null}
            <span className="min-w-0 break-words font-medium">{block.tool.title}</span>
          </div>
          {block.tool.output ? <Output text={block.tool.output} /> : null}
          {block.tool.diff ? <Diff text={block.tool.diff} /> : null}
        </div>
      )
    case "permission":
      return (
        <div className="rounded-md border border-status-warn/40 bg-status-warn-soft/30 px-3 py-2 text-xs">
          <div className="font-medium">Permission · {block.title}</div>
          {block.command ? <Output text={block.command} /> : null}
          {block.diff ? <Diff text={block.diff} /> : null}
          {block.answer ? (
            <div className="pt-1 font-mono text-muted-foreground">
              <span aria-hidden>↳ </span>
              {block.answer.decidedBy
                ? `${block.answer.name} · decided by ${block.answer.decidedBy}`
                : block.answer.name}
            </div>
          ) : (
            picker
          )}
        </div>
      )
    case "note":
      return <div className="font-mono text-xs text-muted-foreground">· {block.text}</div>
    case "error":
      return (
        <div className="rounded-md border border-status-danger/40 bg-status-danger-soft/40 px-3 py-2 text-xs text-status-danger-fg">
          <p className="whitespace-pre-wrap break-words">{block.text}</p>
          {block.diagnosis ? <p className="pt-1 text-muted-foreground">{block.diagnosis}</p> : null}
        </div>
      )
    case "raw":
      return (
        <div className="font-mono text-xs text-muted-foreground">
          {block.kind} · {block.summary}
        </div>
      )
  }
}

const PLAN_MARKS: Record<string, string> = { completed: "✓", in_progress: "◐", pending: "○" }

function ToolStatus({ status }: { status?: string }) {
  const tone =
    status === "failed"
      ? "text-status-danger-fg"
      : isTerminal(status)
        ? "text-status-done-fg"
        : "text-status-active-fg"
  return <span className={tone}>{status ?? "pending"}</span>
}

function Output({ text }: { text: string }) {
  return (
    <pre className="mt-1.5 max-h-64 overflow-auto rounded bg-muted/50 p-2 font-mono text-xs whitespace-pre-wrap break-words">
      {text}
    </pre>
  )
}

function Diff({ text }: { text: string }) {
  return (
    <pre className="mt-1.5 max-h-64 overflow-auto rounded bg-muted/50 p-2 font-mono text-xs">
      {text.split("\n").map((line, index) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff, never reordered
          key={index}
          className={cn(
            line.startsWith("+") && !line.startsWith("+++") && "text-status-done-fg",
            line.startsWith("-") && !line.startsWith("---") && "text-status-danger-fg",
            line.startsWith("@@") && "text-muted-foreground",
          )}
        >
          {line || " "}
        </div>
      ))}
    </pre>
  )
}

function PermissionPicker({
  options,
  selected,
  answering,
  onSelect,
  onAnswer,
  onKeyDown,
}: {
  options: PermissionOption[]
  selected: number
  answering: string | null
  onSelect: (index: number) => void
  onAnswer: (option: PermissionOption) => void
  onKeyDown: (event: KeyboardEvent) => boolean
}) {
  return (
    <div
      role="listbox"
      aria-label="Answer the permission"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="mt-2 space-y-0.5 rounded-md outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
    >
      {options.map((option, index) => (
        <div
          key={option.id}
          role="option"
          aria-selected={index === selected}
          aria-disabled={answering !== null}
          tabIndex={-1}
          onMouseEnter={() => onSelect(index)}
          onClick={() => answering === null && onAnswer(option)}
          onKeyDown={() => undefined}
          className={cn(
            "flex cursor-pointer items-center gap-2 rounded px-2 py-1 font-mono",
            index === selected ? "bg-primary text-primary-foreground" : "hover:bg-muted",
          )}
        >
          <span className="w-4 text-right opacity-70">{index + 1}.</span>
          {option.name}
          {answering === option.id ? (
            <Loader2Icon className="size-3 animate-spin" aria-hidden />
          ) : null}
        </div>
      ))}
    </div>
  )
}

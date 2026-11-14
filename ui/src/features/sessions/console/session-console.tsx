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
 * it is a text field (`isTypingTarget`). Ctrl-O folds and unfolds the whole
 * console, and Escape closes the slash-command list before it cancels a turn.
 *
 * A session with subagents shows a bar of views above the input: `main`, the
 * session's own, and one per child. A click, Tab and Shift-Tab switch the
 * view. A child's view is read-only: Escape there goes back to `main`, or
 * cancels the child where it runs and said it may be cancelled.
 *
 * The extras the CLI's attach pane has (`crates/ariadne-console/src/tui/`):
 * one fold toggle, the agent's slash commands as a suggestion list over the
 * input, a status row, a sent prompt shown at once as `queued`, and a footer
 * of the keys the state takes.
 */

import {
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  Loader2Icon,
  Maximize2Icon,
  Minimize2Icon,
  PlugZapIcon,
  SquareIcon,
} from "lucide-react"
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react"

import { type AgentRequestDto, api, type SessionStatus, unwrap } from "@/api"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { cn, describeError, formatTokens } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { isLiveStatus } from "../session-display"
import { ConsoleStream, type ConsoleStreamStatus, consoleStreamUrl } from "./console-stream"
import { pendingRequests, QuestionCard, restoredQuestions } from "./questions"
import {
  type AgentCommand,
  appendError,
  applyEvent,
  applySnapshot,
  type Block,
  dequeuePrompt,
  emptyTranscript,
  isTerminal,
  oldestEventId,
  type PermissionOption,
  pendingPermission,
  prependEvents,
  queuePrompt,
  runningTool,
  stateMark,
  type TokenTotals,
  type Transcript,
  tokenTotals,
  view,
} from "./transcript"

/** How many lines a folded console keeps of each kind of text. */
const FOLDED_OUTPUT_LINES = 4
const FOLDED_DIFF_LINES = 24
const FOLDED_PROMPT_LINES = 6

/** The most commands the suggestion list shows. */
const SUGGESTIONS = 8

export function SessionConsole({
  sessionId,
  status,
  autoFocus,
  usage,
  className,
}: {
  sessionId: string
  status: SessionStatus
  /** Hand the input the keyboard as soon as it is on screen. */
  autoFocus?: boolean
  /** What the session itself reports spending, the floor of the status row's totals. */
  usage?: TokenTotals
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
            usage={usage}
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
      usage={usage}
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
  usage,
  className,
  onExpand,
  onCollapse,
}: {
  sessionId: string
  status: SessionStatus
  autoFocus?: boolean
  usage?: TokenTotals
  className?: string
  onExpand?: () => void
  onCollapse?: () => void
}) {
  const baseUrl = useBaseUrl()
  const [transcript, setTranscript] = useState<Transcript>(emptyTranscript)
  /** The session's pending requests, as read with the last snapshot. */
  const [requests, setRequests] = useState<AgentRequestDto[]>([])
  /** Why the last read of the pending requests failed; none once one succeeds. */
  const [requestsError, setRequestsError] = useState<string | null>(null)
  /** Counts the reads of the pending requests, so only the latest one lands. */
  const reads = useRef(0)
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [streamError, setStreamError] = useState<string | null>(null)
  const [draft, setDraft] = useState("")
  const [selected, setSelected] = useState(0)
  const [folded, setFolded] = useState(true)
  const [cursor, setCursor] = useState(0)
  const [suggested, setSuggested] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  const [now, setNow] = useState(Date.now)
  const [answering, setAnswering] = useState<string | null>(null)
  /** The child whose view is shown; none for `main`. */
  const [shown, setShown] = useState<string | undefined>(undefined)
  const body = useRef<HTMLDivElement>(null)
  const log = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)
  const collapseButton = useRef<HTMLButtonElement>(null)
  const root = useRef<HTMLDivElement>(null)
  const sends = useRef(0)
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

  // A question older than the snapshot's page still waits: the pending
  // requests are read again with every snapshot, a reconnect's too, and a
  // failed read says so with a Retry rather than hide those questions.
  const readRequests = useCallback(() => {
    const read = ++reads.current
    pendingRequests(sessionId).then(
      (listed) => {
        if (read !== reads.current) return
        setRequests(listed)
        setRequestsError(null)
      },
      (error) => {
        if (read === reads.current) setRequestsError(describeError(error))
      },
    )
  }, [sessionId])

  useEffect(() => {
    setTranscript(emptyTranscript())
    setRequests([])
    setRequestsError(null)
    resetHistory()
    const stream = new ConsoleStream(consoleStreamUrl(baseUrl, sessionId), {
      onSnapshot: (events) => {
        resetHistory()
        setTranscript((current) => applySnapshot(current, events))
        readRequests()
      },
      onEvent: (event) => setTranscript((current) => applyEvent(current, event)),
      onStatus: (next, error) => {
        setStreamStatus(next)
        setStreamError(error ?? null)
      },
    })
    stream.start()
    return () => {
      reads.current += 1
      stream.stop()
    }
  }, [baseUrl, sessionId, resetHistory, readRequests])

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

  const children = transcript.subagents
  // A snapshot that no longer holds the child shown goes back to `main`.
  const child = children.find((one) => one.id === shown)
  const blocks = view(transcript, child?.id)
  const runningChildren = children.filter((one) => one.state === "running").length
  const childCancellable = child?.cancel === true && child.state === "running"
  useEffect(() => {
    if (shown !== undefined && child === undefined) setShown(undefined)
  }, [shown, child])

  const live = isLiveStatus(status)
  const running = live && (status === "running" || transcript.running)
  const pending = live ? pendingPermission(transcript) : undefined
  const pendingId = pending?.id
  const turnStartedAt = running ? transcript.turnStartedAt : undefined

  // The turn clock counts on, once a second, while a turn runs.
  useEffect(() => {
    if (turnStartedAt === undefined) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [turnStartedAt])

  const suggestions =
    live && !pending && !dismissed
      ? suggest(transcript.availableCommands, draft, cursor)
      : undefined
  const pickedSuggestion = Math.min(suggested, Math.max((suggestions?.length ?? 1) - 1, 0))

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

  function edit(text: string, at: number) {
    setDraft(text)
    setCursor(at)
    setSuggested(0)
    setDismissed(false)
  }

  function send() {
    const text = draft
    if (text.trim().length === 0) return
    edit("", 0)
    // Each send is its own queue entry, so a failure takes back that send alone.
    sends.current += 1
    const id = `queued:${sends.current}`
    setTranscript((current) => queuePrompt(current, text, id))
    post(text).catch((error) => {
      setTranscript((current) => dequeuePrompt(current, id, text))
      fail("Could not send the prompt", error)
    })
  }

  /** The suggestion list's keys: Up and Down move, Tab and Enter complete. True when taken. */
  function suggestKey(event: KeyboardEvent): boolean {
    if (!suggestions) return false
    const count = suggestions.length
    if (event.key === "ArrowUp" && count > 0) setSuggested(Math.max(pickedSuggestion - 1, 0))
    else if (event.key === "ArrowDown" && count > 0)
      setSuggested(Math.min(pickedSuggestion + 1, count - 1))
    else if ((event.key === "Tab" || (event.key === "Enter" && !event.shiftKey)) && count > 0) {
      const command = suggestions[pickedSuggestion]
      if (command) edit(`/${command.name} `, command.name.length + 2)
    } else return false
    event.preventDefault()
    return true
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

  const cancelChild = useCallback(
    (target: { id: string; title: string }) => {
      unwrap(
        api().POST("/v1/sessions/{id}/console/cancel", {
          params: { path: { id: sessionId } },
          body: { subagent_id: target.id },
        }),
      ).catch((error) =>
        setTranscript((current) =>
          appendError(current, `Could not cancel the subagent: ${describeError(error)}`, {
            id: target.id,
            title: target.title,
          }),
        ),
      )
    },
    [sessionId],
  )

  /** Show the view of `to`, or `main` for none, and hand it the keyboard. */
  const showView = useCallback((to: string | undefined) => {
    setShown(to)
    // The child's view has no input: the console itself keeps the keys.
    requestAnimationFrame(() => (to === undefined ? field.current : body.current)?.focus())
  }, [])

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
    if (suggestKey(event)) return
    if (event.key === "Enter") {
      event.preventDefault()
      send()
    }
  }

  // Escape and ⌘Escape are claimed on the console's own element, before the
  // pane or the dialog around it hears of them.
  const latest = useRef({
    running,
    cancel,
    onCollapse,
    children,
    child,
    childCancellable,
    suggesting: false,
    dismiss: () => {},
  })
  latest.current = {
    running,
    cancel,
    onCollapse,
    children,
    child,
    childCancellable,
    suggesting: suggestions !== undefined,
    dismiss: () => setDismissed(true),
  }
  const switchers = useRef({ showView, cancelChild })
  switchers.current = { showView, cancelChild }
  useEffect(() => {
    const el = body.current
    if (!el) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      const { children, child, childCancellable } = latest.current
      // Tab and Shift-Tab walk the views in bar order, wrapping around. A
      // session with no child keeps its Tab.
      if (event.key === "Tab" && children.length > 0 && !event.metaKey && !event.ctrlKey) {
        event.preventDefault()
        const views = [undefined, ...children.map((one) => one.id)]
        const at = views.indexOf(child?.id)
        const step = event.shiftKey ? views.length - 1 : 1
        switchers.current.showView(views[(at + step) % views.length])
        return
      }
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
      if (latest.current.suggesting) latest.current.dismiss()
      else if (child) {
        if (childCancellable) switchers.current.cancelChild(child)
        else switchers.current.showView(undefined)
      } else if (latest.current.running) latest.current.cancel()
    }
    el.addEventListener("keydown", onKey)
    return () => el.removeEventListener("keydown", onKey)
  }, [])

  // Ctrl-O folds from anywhere in the console, the header's controls included.
  useEffect(() => {
    const el = root.current
    if (!el) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key.toLowerCase() !== "o" || !event.ctrlKey || event.metaKey || event.altKey) return
      event.preventDefault()
      setFolded((was) => !was)
    }
    el.addEventListener("keydown", onKey)
    return () => el.removeEventListener("keydown", onKey)
  }, [])

  return (
    <div
      ref={root}
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background text-sm text-foreground shadow-xs",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2 border-b px-3 py-1.5 font-mono text-xs text-muted-foreground">
        <span role="status" className="flex min-w-0 items-center gap-3">
          <StreamStatusLine status={streamStatus} error={streamError} />
          {runningChildren > 0 ? (
            <span className="text-status-active-fg">
              running {runningChildren} {runningChildren === 1 ? "subagent" : "subagents"}
            </span>
          ) : null}
        </span>
        <Button
          size="xs"
          variant="ghost"
          onClick={() => setFolded((was) => !was)}
          aria-keyshortcuts="Control+O"
        >
          {folded ? <ChevronsUpDownIcon /> : <ChevronsDownUpIcon />}
          {folded ? "Unfold" : "Fold"}
        </Button>
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
      <div ref={body} tabIndex={-1} className="flex min-h-0 flex-1 flex-col outline-none">
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
          {blocks.map((block) => (
            <BlockView
              key={block.id}
              block={block}
              folded={folded}
              picker={
                block.type === "question" ? (
                  <QuestionCard sessionId={sessionId} block={block} live={live} />
                ) : block.type === "permission" && block.id === pendingId ? (
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
          {requestsError ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-md border border-status-danger/40 px-3 py-1 font-mono text-xs text-status-danger-fg"
            >
              Could not read the pending questions: {requestsError}
              <Button size="xs" variant="outline" onClick={readRequests}>
                Retry
              </Button>
            </div>
          ) : null}
          {child
            ? null
            : restoredQuestions(requests, transcript).map((block) => (
                <BlockView
                  key={block.id}
                  block={block}
                  folded={folded}
                  picker={<QuestionCard sessionId={sessionId} block={block} live={live} />}
                />
              ))}
          {transcript.queuedPrompts.map((prompt) => (
            <div key={prompt.id} data-block="prompt">
              <PromptBox text={prompt.text} queued />
            </div>
          ))}
        </div>
        {children.length > 0 ? (
          <ViewBar
            agents={children}
            shown={child?.id}
            asking={pending?.askedBy?.id}
            onShow={showView}
          />
        ) : null}
        <StatusRow
          status={status}
          turn={
            running
              ? { tool: runningTool(transcript), seconds: elapsedSeconds(turnStartedAt, now) }
              : undefined
          }
          tokens={tokenTotals(transcript, usage)}
        />
        {child ? (
          <div className="border-t px-3 py-2 font-mono text-xs text-muted-foreground">
            read-only · tab next view · {childCancellable ? "esc cancel" : "esc back"}
          </div>
        ) : (
          <>
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
                  onChange={(event) => edit(event.target.value, event.target.selectionStart)}
                  onSelect={(event) => setCursor(event.currentTarget.selectionStart)}
                  onKeyDown={onFieldKey}
                  className="block max-h-40 min-h-10 w-full resize-none rounded-md border border-input bg-transparent px-2.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
                />
                {live ? null : (
                  <p className="pt-1 text-xs text-muted-foreground">
                    This session has ended. Resume it to talk to it.
                  </p>
                )}
                {suggestions ? (
                  <SuggestionList commands={suggestions} picked={pickedSuggestion} />
                ) : null}
              </div>
              {running ? (
                <Button size="sm" variant="outline" onClick={cancel} aria-label="Cancel the turn">
                  <SquareIcon />
                  Cancel
                </Button>
              ) : null}
            </div>
            <Footer hints={footerHints({ live, running, asking: pending !== undefined, folded })} />
          </>
        )}
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

/** The views of a session with subagents: `main`, then each child with the mark of its state. */
function ViewBar({
  agents,
  shown,
  asking,
  onShow,
}: {
  agents: Transcript["subagents"]
  shown: string | undefined
  /** The child whose permission question waits. */
  asking: string | undefined
  onShow: (to: string | undefined) => void
}) {
  const views = [
    { id: undefined, label: "main" },
    ...agents.map((one) => ({
      id: one.id,
      label: `${stateMark(one.state)} ${one.title}${one.id === asking ? " ?" : ""}`,
    })),
  ]
  return (
    <div
      role="tablist"
      aria-label="Console views"
      className="flex flex-wrap gap-1 border-t px-2 py-1 font-mono text-xs"
    >
      {views.map((one) => (
        <button
          key={one.id ?? ""}
          type="button"
          role="tab"
          aria-selected={one.id === shown}
          tabIndex={-1}
          onClick={() => onShow(one.id)}
          className={cn(
            "rounded px-1.5 py-0.5",
            one.id === shown
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-muted",
          )}
        >
          {one.label}
        </button>
      ))}
    </div>
  )
}

/** The commands a typed `/word` names, while the cursor is still in that word. */
function suggest(
  commands: AgentCommand[],
  draft: string,
  cursor: number,
): AgentCommand[] | undefined {
  if (commands.length === 0 || !draft.startsWith("/") || /\s/.test(draft.slice(0, cursor))) {
    return undefined
  }
  const typed = (draft.split(/\s/)[0] ?? "").slice(1).toLowerCase()
  const matching = commands.filter((command) => command.name.toLowerCase().includes(typed))
  const first = matching.filter((command) => command.name.toLowerCase().startsWith(typed))
  return [...first, ...matching.filter((command) => !first.includes(command))].slice(0, SUGGESTIONS)
}

function SuggestionList({ commands, picked }: { commands: AgentCommand[]; picked: number }) {
  if (commands.length === 0) {
    return <p className="pt-1 font-mono text-xs text-muted-foreground">no matching command</p>
  }
  return (
    <div
      role="listbox"
      aria-label="Slash commands"
      className="mt-1 space-y-0.5 rounded-md border p-1 font-mono text-xs"
    >
      {commands.map((command, index) => (
        <div
          key={command.name}
          role="option"
          aria-selected={index === picked}
          tabIndex={-1}
          className={cn(
            "flex items-baseline gap-2 rounded px-2 py-0.5",
            index === picked && "bg-primary text-primary-foreground",
          )}
        >
          <span className="shrink-0">/{command.name}</span>
          <span className="min-w-0 truncate opacity-70">{command.description}</span>
          {command.hint ? <span className="shrink-0 opacity-70">{command.hint}</span> : null}
        </div>
      ))}
    </div>
  )
}

function elapsedSeconds(since: string | undefined, now: number): number | undefined {
  if (since === undefined) return undefined
  const started = Date.parse(since)
  return Number.isNaN(started) ? undefined : Math.max(0, Math.floor((now - started) / 1000))
}

/** `12s`, or `1m 04s` past a minute: the CLI's turn clock. */
function clock(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`
}

function StatusRow({
  status,
  turn,
  tokens,
}: {
  status: SessionStatus
  turn?: { tool?: string; seconds?: number }
  tokens: TokenTotals
}) {
  const spent = tokens.input > 0 || tokens.output > 0
  return (
    <section
      aria-label="Session status"
      className="flex items-center gap-3 border-t px-3 py-1 font-mono text-xs text-muted-foreground"
    >
      <span>{status}</span>
      {turn ? (
        <span className="flex items-center gap-1.5">
          <Loader2Icon className="size-3 animate-spin" aria-hidden />
          {turn.tool === undefined ? "thinking" : `running ${turn.tool}`}
          {turn.seconds === undefined ? null : <span>{clock(turn.seconds)}</span>}
        </span>
      ) : null}
      {spent ? (
        <span className="ml-auto">
          ↑{formatTokens(tokens.input)} ↓{formatTokens(tokens.output)}
        </span>
      ) : null}
    </section>
  )
}

function footerHints(state: {
  live: boolean
  running: boolean
  asking: boolean
  folded: boolean
}): string[] {
  const keys = state.asking
    ? ["up/down or 1-9 choose", "enter answer"]
    : state.running
      ? ["esc cancel", "enter send", "shift+enter newline"]
      : state.live
        ? ["enter send", "shift+enter newline"]
        : []
  return [...keys, state.folded ? "ctrl-o unfold" : "ctrl-o fold"]
}

function Footer({ hints }: { hints: string[] }) {
  return (
    <p className="border-t px-3 py-1 font-mono text-[11px] text-muted-foreground">
      {hints.join(" · ")}
    </p>
  )
}

/** The lines a fold keeps of a text, and how many it hides. */
function clip(
  text: string,
  folded: boolean,
  limit: number,
  keep: "first" | "last",
): { text: string; hidden: number } {
  const lines = text.split("\n")
  if (lines.at(-1) === "") lines.pop()
  if (!folded || lines.length <= limit) return { text, hidden: 0 }
  const shown = keep === "first" ? lines.slice(0, limit) : lines.slice(-limit)
  return { text: shown.join("\n"), hidden: lines.length - limit }
}

function Hidden({ count, keep }: { count: number; keep: "first" | "last" }) {
  if (count === 0) return null
  const noun = count === 1 ? "line" : "lines"
  return (
    <div className="font-mono text-[11px] text-muted-foreground">
      … {count} {keep === "first" ? "more" : "earlier"} {noun} folded
    </div>
  )
}

function PromptBox({
  text,
  daemon,
  queued,
  folded = false,
}: {
  text: string
  daemon?: boolean
  queued?: boolean
  folded?: boolean
}) {
  const shown = clip(text, folded && daemon === true, FOLDED_PROMPT_LINES, "first")
  return (
    <div className="rounded-md border-l-2 border-primary bg-muted/50 px-3 py-2">
      {daemon ? (
        <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
          daemon
        </span>
      ) : null}
      {queued ? (
        <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
          queued
        </span>
      ) : null}
      <p className="whitespace-pre-wrap break-words">{shown.text}</p>
      <Hidden count={shown.hidden} keep="first" />
    </div>
  )
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

function BlockView({
  block,
  picker,
  folded,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
}) {
  return (
    <div data-block={block.type}>
      {block.subagent ? (
        <div className="pb-0.5 font-mono text-[11px] text-muted-foreground">
          ↳ {block.subagent.title}
        </div>
      ) : null}
      <BlockBody block={block} picker={picker} folded={folded} />
    </div>
  )
}

function BlockBody({
  block,
  picker,
  folded,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
}) {
  switch (block.type) {
    case "prompt":
      return <PromptBox text={block.text} daemon={block.daemon} folded={folded} />
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
          {block.tool.output ? <Output text={block.tool.output} folded={folded} /> : null}
          {block.tool.diff ? <Diff text={block.tool.diff} folded={folded} /> : null}
        </div>
      )
    case "permission":
      return (
        <div className="rounded-md border border-status-warn/40 bg-status-warn-soft/30 px-3 py-2 text-xs">
          <div className="font-medium">Permission · {block.title}</div>
          {block.command ? <Output text={block.command} /> : null}
          {block.diff ? <Diff text={block.diff} folded={folded} /> : null}
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
    case "question":
      return picker
    case "note":
      return <div className="font-mono text-xs text-muted-foreground">· {block.text}</div>
    case "child":
      return <div className="font-mono text-xs text-muted-foreground">{block.text}</div>
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

/** Output keeps its last lines where folded: what a command ended on. `folded` is absent for a command. */
function Output({ text, folded = false }: { text: string; folded?: boolean }) {
  const shown = clip(text, folded, FOLDED_OUTPUT_LINES, "last")
  return (
    <div className="mt-1.5">
      <Hidden count={shown.hidden} keep="last" />
      <pre className="max-h-64 overflow-auto rounded bg-muted/50 p-2 font-mono text-xs whitespace-pre-wrap break-words">
        {shown.text}
      </pre>
    </div>
  )
}

/** A diff keeps its first lines where folded. */
function Diff({ text, folded }: { text: string; folded: boolean }) {
  const shown = clip(text, folded, FOLDED_DIFF_LINES, "first")
  return (
    <div className="mt-1.5">
      <pre className="max-h-64 overflow-auto rounded bg-muted/50 p-2 font-mono text-xs">
        {shown.text.split("\n").map((line, index) => (
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
      <Hidden count={shown.hidden} keep="first" />
    </div>
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

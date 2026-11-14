/**
 * A session's console, drawn by the app: the console stream
 * (`console-stream.ts`) folded into blocks (`transcript.ts`), an input box
 * that posts to `/console/input`, and a cancel that posts to
 * `/console/cancel`.
 *
 * Each pane holds one stream for as long as it is on screen, so an Expand
 * into the modal and a Collapse back each close the old stream and open a
 * new one, and leaving the tab closes it. A snapshot replaces the transcript,
 * so a reconnect shows the session afresh rather than twice. What the reader
 * has set up, the draft, the place and the folds, outlives the pane in
 * `console-view-state.ts`.
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
 * session's own, and one per child. A click and the arrow keys inside the bar
 * switch the view; the bar is one Tab stop. A child's view is read-only: Escape there goes back to `main`, or
 * cancels the child where it runs and said it may be cancelled.
 *
 * The transcript is the console's one scroll box: blocks, code and diffs grow
 * with their content and wrap, and none scrolls inside. Tool details, thoughts
 * and daemon prompts fold, each on its own header, into a summary line; the
 * folds are retained with the rest of the view. A reader who scrolls away from
 * the newest output gets a Jump to latest button, marked when output has come
 * in since.
 *
 * The extras the CLI's attach pane has (`crates/ariadne-console/src/tui/`):
 * one fold toggle, the agent's slash commands as a suggestion list over the
 * input, a status row, a sent prompt shown at once as `queued`, and a footer
 * of the keys the state takes.
 */

import {
  ArrowDownIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  Loader2Icon,
  Maximize2Icon,
  Minimize2Icon,
  PlugZapIcon,
  SendIcon,
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

import { toast } from "sonner"

import { type AgentRequestDto, api, type SessionDto, type SessionStatus, unwrap } from "@/api"
import { CodeBlock } from "@/components/code-block"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { copyText } from "@/lib/clipboard"
import { cn, describeError, formatTokens } from "@/lib/format"
import { languageForPath } from "@/lib/highlight"
import { useBaseUrl } from "@/stores/settings"

import { SwitchSessionDialog, useResumeAction } from "../session-actions"
import { isLiveStatus } from "../session-display"
import { ComposerMenu } from "./composer-menu"
import { ConsoleStream, type ConsoleStreamStatus, consoleStreamUrl } from "./console-stream"
import {
  type ConsoleViewState,
  consoleViewKey,
  type FailedSend,
  readViewState,
  updateViewState,
  watchViewState,
} from "./console-view-state"
import { Foldable } from "./foldable"
import {
  answerRequest,
  openQuestion,
  pendingRequests,
  QuestionCard,
  restoredQuestions,
} from "./questions"
import {
  type AgentCommand,
  appendError,
  applyEvent,
  applySnapshot,
  type Block,
  blocksText,
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

/** The characters of a thought or a daemon prompt its folded summary quotes. */
const SUMMARY_CHARS = 90

/** The most commands the suggestion list shows. */
const SUGGESTIONS = 8

/** The lines the composer grows to before it scrolls inside. */
const COMPOSER_LINES = 6

/** Numbers the sends of every console, so a retained failure never shares an id with a new send. */
let sendCount = 0

export function SessionConsole({
  sessionId,
  status,
  autoFocus,
  usage,
  className,
  session,
  canSwitch = true,
  onResumed,
  onSwitched,
}: {
  sessionId: string
  status: SessionStatus
  /** Hand the input the keyboard as soon as it is on screen. */
  autoFocus?: boolean
  /** What the session itself reports spending, the floor of the status row's totals. */
  usage?: TokenTotals
  className?: string
  /** The session, for the menu's Switch; without it the menu offers none. */
  session?: SessionDto
  /** False where the session's goal is cancelled and a switch has nowhere to go. */
  canSwitch?: boolean
  /** Where to go once a resume hands the session back. */
  onResumed?: (session: SessionDto) => void
  /** Where to go once a switch creates a successor session. */
  onSwitched?: (session: SessionDto) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const menu = { session, canSwitch, onResumed, onSwitched }

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
            {...menu}
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
      {...menu}
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
  session,
  canSwitch,
  onResumed,
  onSwitched,
  onExpand,
  onCollapse,
}: {
  sessionId: string
  status: SessionStatus
  autoFocus?: boolean
  usage?: TokenTotals
  className?: string
  session?: SessionDto
  canSwitch: boolean
  onResumed?: (session: SessionDto) => void
  onSwitched?: (session: SessionDto) => void
  onExpand?: () => void
  onCollapse?: () => void
}) {
  const baseUrl = useBaseUrl()
  const key = consoleViewKey(baseUrl, sessionId)
  const keyRef = useRef(key)
  keyRef.current = key
  const [transcript, setTranscript] = useState<Transcript>(emptyTranscript)
  /** The session's pending requests, as read with the last snapshot. */
  const [requests, setRequests] = useState<AgentRequestDto[]>([])
  /** Why the last read of the pending requests failed; none once one succeeds. */
  const [requestsError, setRequestsError] = useState<string | null>(null)
  /** Counts the reads of the pending requests, so only the latest one lands. */
  const reads = useRef(0)
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [streamError, setStreamError] = useState<string | null>(null)
  const [draft, setDraft] = useState(() => readViewState(key).draft)
  const [selected, setSelected] = useState(0)
  const [folded, setFolded] = useState(() => readViewState(key).folded)
  const [blockFolds, setBlockFolds] = useState(() => readViewState(key).blockFolds)
  const [cursor, setCursor] = useState(0)
  const [suggested, setSuggested] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  const [now, setNow] = useState(Date.now)
  const [answering, setAnswering] = useState<string | null>(null)
  /** The child whose view is shown; none for `main`. */
  const [shown, setShown] = useState<string | undefined>(() => readViewState(key).shown)
  /** True once a snapshot has said which children there are. */
  const [snapshotted, setSnapshotted] = useState(false)
  /** The sends the daemon refused, kept for a Retry or an Edit. */
  const [failed, setFailed] = useState<FailedSend[]>(() => readViewState(key).failed)
  /** The requests an accepted answer is in for, whose resolved event has yet to land. */
  const [answered, setAnswered] = useState<ReadonlySet<string>>(new Set())
  const [menuOpen, setMenuOpen] = useState(false)
  const [switchOpen, setSwitchOpen] = useState(false)
  const caret = useRef<number | null>(null)
  /** Where a pending restore of the reader's place goes, until the transcript has blocks. */
  const restore = useRef<number | null>(null)
  /** The oldest event id to load back to before the reader's place is restored. */
  const backfill = useRef<string | undefined>(undefined)
  /** The block to put back where the reader had it, once the history is back. */
  const anchored = useRef<ConsoleViewState["anchor"]>(undefined)
  const body = useRef<HTMLDivElement>(null)
  const log = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)
  const collapseButton = useRef<HTMLButtonElement>(null)
  const root = useRef<HTMLDivElement>(null)
  const following = useRef(readViewState(key).following)
  /** True while the reader is away from the newest output, and whether any came in since. */
  const [away, setAway] = useState(() => !readViewState(key).following)
  const [unread, setUnread] = useState(false)
  const seen = useRef(transcript)
  /** The block the reader was last on, and where its top sat, for layout changes above it. */
  const placed = useRef<ConsoleViewState["anchor"]>(undefined)
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

  // Another daemon or session in the same pane brings its own retained state.
  const [loadedKey, setLoadedKey] = useState(key)
  if (loadedKey !== key) {
    setLoadedKey(key)
    const kept = readViewState(key)
    setDraft(kept.draft)
    setFolded(kept.folded)
    setBlockFolds(kept.blockFolds)
    setShown(kept.shown)
    setFailed(kept.failed)
    setAnswered(new Set())
  }

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
    setSnapshotted(false)
    resetHistory()
    const kept = readViewState(consoleViewKey(baseUrl, sessionId))
    following.current = kept.following
    restore.current = kept.following ? null : kept.scrollTop
    backfill.current = kept.following ? undefined : kept.oldest
    anchored.current = kept.following ? undefined : kept.anchor
    const stream = new ConsoleStream(consoleStreamUrl(baseUrl, sessionId), {
      onSnapshot: (events) => {
        resetHistory()
        setSnapshotted(true)
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
  useLayoutEffect(() => {
    const el = log.current
    if (!el) return
    const kept = anchor.current
    anchor.current = null
    if (restore.current !== null) {
      if (transcript.blocks.length === 0) return
      if (backfill.current !== undefined) {
        // The history the reader had loaded comes back first, so the offset
        // lands on the same content.
        const oldest = oldestEventId(transcript)
        const loaded = reached.current || history.state === "error"
        if (!loaded && oldest !== undefined && oldest > backfill.current) return
        backfill.current = undefined
      }
      el.scrollTop = restore.current
      // The block the reader was on, not the old offset, says where: a page
      // that crosses the old boundary shifts everything below it.
      const place = anchored.current
      const at = place && blockOffset(el, place.blockId)
      if (place && at !== undefined) el.scrollTop += at - place.offset
      anchored.current = undefined
      restore.current = null
    } else if (kept) el.scrollTop = kept.top + (el.scrollHeight - kept.height)
    else if (following.current) pin(el, pinned)
    else if (seen.current !== transcript) setUnread(true)
    seen.current = transcript
  }, [transcript, history])

  // A fold changes what is below the reader's place: a reader at the newest
  // output stays there.
  // biome-ignore lint/correctness/useExhaustiveDependencies: a fold is the trigger, the effect reads no value from it
  useLayoutEffect(() => {
    const el = log.current
    if (el && following.current && restore.current === null) pin(el, pinned)
  }, [folded, blockFolds])

  // Blocks above a reader who scrolled away change height when they fold,
  // unfold or take output; the block the reader was on stays where it was.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the changes are the trigger, the effect reads none of them
  useLayoutEffect(() => {
    const el = log.current
    const place = placed.current
    if (!el || !place || following.current || restore.current !== null) return
    const at = blockOffset(el, place.blockId)
    if (at !== undefined && at !== place.offset) el.scrollTop += at - place.offset
  }, [transcript, folded, blockFolds])

  // A panel that is resized, or a block that grows after it is drawn, keeps a
  // following reader at the newest output too.
  // biome-ignore lint/correctness/useExhaustiveDependencies: new blocks are the trigger, so they are observed
  useEffect(() => {
    const el = log.current
    if (!el || typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver(() => {
      if (following.current && restore.current === null) pin(el, pinned)
    })
    observer.observe(el)
    for (const inner of Array.from(el.children)) observer.observe(inner)
    return () => observer.disconnect()
  }, [transcript.blocks.length])

  const pinned = useRef<number | null>(null)

  const jumpToLatest = () => {
    const el = log.current
    if (!el) return
    following.current = true
    pin(el, pinned)
    setAway(false)
    setUnread(false)
    updateViewState(keyRef.current, {
      scrollTop: el.scrollTop,
      anchor: firstInView(el),
      following: true,
    })
  }

  // Load older pages until the history the reader had open is back.
  useEffect(() => {
    if (!snapshotted || backfill.current === undefined || history.state !== "idle") return
    const oldest = oldestEventId(transcript)
    if (oldest !== undefined && oldest > backfill.current) void loadOlder()
  }, [snapshotted, transcript, history, loadOlder])

  // What is loaded is retained, once a restore is not waiting on it.
  useEffect(() => {
    if (snapshotted && backfill.current === undefined)
      updateViewState(keyRef.current, { oldest: oldestEventId(transcript) })
  }, [snapshotted, transcript])

  const children = transcript.subagents
  // A snapshot that no longer holds the child shown goes back to `main`.
  const child = children.find((one) => one.id === shown)
  const blocks = view(transcript, child?.id)
  const runningChildren = children.filter((one) => one.state === "running").length
  const childCancellable = child?.cancel === true && child.state === "running"
  useEffect(() => {
    if (snapshotted && shown !== undefined && child === undefined) {
      setShown(undefined)
      updateViewState(keyRef.current, { shown: undefined })
    }
  }, [snapshotted, shown, child])

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
    updateViewState(keyRef.current, { draft: text })
    setCursor(at)
    setSuggested(0)
    setDismissed(false)
  }

  /** Put `text` in the composer with the caret at `at`, and hand it the keyboard. */
  function fill(text: string, at: number) {
    edit(text, at)
    caret.current = at
    field.current?.focus()
  }

  // The caret goes where the last insertion put it, once the new text is drawn.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the draft is what moves the caret
  useLayoutEffect(() => {
    const el = field.current
    if (el && caret.current !== null) {
      el.setSelectionRange(caret.current, caret.current)
      caret.current = null
    }
  }, [draft])

  // The composer is one line, grows with its text to six, then scrolls inside.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the draft is what changes the height
  useLayoutEffect(() => {
    const el = field.current
    if (!el) return
    const style = getComputedStyle(el)
    const line = Number.parseFloat(style.lineHeight) || 20
    const chrome =
      Number.parseFloat(style.paddingTop || "0") +
      Number.parseFloat(style.paddingBottom || "0") +
      Number.parseFloat(style.borderTopWidth || "0") +
      Number.parseFloat(style.borderBottomWidth || "0")
    const most = line * COMPOSER_LINES + chrome
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, most)}px`
    el.style.overflowY = el.scrollHeight > most ? "auto" : "hidden"
  }, [draft, child])

  // A question restored from the requests list was asked before the page in
  // view, so it comes first: the composer answers the newest.
  const restored = child ? [] : restoredQuestions(requests, transcript)
  const question = live && !child ? openQuestion([...restored, ...blocks], answered) : undefined
  const questionId = question?.requestId

  /** Deliver one message: to the open question as its answer, else to the console. */
  function dispatch(text: string, requestId: string | undefined) {
    // Each send is its own entry, so a failure takes back that send alone.
    sendCount += 1
    const id = `queued:${sendCount}`
    // A failure can land after the pane is gone: it goes to the retained state.
    const owner = keyRef.current
    if (requestId === undefined) setTranscript((current) => queuePrompt(current, text, id))
    const sent =
      requestId === undefined ? post(text) : answerRequest(sessionId, requestId, { text })
    sent.then(
      () => {
        if (requestId !== undefined) setAnswered((now) => new Set(now).add(requestId))
      },
      (error) => {
        if (requestId === undefined) setTranscript((current) => dequeuePrompt(current, id, text))
        const what = requestId === undefined ? "send the prompt" : "send the answer"
        changeFailed(owner, (now) => [
          ...now,
          { id, text, requestId, message: `Could not ${what}: ${describeError(error)}` },
        ])
      },
    )
  }

  // A send that fails after this pane replaced another lands in the retained
  // state, and this pane follows it.
  useEffect(() => {
    const read = () => setFailed(readViewState(key).failed)
    read()
    return watchViewState(key, read)
  }, [key])

  /** Change the refused sends of the console `owner`, shown or not. */
  function changeFailed(owner: string, change: (now: FailedSend[]) => FailedSend[]) {
    const next = change(readViewState(owner).failed)
    updateViewState(owner, { failed: next })
  }

  /** The text with the command's first word completed to `name`, the rest kept. */
  function complete(name: string) {
    const end = draft.search(/\s/)
    const rest = end === -1 ? "" : draft.slice(end)
    const head = `/${name}`
    if (rest === "") fill(`${head} `, head.length + 1)
    else fill(`${head}${rest}`, head.length + (rest.startsWith(" ") ? 1 : 0))
  }

  function send() {
    const text = draft
    if (!live || text.trim().length === 0) return
    edit("", 0)
    dispatch(text, questionId)
    field.current?.focus()
  }

  function retry(one: FailedSend) {
    changeFailed(keyRef.current, (now) => now.filter((other) => other.id !== one.id))
    dispatch(one.text, one.requestId)
  }

  /** Take a failed message back into the composer, in front of any newer draft. */
  function editFailed(one: FailedSend) {
    changeFailed(keyRef.current, (now) => now.filter((other) => other.id !== one.id))
    const text = draft.length === 0 ? one.text : `${one.text}\n${draft}`
    fill(text, one.text.length)
  }

  /** A command's name into the composer, before whatever is already drafted. */
  function insertCommand(name: string) {
    const head = `/${name} `
    fill(`${head}${draft}`, head.length)
  }

  /** The suggestion list's keys: Up and Down move, Enter completes. True when taken. */
  function suggestKey(event: KeyboardEvent): boolean {
    if (!suggestions) return false
    const count = suggestions.length
    if (event.key === "ArrowUp" && count > 0) setSuggested(Math.max(pickedSuggestion - 1, 0))
    else if (event.key === "ArrowDown" && count > 0)
      setSuggested(Math.min(pickedSuggestion + 1, count - 1))
    else if (event.key === "Enter" && !event.shiftKey && count > 0) {
      const command = suggestions[pickedSuggestion]
      if (command) complete(command.name)
    } else return false
    event.preventDefault()
    return true
  }

  function markAnswered(requestId: string) {
    setAnswered((now) => new Set(now).add(requestId))
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
  const showView = useCallback((to: string | undefined, keepFocus = false) => {
    setShown(to)
    updateViewState(keyRef.current, { shown: to })
    if (keepFocus) return
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
    if (
      event.key === "Enter" &&
      (event.shiftKey || event.nativeEvent.isComposing || event.keyCode === 229)
    )
      return
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
    child,
    childCancellable,
    suggesting: false,
    dismiss: () => {},
  })
  latest.current = {
    running,
    cancel,
    onCollapse,
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
      const { child, childCancellable } = latest.current
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

  /** Fold or unfold one block, and keep the choice with the console's view. */
  const toggleBlock = useCallback((id: string, folded: boolean) => {
    const next = { ...readViewState(keyRef.current).blockFolds, [id]: !folded }
    setBlockFolds(next)
    updateViewState(keyRef.current, { blockFolds: next })
  }, [])

  /** Fold or unfold everything, individual preferences included. */
  const toggleFold = useRef(() => {})
  toggleFold.current = () => {
    const next = !folded
    setFolded(next)
    setBlockFolds({})
    updateViewState(keyRef.current, { folded: next, blockFolds: {} })
  }

  // Ctrl-O folds from anywhere in the console, the header's controls included.
  useEffect(() => {
    const el = root.current
    if (!el) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key.toLowerCase() !== "o" || !event.ctrlKey || event.metaKey || event.altKey) return
      event.preventDefault()
      toggleFold.current()
    }
    el.addEventListener("keydown", onKey)
    return () => el.removeEventListener("keydown", onKey)
  }, [])

  const resume = useResumeAction(onResumed)
  const viewName = child ? child.title : "main"
  const menu = (
    <ComposerMenu
      open={menuOpen}
      onOpenChange={setMenuOpen}
      commands={child ? [] : transcript.availableCommands}
      onCommand={insertCommand}
      onSwitch={session && canSwitch ? () => setSwitchOpen(true) : undefined}
      onResume={live ? undefined : () => resume.resume(sessionId)}
      resumePending={resume.pending}
      folded={folded}
      onToggleFold={() => toggleFold.current()}
      copyLabel={`Copy loaded transcript (${viewName})`}
      onCopy={() => {
        void copyText(blocksText(blocks)).then((copied) =>
          copied
            ? toast.success("Transcript copied", { description: `the loaded ${viewName} view` })
            : toast.error("Could not copy the transcript"),
        )
      }}
      returnFocus={() => field.current}
    />
  )

  return (
    <div
      ref={root}
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background text-sm text-foreground shadow-xs",
        className,
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-1 border-b px-3 py-1.5 font-mono text-xs text-muted-foreground">
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
          onClick={() => toggleFold.current()}
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
        <div className="relative flex min-h-0 flex-1 flex-col">
          <div
            ref={log}
            role="log"
            aria-label="Console transcript"
            className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3"
            onScroll={(event) => {
              const el = event.currentTarget
              // A restore of the reader's place waits on the blocks: until it
              // lands, a scroll is the box shrinking, not the reader.
              if (restore.current !== null) return
              // Our own scroll to the newest output lands after the content may
              // have grown again; it is not the reader leaving.
              placed.current = firstInView(el)
              if (el.scrollTop === pinned.current) {
                pinned.current = null
              } else {
                following.current =
                  el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_SLACK_PX
                setAway(!following.current)
                if (following.current) setUnread(false)
              }
              updateViewState(keyRef.current, {
                scrollTop: el.scrollTop,
                anchor: firstInView(el),
                following: following.current,
              })
              if (el.scrollTop <= TOP_SLACK_PX) void loadOlder()
            }}
          >
            {blocks.map((block) => (
              <BlockView
                key={block.id}
                block={block}
                folded={blockFolds[block.id] ?? folded}
                onToggle={toggleBlock}
                picker={
                  block.type === "question" ? (
                    <QuestionCard
                      sessionId={sessionId}
                      block={block}
                      live={live}
                      sent={answered.has(block.requestId)}
                      onAnswered={markAnswered}
                    />
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
              : restored.map((block) => (
                  <BlockView
                    key={block.id}
                    block={block}
                    folded={blockFolds[block.id] ?? folded}
                    onToggle={toggleBlock}
                    picker={
                      <QuestionCard
                        sessionId={sessionId}
                        block={block}
                        live={live}
                        sent={answered.has(block.requestId)}
                        onAnswered={markAnswered}
                      />
                    }
                  />
                ))}
            {transcript.queuedPrompts.map((prompt) => (
              <div key={prompt.id} data-block="prompt">
                <PromptBox text={prompt.text} queued />
              </div>
            ))}
            {failed.map((one) => (
              <FailedMessage
                key={one.id}
                one={one}
                onRetry={() => retry(one)}
                onEdit={() => editFailed(one)}
              />
            ))}
          </div>
          {away ? (
            <Button
              size="sm"
              variant="secondary"
              onClick={jumpToLatest}
              className="absolute right-3 bottom-2 shadow-md"
            >
              <ArrowDownIcon />
              Jump to latest
              {unread ? (
                <span
                  role="status"
                  className="rounded-full bg-primary px-1.5 text-[10px] text-primary-foreground"
                >
                  New output
                </span>
              ) : null}
            </Button>
          ) : null}
        </div>
        {children.length > 0 ? (
          <ViewBar
            agents={children}
            shown={child?.id}
            asking={pending?.askedBy?.id}
            onShow={(to, keepFocus) => showView(to, keepFocus)}
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
            <div className="flex items-center gap-2">
              {menu}
              <span>
                read-only · arrows switch view · {childCancellable ? "esc cancel" : "esc back"}
              </span>
            </div>
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-end gap-2 border-t p-2">
              {menu}
              <div className="min-w-48 flex-1 basis-48">
                <textarea
                  ref={field}
                  aria-label="Message the agent"
                  value={draft}
                  disabled={!live}
                  rows={1}
                  placeholder={
                    !live
                      ? undefined
                      : question
                        ? "Answer the question. Enter sends your answer."
                        : "Message the agent. Enter sends, Shift+Enter adds a line."
                  }
                  onChange={(event) => edit(event.target.value, event.target.selectionStart)}
                  onSelect={(event) => setCursor(event.currentTarget.selectionStart)}
                  onKeyDown={onFieldKey}
                  className="block w-full resize-none rounded-md border border-input bg-transparent px-2.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
                />
                {live ? null : (
                  <p className="pt-1 text-xs text-muted-foreground">
                    This session has ended. Resume it to talk to it.
                  </p>
                )}
                {suggestions ? (
                  <SuggestionList
                    commands={suggestions}
                    picked={pickedSuggestion}
                    onPick={(command) => complete(command.name)}
                  />
                ) : null}
              </div>
              {running ? (
                <Button size="sm" variant="outline" onClick={cancel} aria-label="Stop the turn">
                  <SquareIcon />
                  Stop
                </Button>
              ) : null}
              <Button
                size="sm"
                onClick={send}
                disabled={!live || draft.trim().length === 0}
                aria-label="Send"
              >
                <SendIcon />
                Send
              </Button>
            </div>
            <Footer hints={footerHints({ live, running, asking: pending !== undefined, folded })} />
          </>
        )}
      </div>
      {session ? (
        <SwitchSessionDialog
          session={session}
          open={switchOpen}
          onOpenChange={setSwitchOpen}
          onSwitched={onSwitched}
        />
      ) : null}
    </div>
  )
}

function FailedMessage({
  one,
  onRetry,
  onEdit,
}: {
  one: FailedSend
  onRetry: () => void
  onEdit: () => void
}) {
  return (
    <div
      role="alert"
      data-block="failed"
      className="space-y-1.5 rounded-md border border-status-danger/40 bg-status-danger-soft/40 px-3 py-2 text-xs"
    >
      <p className="text-status-danger-fg">{one.message}</p>
      <p className="whitespace-pre-wrap break-words text-sm">{one.text}</p>
      <div className="flex gap-1.5">
        <Button size="xs" variant="outline" onClick={onRetry}>
          Retry
        </Button>
        <Button size="xs" variant="outline" onClick={onEdit}>
          Edit
        </Button>
      </div>
    </div>
  )
}

/**
 * Scroll to the newest output, and remember where to: the scroll event that
 * follows is ours, not the reader's. A box with nothing to scroll sends none.
 */
function pin(el: HTMLElement, pinned: { current: number | null }) {
  el.scrollTop = el.scrollHeight
  pinned.current = el.scrollTop > 0 ? el.scrollTop : null
}

/** Where the top of block `blockId` sits below the top of the scroll box, if it is drawn. */
function blockOffset(log: HTMLElement, blockId: string): number | undefined {
  const block = Array.from(log.querySelectorAll<HTMLElement>("[data-block-id]")).find(
    (one) => one.dataset.blockId === blockId,
  )
  return block ? block.getBoundingClientRect().top - log.getBoundingClientRect().top : undefined
}

/** The first block that reaches into view, with where its top sits. */
function firstInView(log: HTMLElement): { blockId: string; offset: number } | undefined {
  const top = log.getBoundingClientRect().top
  for (const block of log.querySelectorAll<HTMLElement>("[data-block-id]")) {
    const rect = block.getBoundingClientRect()
    if (rect.bottom > top && block.dataset.blockId !== undefined)
      return { blockId: block.dataset.blockId, offset: rect.top - top }
  }
  return undefined
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

/**
 * The views of a session with subagents: `main`, then each child with the
 * mark of its state. The bar is one stop in the Tab order: the arrow keys,
 * Home and End move between its views inside it.
 */
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
  onShow: (to: string | undefined, keepFocus?: boolean) => void
}) {
  const bar = useRef<HTMLDivElement>(null)
  const views = [
    { id: undefined, label: "main" },
    ...agents.map((one) => ({
      id: one.id,
      label: `${stateMark(one.state)} ${one.title}${one.id === asking ? " ?" : ""}`,
    })),
  ]
  const at = Math.max(
    views.findIndex((one) => one.id === shown),
    0,
  )

  function onKeyDown(event: KeyboardEvent) {
    const last = views.length - 1
    const to =
      event.key === "ArrowRight"
        ? (at + 1) % views.length
        : event.key === "ArrowLeft"
          ? (at + last) % views.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : undefined
    if (to === undefined) return
    event.preventDefault()
    onShow(views[to]?.id, true)
    bar.current?.querySelectorAll<HTMLElement>('[role="tab"]')[to]?.focus()
  }

  return (
    <div
      ref={bar}
      role="tablist"
      aria-label="Console views"
      onKeyDown={onKeyDown}
      className="flex flex-wrap gap-1 border-t px-2 py-1 font-mono text-xs"
    >
      {views.map((one, index) => (
        <button
          key={one.id ?? ""}
          type="button"
          role="tab"
          aria-selected={one.id === shown}
          tabIndex={index === at ? 0 : -1}
          onClick={() => onShow(one.id)}
          className={cn(
            "rounded px-1.5 py-0.5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
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

function SuggestionList({
  commands,
  picked,
  onPick,
}: {
  commands: AgentCommand[]
  picked: number
  onPick: (command: AgentCommand) => void
}) {
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
          // A press must not take the keyboard from the composer, and a tap
          // or a click completes the command for editing.
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => onPick(command)}
          onKeyDown={() => undefined}
          className={cn(
            "flex cursor-pointer items-baseline gap-2 rounded px-2 py-1.5 hover:bg-muted",
            index === picked && "bg-primary text-primary-foreground hover:bg-primary",
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

/** The first line of a text, cut where a summary line has room for it. */
function firstLine(text: string): string {
  const line = text.split("\n").find((one) => one.trim() !== "") ?? ""
  return line.length > SUMMARY_CHARS ? `${line.slice(0, SUMMARY_CHARS).trimEnd()}…` : line.trim()
}

function lineCount(text: string): string {
  const count = text.replace(/\n$/, "").split("\n").length
  return `${count} ${count === 1 ? "line" : "lines"}`
}

/** What a person typed is a message: set apart and full size. */
function PromptBox({ text, queued }: { text: string; queued?: boolean }) {
  return (
    <div className="rounded-md border-l-2 border-primary bg-primary/10 px-3 py-2">
      {queued ? (
        <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
          queued
        </span>
      ) : null}
      <p className="whitespace-pre-wrap break-words">{text}</p>
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
  onToggle,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
  onToggle: (id: string, folded: boolean) => void
}) {
  return (
    <div data-block={block.type} data-block-id={block.id}>
      {block.subagent ? (
        <div className="pb-0.5 font-mono text-[11px] text-muted-foreground">
          ↳ {block.subagent.title}
        </div>
      ) : null}
      <BlockBody
        block={block}
        picker={picker}
        folded={folded}
        onToggle={() => onToggle(block.id, folded)}
      />
    </div>
  )
}

function BlockBody({
  block,
  picker,
  folded,
  onToggle,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
  onToggle: () => void
}) {
  switch (block.type) {
    case "prompt":
      return block.daemon ? (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          className="rounded-md border-l-2 bg-muted/40"
          summary={
            <>
              daemon prompt · {firstLine(block.text)} · {lineCount(block.text)}
            </>
          }
        >
          <p className="whitespace-pre-wrap break-words">{block.text}</p>
        </Foldable>
      ) : (
        <PromptBox text={block.text} />
      )
    case "message":
      return <Markdown>{block.text}</Markdown>
    case "thought":
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={<>thought · {firstLine(block.text)}</>}
        >
          <Markdown className="text-muted-foreground italic">{block.text}</Markdown>
        </Foldable>
      )
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
    case "tool": {
      const { tool } = block
      const body = tool.output || tool.diff
      const summary = (
        <>
          <ToolStatus status={tool.status} />
          {tool.kind ? ` · ${tool.kind}` : null}
          {" · "}
          <span className="font-medium text-foreground">{tool.title}</span>
          {body ? ` · ${lineCount(body)}` : null}
        </>
      )
      if (!body) {
        return (
          <div className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
            {summary}
          </div>
        )
      }
      // A file the tool read is shown in the file's language.
      const language =
        tool.kind === "read" ? languageForPath(tool.title.split(/\s+/).at(-1) ?? "") : undefined
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={summary}
          className="rounded-md border"
        >
          {tool.output ? <Output text={tool.output} language={language} /> : null}
          {tool.diff ? <Diff text={tool.diff} /> : null}
        </Foldable>
      )
    }
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

/** A tool's output or a command, whole: it grows with its content and wraps, and never scrolls inside. */
function Output({ text, language }: { text: string; language?: string }) {
  return <CodeBlock code={text} language={language} className="mt-1.5" />
}

/** A file change, with its added and removed lines colored. */
function Diff({ text }: { text: string }) {
  return <CodeBlock code={text} diff className="mt-1.5" />
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

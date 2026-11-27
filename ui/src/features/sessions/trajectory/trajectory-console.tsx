/**
 * The turn console: a session drawn from the daemon's trajectory (040) as
 * turns, each a prompt, one activity line and an answer. The session view
 * opens it in place of the console of today behind a hidden URL parameter.
 *
 * The daemon folds the events: this pane applies its snapshot and its
 * updates (`store.ts`) and folds nothing. A snapshot, the one a reconnect
 * brings included, replaces the state, so no item shows two times.
 *
 * The composer is the one of today: it sends, queues, cancels, retries and
 * edits a prompt, and lists the agent's slash commands. A pending
 * permission is a card above it, in the dock; Up, Down, 1 to 9 and Enter
 * answer it from the card and from an empty composer. Escape inside the
 * console closes the slash-command list, else cancels a running turn.
 *
 * A scroll to the top loads the older whole turns and keeps the reader's
 * place. The console follows the newest output while the reader is at the
 * bottom, and shows `Jump to latest` once the reader scrolls away.
 */

import { skipToken, useQuery } from "@tanstack/react-query"
import { ArrowDownIcon } from "lucide-react"
import {
  type KeyboardEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react"
import { toast } from "sonner"

import {
  api,
  type Host,
  hostBaseUrl,
  hostUnreachable,
  type QuestionDto,
  qk,
  type SessionDto,
  type SessionStatus,
  unwrap,
} from "@/api"
import { Button } from "@/components/ui/button"
import { copyText } from "@/lib/clipboard"
import { cn, describeError } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { PromptBox } from "../console/blocks"
import { Composer, suggest } from "../console/composer"
import { ComposerMenu } from "../console/composer-menu"
import type { ConsoleStreamStatus } from "../console/console-stream"
import {
  consoleViewKey,
  type FailedSend,
  readViewState,
  updateViewState,
  watchViewState,
} from "../console/console-view-state"
import { listQuestions } from "../console/form-questions"
import {
  elapsedSeconds,
  FailedMessage,
  type History,
  HistoryLine,
  StreamStatusLine,
} from "../console/status"
import type { PermissionOption, TokenTotals } from "../console/transcript"
import { SwitchSessionDialog, useResumeAction } from "../session-actions"
import { isLiveStatus } from "../session-display"
import { TrajectoryDock } from "./dock-slot"
import { TrajectoryFind } from "./find-slot"
import { StepWorkspace } from "./full-view"
import { PermissionCard } from "./permission-card"
import { TrajectoryStatus } from "./status-slot"
import {
  addStepPage,
  applySnapshot,
  applyUpdate,
  contentText,
  dequeuePrompt,
  emptyTrajectory,
  foldSteps,
  pendingPermissions,
  prependTurns,
  queuePrompt,
  shownTurns,
  type TrajectoryState,
} from "./store"
import { TrajectoryStream, trajectoryStreamUrl } from "./trajectory-stream"
import { type PageLoad, TurnView } from "./turn-view"

/** Numbers the sends of every console, so a refused send never shares an id with a new one. */
let sendCount = 0

/** How near the top, in pixels, still counts as reaching the oldest turn shown. */
const TOP_SLACK_PX = 8

/** How near the bottom, in pixels, still counts as reading the newest output. */
const FOLLOW_SLACK_PX = 24

const IDLE: PageLoad = { state: "idle" }

export function TrajectoryConsole({
  host,
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
  /** The host the session runs on: its stream and every post go there. */
  host: Host
  sessionId: string
  status: SessionStatus
  /** Hand the input the keyboard as soon as it is on screen. */
  autoFocus?: boolean
  /** What the session row reports spending, until the trajectory says. */
  usage?: TokenTotals
  className?: string
  /** The session, for the menu's Switch; without it the menu offers none. */
  session?: SessionDto
  /** False where the session's goal is cancelled and a switch has nowhere to go. */
  canSwitch?: boolean
  onResumed?: (session: SessionDto) => void
  onSwitched?: (session: SessionDto) => void
}) {
  // Read again when either address moves: the settings base URL for `local`,
  // the hosts list for a named host.
  useBaseUrl()
  useQuery({ queryKey: qk.hosts.list(), queryFn: skipToken })
  const baseUrl = hostBaseUrl(host)
  const unreachable = hostUnreachable(host)

  const [state, setState] = useState<TrajectoryState>(emptyTrajectory)
  const latest = useRef(state)
  latest.current = state
  const [streamStatus, setStreamStatus] = useState<ConsoleStreamStatus>("connecting")
  const [streamError, setStreamError] = useState<string | null>(null)
  const [questions, setQuestions] = useState<QuestionDto[]>([])
  const [open, setOpen] = useState<Record<string, boolean>>({})
  const [loads, setLoads] = useState<Record<string, PageLoad>>({})
  const [history, setHistory] = useState<History>({ state: "idle" })
  const key = consoleViewKey(host, baseUrl ?? "", sessionId)
  const keyRef = useRef(key)
  keyRef.current = key
  const [draft, setDraft] = useState(() => readViewState(key).draft)
  const [cursor, setCursor] = useState(0)
  const [suggested, setSuggested] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  const [failed, setFailed] = useState<FailedSend[]>(() => readViewState(key).failed)
  // Another daemon or session in the same pane brings its own retained composer.
  const [loadedKey, setLoadedKey] = useState(key)
  if (loadedKey !== key) {
    setLoadedKey(key)
    const kept = readViewState(key)
    setDraft(kept.draft)
    setFailed(kept.failed)
  }
  // A send that fails after this pane replaced another lands in the retained
  // state, and this pane follows it.
  useEffect(() => {
    const read = () => setFailed(readViewState(key).failed)
    read()
    return watchViewState(key, read)
  }, [key])
  const [selected, setSelected] = useState(0)
  const [answering, setAnswering] = useState<string | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [switchOpen, setSwitchOpen] = useState(false)
  const [now, setNow] = useState(Date.now)
  const [away, setAway] = useState(false)
  const [unread, setUnread] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const log = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)
  const caret = useRef<number | null>(null)
  const following = useRef(true)
  const pinned = useRef<number | null>(null)
  /** The scroll box's size before a page of older turns, to keep the reader's place after it. */
  const anchor = useRef<{ height: number; top: number } | null>(null)
  const seen = useRef(state)
  /** Bumped by each snapshot, so a page asked for before it is dropped. */
  const generation = useRef(0)
  const loadingOlder = useRef(false)
  const questionReads = useRef(0)
  /** Why the last read of the open questions failed; none once one succeeds. */
  const [questionsError, setQuestionsError] = useState<string | null>(null)
  /** The questions a read was asked for, so each new one is read once. */
  const asked = useRef(new Set<string>())

  // A failed read says so with a Retry, and the next new question reads again.
  const readQuestions = useCallback(() => {
    const read = ++questionReads.current
    listQuestions(host, sessionId).then(
      (listed) => {
        if (read !== questionReads.current) return
        setQuestions(listed)
        setQuestionsError(null)
      },
      (error) => {
        if (read !== questionReads.current) return
        asked.current.clear()
        setQuestionsError(describeError(error))
      },
    )
  }, [host, sessionId])

  useEffect(() => {
    setState(emptyTrajectory())
    setOpen({})
    setLoads({})
    setQuestions([])
    setQuestionsError(null)
    asked.current.clear()
    setHistory({ state: "idle" })
    generation.current += 1
    if (baseUrl === null) {
      setStreamStatus("reconnecting")
      setStreamError(unreachable)
      return
    }
    const stream = new TrajectoryStream(trajectoryStreamUrl(baseUrl, sessionId), {
      onSnapshot: (snapshot) => {
        generation.current += 1
        loadingOlder.current = false
        setHistory({ state: "idle" })
        setLoads({})
        setState((current) => applySnapshot(current, snapshot))
        readQuestions()
      },
      onUpdate: (update) => setState((current) => applyUpdate(current, update)),
      onStatus: (next, error) => {
        setStreamStatus(next)
        setStreamError(error ?? null)
      },
    })
    stream.start()
    return () => {
      questionReads.current += 1
      stream.stop()
    }
  }, [baseUrl, unreachable, sessionId, readQuestions])

  // A question the stream brings that the last read did not list is read again.
  const unlisted = state.turns.some((turn) =>
    turn.attention.some(
      (step) =>
        step.type === "question" &&
        step.state === "open" &&
        !questions.some((one) => one.id === step.request_id),
    ),
  )
  useEffect(() => {
    if (!unlisted) return
    const ids = state.turns.flatMap((turn) =>
      turn.attention.flatMap((step) =>
        step.type === "question" && step.state === "open" ? [step.request_id] : [],
      ),
    )
    const fresh = ids.filter((id) => !asked.current.has(id))
    if (fresh.length === 0) return
    for (const id of fresh) asked.current.add(id)
    readQuestions()
  }, [unlisted, state.turns, readQuestions])

  // biome-ignore lint/correctness/useExhaustiveDependencies: the focus request is read once, on arrival
  useEffect(() => {
    if (autoFocus) field.current?.focus()
  }, [])

  const live = isLiveStatus(status)
  const sessionState = state.session
  const running = live && (status === "running" || (sessionState?.running_turn_id ?? null) !== null)
  const runningTurn = state.turns.find((turn) => turn.state === "running")
  const startedAt = running ? runningTurn?.started_at : undefined

  // The clock of the running turn counts on, once a second.
  useEffect(() => {
    if (startedAt === undefined) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [startedAt])

  const permissions = live ? pendingPermissions(state) : []
  const pending = permissions.at(-1)
  const pendingId = pending?.id
  // A new permission starts on its first option, with nobody answering it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only a new permission resets the pick
  useEffect(() => {
    setSelected(0)
    setAnswering(null)
  }, [pendingId])

  const commands = (sessionState?.commands ?? []).map((command) => ({
    name: command.name,
    description: command.description,
    hint: command.hint ?? undefined,
  }))
  const suggestions = live && !pending && !dismissed ? suggest(commands, draft, cursor) : undefined
  const pickedSuggestion = Math.min(suggested, Math.max((suggestions?.length ?? 1) - 1, 0))

  // Follow the newest output, unless the reader scrolled away to read. A
  // refused send and an opened fold change the height too.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the refused sends and the folds are triggers, not read
  useLayoutEffect(() => {
    const el = log.current
    if (!el) return
    const kept = anchor.current
    anchor.current = null
    if (kept) el.scrollTop = kept.top + (el.scrollHeight - kept.height)
    else if (following.current) pin(el, pinned)
    else if (seen.current !== state) setUnread(true)
    seen.current = state
  }, [state, failed, open])

  // A panel that is resized, or a turn that grows after it is drawn, keeps a
  // following reader at the newest output.
  // biome-ignore lint/correctness/useExhaustiveDependencies: new turns are the trigger, so they are observed
  useEffect(() => {
    const el = log.current
    if (!el || typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver(() => {
      if (following.current) pin(el, pinned)
    })
    observer.observe(el)
    for (const inner of Array.from(el.children)) observer.observe(inner)
    return () => observer.disconnect()
  }, [state.turns.length])

  function jumpToLatest() {
    const el = log.current
    if (!el) return
    following.current = true
    pin(el, pinned)
    setAway(false)
    setUnread(false)
  }

  const loadOlder = useCallback(async () => {
    // The page starts at the history read whole, not at an older turn the
    // stream brought: the turns between them are still to read.
    const oldest = latest.current.cursor
    if (loadingOlder.current || !latest.current.hasOlder || oldest === undefined) return
    const mine = generation.current
    loadingOlder.current = true
    setHistory({ state: "loading" })
    try {
      const page = await unwrap(
        api(host).GET("/v1/sessions/{id}/trajectory", {
          params: { path: { id: sessionId }, query: { before: oldest } },
        }),
      )
      if (generation.current !== mine) return
      loadingOlder.current = false
      const el = log.current
      anchor.current = el ? { height: el.scrollHeight, top: el.scrollTop } : null
      setState((current) => prependTurns(current, page))
      setHistory(page.has_older ? { state: "idle" } : { state: "start" })
    } catch (error) {
      if (generation.current !== mine) return
      loadingOlder.current = false
      setHistory({ state: "error", message: describeError(error) })
    }
  }, [host, sessionId])

  // Turns that fit the box leave no scroll to reach the top with: while older
  // turns exist, they load until the box scrolls or the session starts. A
  // box with no height yet is not on screen, and waits.
  useLayoutEffect(() => {
    const el = log.current
    if (!el || history.state !== "idle" || !state.hasOlder || state.cursor === undefined) return
    if (el.clientHeight > 0 && el.scrollHeight <= el.clientHeight) void loadOlder()
  })

  /** Load a page of a turn's steps: its last ones, or with `before` the ones older than that step. */
  const loadSteps = useCallback(
    async (turnId: string, before?: string) => {
      const mine = generation.current
      setLoads((current) => ({ ...current, [turnId]: { state: "loading" } }))
      try {
        const page = await unwrap(
          api(host).GET("/v1/sessions/{id}/trajectory/turns/{turn_id}/steps", {
            params: {
              path: { id: sessionId, turn_id: turnId },
              query: before === undefined ? {} : { before },
            },
          }),
        )
        if (generation.current !== mine) return
        setState((current) => addStepPage(current, turnId, page, before))
        setLoads((current) => ({ ...current, [turnId]: IDLE }))
      } catch (error) {
        if (generation.current !== mine) return
        setLoads((current) => ({
          ...current,
          [turnId]: { state: "error", message: describeError(error) },
        }))
      }
    },
    [host, sessionId],
  )

  // An open fold whose page a snapshot dropped reads it again.
  useEffect(() => {
    for (const turn of state.turns) {
      if (open[turn.id] && !state.pages[turn.id] && (loads[turn.id] ?? IDLE).state === "idle")
        void loadSteps(turn.id)
    }
  }, [state, open, loads, loadSteps])

  const toggleTurn = useCallback((turnId: string) => {
    setOpen((current) => ({ ...current, [turnId]: !current[turnId] }))
  }, [])

  const earlierSteps = useCallback(
    (turnId: string) => {
      // The page before the first row shown, which may be a live step older
      // than the page loaded.
      const turn = latest.current.turns.find((one) => one.id === turnId)
      const page = latest.current.pages[turnId]
      const first = turn ? foldSteps(turn, page).steps[0] : page?.steps[0]
      void loadSteps(turnId, first?.id)
    },
    [loadSteps],
  )

  const post = useCallback(
    (text: string) =>
      unwrap(
        api(host).POST("/v1/sessions/{id}/console/input", {
          params: { path: { id: sessionId } },
          body: { text },
        }),
      ),
    [host, sessionId],
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

  /** Send one prompt: it shows queued until a turn holds it, and a refusal keeps it. */
  function dispatch(text: string) {
    sendCount += 1
    const id = `queued:${sendCount}`
    setState((current) => queuePrompt(current, text, id))
    // A failure can land after the pane is gone: it goes to the retained state.
    const owner = keyRef.current
    post(text).catch((error) => {
      setState((current) => dequeuePrompt(current, id, text))
      changeFailed(owner, (now) => [
        ...now,
        { id, text, message: `Could not send the prompt: ${describeError(error)}` },
      ])
    })
  }

  /** Change the refused sends of the console `owner`, shown or not. */
  function changeFailed(owner: string, change: (now: FailedSend[]) => FailedSend[]) {
    updateViewState(owner, { failed: change(readViewState(owner).failed) })
  }

  function send() {
    const text = draft
    if (!live || text.trim().length === 0) return
    edit("", 0)
    dispatch(text)
    field.current?.focus()
  }

  function retry(one: FailedSend) {
    changeFailed(keyRef.current, (now) => now.filter((other) => other.id !== one.id))
    dispatch(one.text)
  }

  /** Take a refused prompt back into the composer, in front of any newer draft. */
  function editFailed(one: FailedSend) {
    changeFailed(keyRef.current, (now) => now.filter((other) => other.id !== one.id))
    fill(draft.length === 0 ? one.text : `${one.text}\n${draft}`, one.text.length)
  }

  /** The text with the command's first word completed to `name`, the rest kept. */
  function complete(name: string) {
    const end = draft.search(/\s/)
    const rest = end === -1 ? "" : draft.slice(end)
    const head = `/${name}`
    if (rest === "") fill(`${head} `, head.length + 1)
    else fill(`${head}${rest}`, head.length + (rest.startsWith(" ") ? 1 : 0))
  }

  function insertCommand(name: string) {
    const head = `/${name} `
    fill(`${head}${draft}`, head.length)
  }

  const cancel = useCallback(() => {
    unwrap(
      api(host).POST("/v1/sessions/{id}/console/cancel", { params: { path: { id: sessionId } } }),
    ).catch((error) =>
      toast.error("Could not cancel the turn", { description: describeError(error) }),
    )
  }, [host, sessionId])

  function answer(option: PermissionOption) {
    setAnswering(option.id)
    post(option.id).catch((error) => {
      setAnswering(null)
      toast.error("Could not answer the permission", { description: describeError(error) })
    })
  }

  /** The permission's keys: Up, Down and 1 to 9 select, Enter answers. True when taken. */
  function pickKey(event: KeyboardEvent): boolean {
    if (!pending || answering !== null || pending.options.length === 0) return false
    const count = pending.options.length
    if (event.key === "ArrowUp") setSelected((index) => (index + count - 1) % count)
    else if (event.key === "ArrowDown") setSelected((index) => (index + 1) % count)
    else if (/^[1-9]$/.test(event.key) && Number(event.key) <= count)
      setSelected(Number(event.key) - 1)
    else if (event.key === "Enter") {
      const option = pending.options[selected]
      if (option) answer({ id: option.id, name: option.name })
    } else return false
    event.preventDefault()
    return true
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

  function onFieldKey(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key === "Enter" &&
      (event.shiftKey || event.nativeEvent.isComposing || event.keyCode === 229)
    )
      return
    // An empty composer hands the permission card its keys.
    if (draft.length === 0 && pickKey(event)) return
    if (suggestKey(event)) return
    if (event.key === "Enter") {
      event.preventDefault()
      send()
    }
  }

  // Escape is claimed on the console's own element, before the pane around it hears of it.
  const escapeKeys = useRef({ running, cancel, suggesting: false, dismiss: () => {} })
  escapeKeys.current = {
    running,
    cancel,
    suggesting: suggestions !== undefined,
    dismiss: () => setDismissed(true),
  }
  useEffect(() => {
    const el = root.current
    if (!el) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return
      event.preventDefault()
      event.stopPropagation()
      // ⌘Escape leaves the console for the pane's Close button, and closes nothing.
      if (event.metaKey) {
        el.closest<HTMLElement>('[data-slot="docked-pane"]')
          ?.querySelector<HTMLElement>('[data-slot="pane-close"]')
          ?.focus()
        return
      }
      if (escapeKeys.current.suggesting) escapeKeys.current.dismiss()
      else if (escapeKeys.current.running) escapeKeys.current.cancel()
    }
    el.addEventListener("keydown", onKey)
    return () => el.removeEventListener("keydown", onKey)
  }, [])

  const resume = useResumeAction(host, onResumed)
  const menu = (
    <ComposerMenu
      open={menuOpen}
      onOpenChange={setMenuOpen}
      commands={commands}
      onCommand={insertCommand}
      onSwitch={session && canSwitch ? () => setSwitchOpen(true) : undefined}
      onResume={live ? undefined : () => resume.resume(sessionId)}
      resumePending={resume.pending}
      copyLabel="Copy loaded transcript (main)"
      onCopy={() => {
        void copyText(turnsText(state)).then((copied) =>
          copied
            ? toast.success("Transcript copied", { description: "the loaded turns" })
            : toast.error("Could not copy the transcript"),
        )
      }}
      returnFocus={() => field.current}
    />
  )

  const nowIso = new Date(now).toISOString()
  const usageNow = sessionState?.usage
  const tokens = usageNow
    ? { input: usageNow.input_tokens, output: usageNow.output_tokens }
    : (usage ?? { input: 0, output: 0 })
  const waiting =
    live &&
    state.turns.some((turn) =>
      turn.attention.some((step) => step.type === "question" && step.state === "open"),
    )
  const runningTool = runningTurn?.steps.findLast(
    (step) => step.type === "tool_call" && (step.status === "running" || step.status === "pending"),
  )

  return (
    <div
      ref={root}
      data-console="turns"
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background text-sm text-foreground shadow-xs",
        className,
      )}
    >
      <div className="flex items-center gap-3 border-b px-3 py-1.5 font-mono text-xs text-muted-foreground">
        <span role="status" className="flex min-w-0 items-center gap-3">
          <StreamStatusLine status={streamStatus} error={streamError} />
        </span>
      </div>
      <TrajectoryFind turns={shownTurns(state)} />
      <HistoryLine history={history} onRetry={() => void loadOlder()} />
      <StepWorkspace
        host={host}
        sessionId={sessionId}
        trajectory={state}
        className="relative flex min-h-0 flex-1 flex-col"
      >
        <div
          ref={log}
          role="log"
          aria-label="Console transcript"
          className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-2"
          onScroll={(event) => {
            const el = event.currentTarget
            // Our own scroll to the newest output is not the reader leaving.
            if (el.scrollTop === pinned.current) pinned.current = null
            else {
              following.current =
                el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_SLACK_PX
              setAway(!following.current)
              if (following.current) setUnread(false)
            }
            if (el.scrollTop <= TOP_SLACK_PX) void loadOlder()
          }}
        >
          {shownTurns(state).map((turn) => (
            <TurnView
              key={turn.id}
              host={host}
              sessionId={sessionId}
              turn={turn}
              page={state.pages[turn.id]}
              load={loads[turn.id] ?? IDLE}
              open={open[turn.id] === true}
              live={live}
              now={turn.state === "running" ? nowIso : (turn.ended_at ?? turn.started_at)}
              questions={questions}
              onToggle={toggleTurn}
              onEarlier={earlierSteps}
            />
          ))}
          {state.queued.map((prompt) => (
            <div key={prompt.id} data-block="prompt">
              <PromptBox text={prompt.text} queued />
            </div>
          ))}
          {questionsError ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-md border border-status-danger/40 px-3 py-1 font-mono text-xs text-status-danger-fg"
            >
              Could not read the open questions: {questionsError}
              <Button size="xs" variant="outline" onClick={readQuestions}>
                Retry
              </Button>
            </div>
          ) : null}
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
      </StepWorkspace>
      <TrajectoryDock>
        {pending ? (
          <PermissionCard
            step={pending}
            selected={selected}
            answering={answering}
            onSelect={setSelected}
            onAnswer={answer}
            onKeyDown={pickKey}
          />
        ) : null}
      </TrajectoryDock>
      <Composer
        fieldRef={field}
        menu={menu}
        draft={draft}
        live={live}
        running={running}
        child={undefined}
        suggestions={suggestions}
        picked={pickedSuggestion}
        onChange={edit}
        onSelect={setCursor}
        onKeyDown={onFieldKey}
        onPick={(command) => complete(command.name)}
        onSend={send}
        onStop={cancel}
      />
      <TrajectoryStatus
        status={status}
        turn={
          running
            ? {
                tool:
                  runningTool?.type === "tool_call"
                    ? (runningTool.subject ?? runningTool.title)
                    : undefined,
                seconds: elapsedSeconds(startedAt, now),
              }
            : undefined
        }
        waiting={waiting}
        tokens={tokens}
      />
      {session ? (
        <SwitchSessionDialog
          host={host}
          session={session}
          open={switchOpen}
          onOpenChange={setSwitchOpen}
          onSwitched={onSwitched}
        />
      ) : null}
    </div>
  )
}

/** The loaded turns as text: each prompt and answer, oldest first. */
function turnsText(state: TrajectoryState): string {
  return shownTurns(state)
    .flatMap((turn) => {
      const lines: string[] = []
      if (turn.prompt) lines.push(`> ${contentText(turn.prompt.content)}`)
      const answer = contentText(turn.answer)
      if (answer) lines.push(answer)
      return lines
    })
    .join("\n\n")
}

/**
 * Scroll to the newest output, and remember where to: the scroll event that
 * follows is ours, not the reader's. A box with nothing to scroll sends none.
 */
function pin(el: HTMLElement, pinned: { current: number | null }) {
  el.scrollTop = el.scrollHeight
  pinned.current = el.scrollTop > 0 ? el.scrollTop : null
}

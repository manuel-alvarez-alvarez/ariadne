/**
 * What the ACP runtime recorded for the agent: `GET /v1/events?session={id}`,
 * tailed live.
 *
 * The endpoint is cursor-forward only — it returns the *oldest* events after a
 * given id, never the newest — so the feed is built by sweeping forward once
 * and then asking only for what is new. That also makes the live path cheap:
 * `agent_event` invalidates `agentEvents.lists()` in the dispatcher, this query
 * refetches, and the refetch is a single request for the handful of events
 * since the last one seen rather than the whole history again.
 *
 * The accumulated events therefore live in a ref rather than in the response:
 * each refetch appends to them, and the query's data is that running list.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronRightIcon, DotIcon } from "lucide-react"
import { useRef, useState } from "react"

import { type AgentEventDto, api, qk, unwrap } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"
import { cn } from "@/lib/format"

/** Page size; the daemon caps `limit` at 200. */
const PAGE = 200
/** Pages the initial sweep will walk before settling for the tail it has. */
const MAX_SWEEP_PAGES = 20
/** Events kept in the feed; older ones scroll out of usefulness anyway. */
const MAX_KEPT = 500

interface Tail {
  sessionId: string
  /** Id of the newest event taken so far; the `after` cursor. */
  cursor: string | undefined
  events: AgentEventDto[]
}

type SubagentState = "running" | "completed" | "failed" | "cancelled"

interface StoredSubagent {
  id: string
  title?: string | null
}

type StoredAgentEvent = AgentEventDto & { subagent?: StoredSubagent | null }

function subagentFrom(value: unknown): StoredSubagent | undefined {
  if (typeof value !== "object" || value === null) return undefined
  const { id, title } = value as Record<string, unknown>
  if (typeof id !== "string") return undefined
  return { id, title: typeof title === "string" ? title : undefined }
}

function subagentOf(event: StoredAgentEvent): StoredSubagent | undefined {
  const stored = subagentFrom(event.subagent)
  if (stored) return stored
  if (
    event.kind !== "subagent_update" ||
    typeof event.payload !== "object" ||
    event.payload === null
  ) {
    return undefined
  }
  return subagentFrom((event.payload as Record<string, unknown>).subagent)
}

function subagentStateOf(event: StoredAgentEvent): SubagentState | undefined {
  if (
    event.kind !== "subagent_update" ||
    typeof event.payload !== "object" ||
    event.payload === null
  ) {
    return undefined
  }
  const state = (event.payload as Record<string, unknown>).state
  return state === "running" || state === "completed" || state === "failed" || state === "cancelled"
    ? state
    : undefined
}

function subagentTitle(subagent: StoredSubagent): string {
  return subagent.title || subagent.id
}

function eventLabel(event: StoredAgentEvent, startsSubagent: boolean): string {
  if (event.kind === "subagent_update") {
    const subagent = subagentOf(event)
    if (subagent) {
      if (startsSubagent) return `Subagent started · ${subagentTitle(subagent)}`
      const state = subagentStateOf(event)
      return `Subagent ${state ?? "updated"} · ${subagentTitle(subagent)}`
    }
  }

  const labels: Record<string, string> = {
    pre_tool_use: "Tool call",
    post_tool_use: "Tool result",
    tool_call_update: "Tool update",
    permission_request: "Permission asked",
    "permission.replied": "Permission answered",
    agent_message: "Agent said",
    agent_request: "Waiting for you",
    agent_thought: "Agent thought",
    agent_message_chunk: "Agent message",
    agent_thought_chunk: "Thinking",
    user_message_chunk: "User message",
    user_prompt_submit: "User submitted",
    available_commands_update: "Commands updated",
    session_start: "Session started",
    stop: "Stopped",
    plan: "Plan",
  }
  return labels[event.kind] ?? event.kind
}

function eventColor(event: StoredAgentEvent): string {
  const subagentState = subagentStateOf(event)
  if (subagentState === "completed") return "var(--color-status-active)"
  if (subagentState === "failed") return "var(--color-status-danger)"

  const { kind } = event
  if (
    kind === "permission_request" ||
    kind === "permission.replied" ||
    kind === "agent_message_chunk" ||
    kind === "user_message_chunk"
  ) {
    return "var(--color-status-warn)"
  }
  if (kind === "stop" || kind.startsWith("error") || kind === "agent_thought") {
    return "var(--color-status-danger)"
  }
  if (kind === "agent_message" || kind === "plan") {
    return "var(--color-status-active)"
  }
  return "var(--color-status-pending)"
}

function extractToolName(payload: unknown): string | undefined {
  if (typeof payload !== "object" || payload === null) return undefined
  const record = payload as Record<string, unknown>
  return typeof record.tool_name === "string"
    ? record.tool_name
    : typeof record.command === "string"
      ? record.command
      : undefined
}

function extractFirstArg(payload: unknown): string | undefined {
  if (typeof payload !== "object" || payload === null) return undefined
  const record = payload as Record<string, unknown>

  if (typeof record.file_path === "string") return record.file_path
  if (typeof record.path === "string") return record.path
  if (typeof record.pattern === "string") return record.pattern

  const input = record.tool_input
  if (typeof input === "string") return input
  if (typeof input === "object" && input !== null) {
    const inputRec = input as Record<string, unknown>
    if (typeof inputRec.file_path === "string") return inputRec.file_path
    if (typeof inputRec.path === "string") return inputRec.path
    if (typeof inputRec.pattern === "string") return inputRec.pattern
    const firstVal = Object.values(inputRec)[0]
    if (typeof firstVal === "string") return firstVal
  }
  return undefined
}

function deriveSummary(event: AgentEventDto): string {
  if (event.summary && event.summary !== "…") {
    return event.summary
  }

  const toolName = extractToolName(event.payload)
  const firstArg = extractFirstArg(event.payload)

  if (toolName && firstArg) {
    return `${toolName} ${firstArg}`
  }
  if (toolName) {
    return toolName
  }
  return event.summary || "…"
}

interface FoldedRow {
  kind: string
  tool: string
  count: number
  events: StoredAgentEvent[]
}

function foldEvents(events: StoredAgentEvent[]): (StoredAgentEvent | FoldedRow)[] {
  const result: (StoredAgentEvent | FoldedRow)[] = []

  for (let i = 0; i < events.length; i++) {
    const event = events[i]
    if (!event) continue

    const toolName = extractToolName(event.payload) || ""
    const run: StoredAgentEvent[] = [event]

    let j = i + 1
    while (j < events.length) {
      const next = events[j]
      if (
        !next ||
        next.kind !== event.kind ||
        (extractToolName(next.payload) || "") !== toolName ||
        subagentOf(next)?.id !== subagentOf(event)?.id ||
        subagentStateOf(next) !== subagentStateOf(event)
      ) {
        break
      }
      run.push(next)
      j += 1
    }

    if (run.length > 1) {
      result.push({
        kind: event.kind,
        tool: toolName,
        count: run.length,
        events: run,
      })
    } else {
      result.push(event)
    }

    i = j - 1
  }

  return result
}

function isFoldedRow(row: StoredAgentEvent | FoldedRow): row is FoldedRow {
  return "count" in row
}

/** The `error_event_id` a `session.diagnosis` payload names, where it is a string. */
function errorEventIdOf(payload: unknown): string | undefined {
  if (typeof payload !== "object" || payload === null) return undefined
  const value = (payload as Record<string, unknown>).error_event_id
  return typeof value === "string" ? value : undefined
}

/**
 * Every `session.diagnosis` event's note, by the id of the `session.error`
 * it is about — read off `data` whole, so a late diagnosis still attaches
 * to its error whatever else has happened since (024): the session ending,
 * a resume, a later launch's own failure.
 */
function diagnosisNotes(events: AgentEventDto[]): Map<string, string> {
  const notes = new Map<string, string>()
  for (const event of events) {
    if (event.kind !== "session.diagnosis") continue
    const errorEventId = errorEventIdOf(event.payload)
    if (errorEventId) notes.set(errorEventId, event.summary)
  }
  return notes
}

export function SessionActivity({ sessionId }: { sessionId: string }) {
  // Survives refetches, reset when the screen moves to another session.
  const tail = useRef<Tail>({ sessionId, cursor: undefined, events: [] })

  const { data, isPending, isError, error, refetch } = useQuery({
    queryKey: qk.agentEvents.list({ session: sessionId }),
    queryFn: async () => {
      if (tail.current.sessionId !== sessionId) {
        tail.current = { sessionId, cursor: undefined, events: [] }
      }
      const fresh = await sweep(sessionId, tail.current.cursor)
      const newest = fresh.at(-1)
      if (newest) {
        tail.current = {
          sessionId,
          cursor: newest.id,
          events: [...tail.current.events, ...fresh].slice(-MAX_KEPT),
        }
      }
      return tail.current.events
    },
  })

  if (isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-6 w-full" />
        <Skeleton className="h-6 w-4/5" />
        <Skeleton className="h-6 w-2/3" />
      </div>
    )
  }

  if (isError) {
    return (
      <ErrorState
        title="Could not load agent events"
        error={error}
        onRetry={() => void refetch()}
      />
    )
  }

  if (data.length === 0) {
    return (
      <EmptyState
        emphasis="quiet"
        title="No agent events yet"
        description="The ACP runtime records them as the agent starts, uses tools and finishes turns."
        className="border-0"
      />
    )
  }

  // A diagnosis (024) carries no row of its own: it reads as a note on the
  // `session.error` it names, by id, so it never drifts apart from that
  // error — not into its own row, and not behind a later event.
  const events = data as StoredAgentEvent[]
  const notes = diagnosisNotes(events)
  const visible = events.filter((event) => event.kind !== "session.diagnosis")
  const seenSubagents = new Set<string>()
  const firstSubagentUpdates = new Set<string>()
  for (const event of visible) {
    if (event.kind !== "subagent_update") continue
    const subagent = subagentOf(event)
    if (subagent && !seenSubagents.has(subagent.id)) {
      seenSubagents.add(subagent.id)
      firstSubagentUpdates.add(event.id)
    }
  }
  const folded = foldEvents([...visible].reverse())

  return (
    <ol className="divide-y">
      {folded.map((row) =>
        isFoldedRow(row) ? (
          <FoldedActivityRow
            key={`folded-${row.events[0]?.id}`}
            row={row}
            firstSubagentUpdates={firstSubagentUpdates}
          />
        ) : (
          <ActivityRow
            key={row.id}
            event={row}
            note={notes.get(row.id)}
            startsSubagent={firstSubagentUpdates.has(row.id)}
          />
        ),
      )}
    </ol>
  )
}

function ActivityRow({
  event,
  note,
  startsSubagent,
}: {
  event: StoredAgentEvent
  note?: string
  startsSubagent: boolean
}) {
  const [open, setOpen] = useState(false)
  const summary = deriveSummary(event)
  const label = eventLabel(event, startsSubagent)
  const subagent = event.kind === "subagent_update" ? undefined : subagentFrom(event.subagent)

  return (
    <li className="py-1.5">
      <button
        type="button"
        className="flex w-full items-baseline gap-2 text-left text-sm"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 translate-y-0.5 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
        />
        <DotIcon className="size-2 shrink-0 fill-current" style={{ color: eventColor(event) }} />
        <span className="text-xs font-medium" style={{ color: eventColor(event) }}>
          {subagent ? `${subagentTitle(subagent)} · ${label}` : label}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {summary}
        </span>
        <When
          at={event.created_at}
          format="age"
          label="reported"
          className="shrink-0 text-xs text-muted-foreground tabular-nums"
        />
      </button>
      {note ? (
        <p className="ml-5 truncate font-mono text-xs text-muted-foreground">{note}</p>
      ) : null}
      {open ? (
        // Focusable and named, so the payload scrolls under the arrow keys and
        // announces what it is when focus lands in it.
        <section
          aria-label={`${event.kind} payload`}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className="mt-1 max-h-64 overflow-auto rounded-md bg-muted p-2 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <pre className="font-mono text-xs">{stringify(event.payload, 2)}</pre>
        </section>
      ) : null}
    </li>
  )
}

function FoldedActivityRow({
  row,
  firstSubagentUpdates,
}: {
  row: FoldedRow
  firstSubagentUpdates: Set<string>
}) {
  const [open, setOpen] = useState(false)
  const firstEvent = row.events[0]
  if (!firstEvent) return null
  const summary = deriveSummary(firstEvent)
  const label = eventLabel(firstEvent, firstSubagentUpdates.has(firstEvent.id))
  const subagent =
    firstEvent.kind === "subagent_update" ? undefined : subagentFrom(firstEvent.subagent)

  return (
    <li className="py-1.5">
      <button
        type="button"
        className="flex w-full items-baseline gap-2 text-left text-sm"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 translate-y-0.5 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
        />
        <DotIcon
          className="size-2 shrink-0 fill-current"
          style={{ color: eventColor(firstEvent) }}
        />
        <span className="text-xs font-medium" style={{ color: eventColor(firstEvent) }}>
          {subagent ? `${subagentTitle(subagent)} · ${label}` : label}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {summary} ×{row.count}
        </span>
      </button>
      {open ? (
        <ol className="mt-1 space-y-1 pl-4">
          {row.events.map((event) => (
            <li key={event.id}>
              <ExpandableEventPayload event={event} />
            </li>
          ))}
        </ol>
      ) : null}
    </li>
  )
}

function ExpandableEventPayload({ event }: { event: StoredAgentEvent }) {
  const [open, setOpen] = useState(false)
  const summary = deriveSummary(event)

  return (
    <div className="space-y-1">
      <button
        type="button"
        className="flex w-full items-baseline gap-2 text-left text-sm"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 translate-y-0.5 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
        />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {summary}
        </span>
        <When
          at={event.created_at}
          format="age"
          label="reported"
          className="shrink-0 text-xs text-muted-foreground tabular-nums"
        />
      </button>
      {open ? (
        // Focusable and named, so the payload scrolls under the arrow keys and
        // announces what it is when focus lands in it.
        <section
          aria-label={`${event.kind} payload`}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className="max-h-64 overflow-auto rounded-md bg-muted p-2 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <pre className="font-mono text-xs">{stringify(event.payload, 2)}</pre>
        </section>
      ) : null}
    </div>
  )
}

/** Walk the cursor forward until the daemon stops filling pages. */
async function sweep(sessionId: string, after: string | undefined): Promise<AgentEventDto[]> {
  const collected: AgentEventDto[] = []
  let cursor = after
  for (let page = 0; page < MAX_SWEEP_PAGES; page++) {
    const batch = await unwrap(
      api().GET("/v1/events", {
        params: { query: { session: sessionId, after: cursor, limit: PAGE } },
      }),
    )
    collected.push(...batch)
    if (batch.length < PAGE) break
    cursor = batch[batch.length - 1]?.id
    if (cursor === undefined) break
  }
  return collected.slice(-MAX_KEPT)
}

function stringify(payload: unknown, indent?: number): string {
  try {
    return JSON.stringify(payload, null, indent) ?? String(payload)
  } catch {
    return String(payload)
  }
}

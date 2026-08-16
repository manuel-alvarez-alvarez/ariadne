/**
 * What the agent's hooks reported: `GET /v1/events?session={id}`, tailed live.
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
import { ChevronRightIcon } from "lucide-react"
import { useRef, useState } from "react"

import { type AgentEventDto, api, qk, unwrap } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

import { formatAge, formatTimestamp } from "./session-display"
import { useNow } from "./use-now"

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

export function SessionActivity({ sessionId }: { sessionId: string }) {
  const now = useNow()
  // Survives refetches, reset when the screen moves to another session.
  const tail = useRef<Tail>({ sessionId, cursor: undefined, events: [] })

  const { data, isPending, isError, error } = useQuery({
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
    return <p className="text-sm text-destructive">{error.message}</p>
  }

  if (data.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No agent events yet. Hooks report them as the agent starts, uses tools and finishes turns.
      </p>
    )
  }

  return (
    <ol className="divide-y">
      {[...data].reverse().map((event) => (
        <ActivityRow key={event.id} event={event} now={now} />
      ))}
    </ol>
  )
}

function ActivityRow({ event, now }: { event: AgentEventDto; now: number }) {
  const [open, setOpen] = useState(false)
  const summary = summarize(event.payload)

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
        <Badge variant="secondary" className="shrink-0 font-mono">
          {event.kind}
        </Badge>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {summary}
        </span>
        <time
          className="shrink-0 text-xs text-muted-foreground tabular-nums"
          dateTime={event.created_at}
          title={formatTimestamp(event.created_at)}
        >
          {formatAge(event.created_at, now)}
        </time>
      </button>
      {open ? (
        <pre className="mt-1 max-h-64 overflow-auto rounded-md bg-muted p-2 font-mono text-xs">
          {stringify(event.payload, 2)}
        </pre>
      ) : null}
    </li>
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

/**
 * One line of whatever the hook sent. Payload shapes differ per agent and per
 * event kind, so the useful fields are picked when present and the rest is
 * shown as compact JSON — the full payload is one click away either way.
 */
function summarize(payload: unknown): string {
  if (payload === null || payload === undefined) return ""
  if (typeof payload !== "object") return String(payload)
  const record = payload as Record<string, unknown>
  const parts: string[] = []
  for (const key of ["tool_name", "message", "reason", "type", "cwd"]) {
    const value = record[key]
    if (typeof value === "string" && value.length > 0) parts.push(`${key}=${value}`)
  }
  if (parts.length > 0) return parts.join(" · ")
  const json = stringify(payload)
  return json.length > 160 ? `${json.slice(0, 160)}…` : json
}

function stringify(payload: unknown, indent?: number): string {
  try {
    return JSON.stringify(payload, null, indent) ?? String(payload)
  } catch {
    return String(payload)
  }
}

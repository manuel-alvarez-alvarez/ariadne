/**
 * The sessions screen's own filters over the merged listing's outside half —
 * everything `GET /v1/outside-sessions` takes — plus `kind`, which the daemon
 * has no filter for at all: the two listings are two separate requests (see
 * `queries.ts`), and `kind` decides client-side which of them the table
 * includes.
 *
 * The five daemon ones travel under its own name for each — `?agent=`,
 * `?dir=`, `?since=`, `?until=`, `?q=` — so the URL says both what the screen
 * shows and what it asked for. They live there for the reason the sessions
 * screen's status and seat do: a reload is routine in a hash-router desktop
 * app, and a narrowed screen is worth linking to. Unlike those two they are
 * not remembered between visits, because this half of the bar is for finding
 * one conversation rather than for a monitoring view left standing open.
 *
 * The date fields are the one place the URL and the request differ. A date
 * field names a day and the daemon takes a moment (RFC 3339), so a day is sent
 * as the instants that bound it — `since` as its first, `until` as its last, in
 * UTC — and a day in both fields is the whole of that day. A value that already
 * names a time is sent as it stands.
 *
 * `?window=` is separate from the five: it is not a daemon parameter of its
 * own but how far back the table looks by default — the last 7 days (the
 * daemon's own default, so unset asks for nothing extra), the last 30, or
 * all of it — and it only fills `since` or `all` where the since/until
 * fields above are themselves unset, an explicit day always being the more
 * specific ask. It is not remembered between visits: the table opens on 7
 * days every time.
 */

import { useSearchParams } from "react-router-dom"

import { ALL } from "./filters"
import type { OutsideSessionListFilters } from "./queries"

/** What kind of session the merged table shows, client-side only. */
type SessionKind = "ariadne" | "outside"

const KINDS: SessionKind[] = ["ariadne", "outside"]

export const KIND_LABELS: Record<SessionKind, string> = {
  ariadne: "Ariadne",
  outside: "Outside",
}

/** The params the filters travel in, in the order the bar shows them. */
export const OUTSIDE_FILTER_PARAMS = ["kind", "agent", "dir", "since", "until", "q"] as const

/** The params this half of the bar reads, and the only ones `filterBy` writes. */
export type OutsideFilterParam = (typeof OUTSIDE_FILTER_PARAMS)[number]

/** The param the window picker travels in, apart from the filters above. */
export const WINDOW_PARAM = "window"

/** How far back the table looks by default: 7 days, 30, or all of it. */
export type Window = "7" | "30" | "all"

const WINDOWS: Window[] = ["7", "30", "all"]

/** What the window picker opens on: the daemon's own default window. */
export const DEFAULT_WINDOW: Window = "7"

interface OutsideFiltersState {
  /** What each field shows: the param exactly as the URL carries it. */
  values: Record<OutsideFilterParam, string>
  /** Which kind the table is narrowed to, or `null` for both. */
  kind: SessionKind | null
  /** How far back the table looks, {@link DEFAULT_WINDOW} where unset. */
  window: Window
  /** What the daemon's outside-sessions endpoint is asked for. */
  filters: OutsideSessionListFilters
  /** Apply one selection. "All" and an empty field drop the param. */
  filterBy: (param: OutsideFilterParam, value: string) => void
  /** Apply several at once, the way {@link filterBy} applies one. */
  filterByMany: (values: Partial<Record<OutsideFilterParam, string>>) => void
  /** Pick the window: {@link DEFAULT_WINDOW} drops the param. */
  filterWindow: (window: string) => void
}

function parseKindFilter(value: string): SessionKind | null {
  return KINDS.find((known) => known === value) ?? null
}

function parseWindow(value: string | null): Window {
  return WINDOWS.find((known) => known === value) ?? DEFAULT_WINDOW
}

export function useOutsideSessionFilters(): OutsideFiltersState {
  const [search, setSearch] = useSearchParams()

  const values = Object.fromEntries(
    OUTSIDE_FILTER_PARAMS.map((param) => [param, search.get(param)?.trim() ?? ""]),
  ) as Record<OutsideFilterParam, string>
  const window = parseWindow(search.get(WINDOW_PARAM))

  const since = dayBound(values.since, "first")
  const until = dayBound(values.until, "last")
  const filters: OutsideSessionListFilters = {
    agent: values.agent || undefined,
    dir: values.dir || undefined,
    since: since || (window === "30" ? dayBound(dayBefore(29), "first") : undefined) || undefined,
    until: until || undefined,
    q: values.q || undefined,
    all: !since && !until && window === "all" ? true : undefined,
  }

  function filterByMany(changes: Partial<Record<OutsideFilterParam, string>>) {
    const next = new URLSearchParams(search)
    for (const [param, value] of Object.entries(changes)) {
      if (value === undefined || value === "" || value === ALL) next.delete(param)
      else next.set(param, value)
    }
    // A filter is not a place: Back leaves the screen rather than walking back
    // through every narrowing, as the status and seat filters do.
    setSearch(next, { replace: true })
  }

  function filterBy(param: OutsideFilterParam, value: string) {
    filterByMany({ [param]: value })
  }

  function filterWindow(value: string) {
    const next = new URLSearchParams(search)
    if (value === DEFAULT_WINDOW) next.delete(WINDOW_PARAM)
    else next.set(WINDOW_PARAM, value)
    setSearch(next, { replace: true })
  }

  return {
    values,
    kind: parseKindFilter(values.kind),
    window,
    filters,
    filterBy,
    filterByMany,
    filterWindow,
  }
}

/** A day as the date field carries it: `YYYY-MM-DD`, `days` before today. */
export function dayBefore(days: number): string {
  const day = new Date()
  day.setDate(day.getDate() - days)
  const pad = (value: number) => String(value).padStart(2, "0")
  return `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`
}

/**
 * `2026-09-10` as `2026-09-10T00:00:00Z` or `2026-09-10T23:59:59.999999999Z`,
 * UTC.
 *
 * The last bound is the finest moment RFC 3339 carries, because the daemon
 * keeps a session whose activity is at or before it: any coarser bound drops a
 * session last active after it, on the very day that was asked for. How fine a
 * timestamp is, is the agent's to choose and not the daemon's — a stored
 * session carries the `updatedAt` string its agent reported, unparsed — so the
 * bound is as fine as the parse behind the filter reads, which is nanoseconds.
 */
function dayBound(value: string, edge: "first" | "last"): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return value
  return edge === "first" ? `${value}T00:00:00Z` : `${value}T23:59:59.999999999Z`
}

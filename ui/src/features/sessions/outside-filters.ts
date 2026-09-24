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
const OUTSIDE_FILTER_PARAMS = ["kind", "agent", "dir", "since", "until", "q"] as const

/** The params this half of the bar reads, and the only ones `filterBy` writes. */
export type OutsideFilterParam = (typeof OUTSIDE_FILTER_PARAMS)[number]

interface OutsideFiltersState {
  /** What each field shows: the param exactly as the URL carries it. */
  values: Record<OutsideFilterParam, string>
  /** Which kind the table is narrowed to, or `null` for both. */
  kind: SessionKind | null
  /** What the daemon's outside-sessions endpoint is asked for. */
  filters: OutsideSessionListFilters
  /** Apply one selection. "All" and an empty field drop the param. */
  filterBy: (param: OutsideFilterParam, value: string) => void
}

function parseKindFilter(value: string): SessionKind | null {
  return KINDS.find((known) => known === value) ?? null
}

export function useOutsideSessionFilters(): OutsideFiltersState {
  const [search, setSearch] = useSearchParams()

  const values = Object.fromEntries(
    OUTSIDE_FILTER_PARAMS.map((param) => [param, search.get(param)?.trim() ?? ""]),
  ) as Record<OutsideFilterParam, string>

  const filters: OutsideSessionListFilters = {
    agent: values.agent || undefined,
    dir: values.dir || undefined,
    since: dayBound(values.since, "first") || undefined,
    until: dayBound(values.until, "last") || undefined,
    q: values.q || undefined,
  }

  function filterBy(param: OutsideFilterParam, value: string) {
    const next = new URLSearchParams(search)
    if (value === "" || value === ALL) next.delete(param)
    else next.set(param, value)
    // A filter is not a place: Back leaves the screen rather than walking back
    // through every narrowing, as the status and seat filters do.
    setSearch(next, { replace: true })
  }

  return { values, kind: parseKindFilter(values.kind), filters, filterBy }
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

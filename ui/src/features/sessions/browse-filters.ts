/**
 * The sessions screen's own filters over its rows: the agent, the directory,
 * the activity window and the typed search. `GET /v1/sessions` takes a
 * directory, a window and a title search of its own, but this screen applies
 * all five client-side over the one list it already holds (see
 * `sessions-page.tsx`), so a filter is no request of its own.
 *
 * They live in the URL — `?agent=`, `?dir=`, `?since=`, `?until=`, `?q=` — for
 * the reason the status and the seat do: a reload is routine in a hash-router
 * desktop app, and a narrowed screen is worth linking to. Unlike those two
 * they are not remembered between visits, because this half of the bar is for
 * finding one session rather than for a monitoring view left standing open.
 *
 * The date fields name a day, and a day is compared as the instants that bound
 * it — `since` as its first, `until` as its last, in UTC — so a day in both
 * fields is the whole of that day. A value that already names a time is used
 * as it stands.
 */

import { useSearchParams } from "react-router-dom"

import { ALL } from "./filters"

/** The params the filters travel in, in the order the bar shows them. */
export const BROWSE_FILTER_PARAMS = ["agent", "dir", "since", "until", "q"] as const

/** The params this half of the bar reads, and the only ones `filterBy` writes. */
export type BrowseFilterParam = (typeof BROWSE_FILTER_PARAMS)[number]

/** What the rows are narrowed by, the dates already as instants. */
export interface BrowseFilters {
  agent?: string
  dir?: string
  since?: string
  until?: string
  q?: string
}

interface BrowseFiltersState {
  /** What each field shows: the param exactly as the URL carries it. */
  values: Record<BrowseFilterParam, string>
  /** What the rows are narrowed by. */
  filters: BrowseFilters
  /** Apply one selection. "All" and an empty field drop the param. */
  filterBy: (param: BrowseFilterParam, value: string) => void
  /** Apply several at once, the way {@link filterBy} applies one. */
  filterByMany: (values: Partial<Record<BrowseFilterParam, string>>) => void
}

export function useBrowseFilters(): BrowseFiltersState {
  const [search, setSearch] = useSearchParams()

  const values = Object.fromEntries(
    BROWSE_FILTER_PARAMS.map((param) => [param, search.get(param)?.trim() ?? ""]),
  ) as Record<BrowseFilterParam, string>

  const filters: BrowseFilters = {
    agent: values.agent || undefined,
    dir: values.dir || undefined,
    since: dayBound(values.since, "first") || undefined,
    until: dayBound(values.until, "last") || undefined,
    q: values.q || undefined,
  }

  function filterByMany(changes: Partial<Record<BrowseFilterParam, string>>) {
    const next = new URLSearchParams(search)
    for (const [param, value] of Object.entries(changes)) {
      if (value === undefined || value === "" || value === ALL) next.delete(param)
      else next.set(param, param === "dir" || param === "q" ? value.trim() : value)
    }
    // A filter is not a place: Back leaves the screen rather than walking back
    // through every narrowing, as the status and seat filters do.
    setSearch(next, { replace: true })
  }

  function filterBy(param: BrowseFilterParam, value: string) {
    filterByMany({ [param]: value })
  }

  return { values, filters, filterBy, filterByMany }
}

/** A day as the date field carries it: `YYYY-MM-DD`, `days` before today. */
export function dayBefore(days: number): string {
  const day = new Date()
  day.setDate(day.getDate() - days)
  const pad = (value: number) => String(value).padStart(2, "0")
  return `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`
}

/** `2026-09-10` as `2026-09-10T00:00:00Z` or `2026-09-10T23:59:59.999999999Z`, UTC. */
function dayBound(value: string, edge: "first" | "last"): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return value
  return edge === "first" ? `${value}T00:00:00Z` : `${value}T23:59:59.999999999Z`
}

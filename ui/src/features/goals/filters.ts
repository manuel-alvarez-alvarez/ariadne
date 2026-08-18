/**
 * The goals board's status filter, as a URL search param.
 *
 * Every other piece of board state — which goal's panel is open, which task's,
 * which tab inside it — already lives in the URL, and a reload is routine in a
 * hash-router desktop app. A filter kept in component state is the one thing
 * that silently resets there, so it is kept here instead.
 *
 * Several statuses can be selected at once, and they travel as one
 * comma-separated `?status=active,completed` — the encoding `GET /v1/goals`
 * reads, so the URL the user sees and the request the board makes spell the
 * selection the same way.
 *
 * The URL only lasts as long as the user is on the board, and the sidebar
 * links to bare `/goals`. So the selection is also mirrored into the persisted
 * settings, and put back on a route entry that carries no `?status=` of its
 * own — see `restoreStatusFilter` and `use-status-filter.ts`.
 */

import type { GoalStatus } from "@/api"
import { GOAL_STATUSES } from "./status"

/**
 * The statuses the board is narrowed to, in `GOAL_STATUSES` order.
 *
 * Empty is no filter — the board shows every goal. That is also what selecting
 * every status means, so the two collapse onto one state: see
 * `normalizeStatusFilter`.
 */
export type StatusFilter = readonly GoalStatus[]

/** No filter: every goal, whatever its status. */
export const NO_STATUS_FILTER: StatusFilter = []

/** The param the filter round-trips through, alongside `?goal=` and `?task=`. */
export const STATUS_PARAM = "status"

/**
 * A selection in canonical form: `GOAL_STATUSES` order, no duplicates, nothing
 * the daemon does not define, and "all of them" folded back to no filter.
 *
 * Everything that leaves this module goes through it, so equal selections
 * produce equal URLs — and, through `goalListKey`, equal query keys.
 */
export function normalizeStatusFilter(statuses: readonly string[]): StatusFilter {
  const asked = new Set(statuses)
  const selected = GOAL_STATUSES.filter((status) => asked.has(status))
  return selected.length === GOAL_STATUSES.length ? NO_STATUS_FILTER : selected
}

/**
 * A selection out of the comma-separated form the param travels in, which is
 * also the form it is remembered in between visits to the board.
 */
export function parseStatusFilter(value: string): StatusFilter {
  if (!value) return NO_STATUS_FILTER
  return normalizeStatusFilter(value.split(",").map((status) => status.trim()))
}

/** The canonical param value for a selection; empty is no filter. */
export function serializeStatusFilter(filter: StatusFilter): string {
  return normalizeStatusFilter(filter).join(",")
}

/** What the URL asks for; anything the daemon does not define is ignored. */
export function readStatusFilter(params: URLSearchParams): StatusFilter {
  return parseStatusFilter(params.get(STATUS_PARAM) ?? "")
}

/**
 * The same params with the filter applied, every other one kept — an open
 * panel survives a change of filter.
 *
 * "All statuses" drops the param rather than spelling every status out: the
 * unfiltered board is `/goals`, not `/goals?status=planning,active,...`.
 */
export function withStatusFilter(params: URLSearchParams, filter: StatusFilter): URLSearchParams {
  const next = new URLSearchParams(params)
  const value = serializeStatusFilter(filter)
  if (value === "") next.delete(STATUS_PARAM)
  else next.set(STATUS_PARAM, value)
  return next
}

/**
 * The selection with one status checked or unchecked.
 *
 * Unchecking the last one lands on no filter rather than an empty board: a
 * filter that can match nothing is never what the click meant.
 */
export function toggleStatusFilter(filter: StatusFilter, status: GoalStatus): StatusFilter {
  const next = filter.includes(status)
    ? filter.filter((selected) => selected !== status)
    : [...filter, status]
  return normalizeStatusFilter(next)
}

/**
 * The params the goals route should be rewritten to when it is entered, or
 * `null` when the URL it was entered with already says everything.
 *
 * The URL stays the source of truth: a `?status=` on it — a deep link, a Back
 * step, a filter the user just set — is the answer, and `remembered` is only
 * consulted when the route was entered with nothing to go on, which is what a
 * sidebar link to bare `/goals` is. Whatever else the URL carries (an open
 * `?goal=` panel) is kept, so restoring a filter never closes a panel.
 *
 * A remembered "all statuses" restores to nothing rather than to a param: a
 * filter the user cleared stays cleared, here as much as on the board.
 */
export function restoreStatusFilter(
  params: URLSearchParams,
  remembered: string,
): URLSearchParams | null {
  if (params.has(STATUS_PARAM)) return null
  const filter = parseStatusFilter(remembered)
  if (filter.length === 0) return null
  return withStatusFilter(params, filter)
}

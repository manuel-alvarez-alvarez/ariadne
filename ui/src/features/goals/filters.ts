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

/** What the URL asks for; anything the daemon does not define is ignored. */
export function readStatusFilter(params: URLSearchParams): StatusFilter {
  const value = params.get(STATUS_PARAM)
  if (!value) return NO_STATUS_FILTER
  return normalizeStatusFilter(value.split(",").map((status) => status.trim()))
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
  const selected = normalizeStatusFilter(filter)
  if (selected.length === 0) next.delete(STATUS_PARAM)
  else next.set(STATUS_PARAM, selected.join(","))
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

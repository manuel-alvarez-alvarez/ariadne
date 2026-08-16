/**
 * The goals board's status filter, as a URL search param.
 *
 * Every other piece of board state — which goal's panel is open, which task's,
 * which tab inside it — already lives in the URL, and a reload is routine in a
 * hash-router desktop app. A filter kept in component state is the one thing
 * that silently resets there, so it is kept here instead.
 */

import type { GoalStatus } from "@/api"
import { GOAL_STATUSES } from "./status"

/** Sentinel for "no status filter" — `Select` needs a value for every item. */
export const ALL = "all"

export type StatusFilter = GoalStatus | typeof ALL

/** The param the filter round-trips through, alongside `?goal=` and `?task=`. */
export const STATUS_PARAM = "status"

/** What the URL asks for; anything the daemon does not define reads as no filter. */
export function readStatusFilter(params: URLSearchParams): StatusFilter {
  const value = params.get(STATUS_PARAM)
  return GOAL_STATUSES.includes(value as GoalStatus) ? (value as GoalStatus) : ALL
}

/**
 * The same params with the filter applied, every other one kept — an open
 * panel survives a change of filter.
 *
 * "All statuses" drops the param rather than spelling the sentinel out: the
 * unfiltered board is `/goals`, not `/goals?status=all`.
 */
export function withStatusFilter(params: URLSearchParams, filter: StatusFilter): URLSearchParams {
  const next = new URLSearchParams(params)
  if (filter === ALL) next.delete(STATUS_PARAM)
  else next.set(STATUS_PARAM, filter)
  return next
}

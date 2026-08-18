/**
 * The goals board's status filter: what it is showing, and how to change it.
 *
 * The URL is the source of truth while the user is on the board — `?status=`
 * is what the query reads, what a link can be shared as, and what Back walks.
 * But the sidebar entry is a plain `/goals`, so leaving the board and coming
 * back would otherwise drop the filter on the floor. The selection is
 * therefore mirrored into the persisted settings as it changes, and a route
 * entry with no `?status=` on it is rewritten to the remembered one.
 *
 * The rewrite replaces rather than pushes, for the same reason changing the
 * filter does: a filter is not a place, and Back from the board should leave
 * the board rather than step through the URL it corrected itself from.
 */

import { useEffect } from "react"
import { useSearchParams } from "react-router-dom"

import { useSettingsStore } from "@/stores/settings"
import {
  readStatusFilter,
  restoreStatusFilter,
  type StatusFilter,
  serializeStatusFilter,
  withStatusFilter,
} from "./filters"

export interface StatusFilterState {
  /** The statuses the board is narrowed to; empty is every goal. */
  statuses: StatusFilter
  /** Apply a selection: to the URL, and to what the next visit will restore. */
  filterBy: (next: StatusFilter) => void
}

export function useStatusFilter(): StatusFilterState {
  const [search, setSearch] = useSearchParams()
  const remembered = useSettingsStore((state) => state.goalStatusFilter)
  const remember = useSettingsStore((state) => state.setGoalStatusFilter)

  // Read the restored params rather than waiting for the effect below to put
  // them in the URL: the board asks the daemon for the right statuses on its
  // first render, instead of loading the unfiltered list and replacing it.
  const restored = restoreStatusFilter(search, remembered)
  const statuses = readStatusFilter(restored ?? search)
  const restoreTo = restored?.toString() ?? null
  const selected = serializeStatusFilter(statuses)

  useEffect(() => {
    if (restoreTo !== null) setSearch(restoreTo, { replace: true })
  }, [restoreTo, setSearch])

  // Covers the selections that arrive through the URL rather than through
  // `filterBy`: a deep link, a Back step. What the board is showing is what it
  // will show next time, however it got there.
  useEffect(() => {
    remember(selected)
  }, [selected, remember])

  function filterBy(next: StatusFilter) {
    // Remembered first, and synchronously: the render this navigation causes
    // asks `restoreStatusFilter` what to do, and a cleared filter would be put
    // straight back if the store still held the old one.
    remember(serializeStatusFilter(next))
    setSearch(withStatusFilter(search, next), { replace: true })
  }

  return { statuses, filterBy }
}

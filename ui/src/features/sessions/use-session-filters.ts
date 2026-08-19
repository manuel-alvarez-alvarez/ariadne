/**
 * The sessions screen's filters: what it is showing, and how to change it.
 *
 * The URL is the source of truth while the user is on the screen — `?status=`
 * and `?role=` are what the list reads, what a link can be shared as, and what
 * Back walks. But the sidebar entry is a plain `/sessions`, so leaving the
 * screen and coming back would otherwise drop both selections on the floor.
 * They are therefore mirrored into the persisted settings as they change, and
 * a route entry carrying neither is rewritten to the remembered ones.
 *
 * The rewrite replaces rather than pushes, for the same reason changing a
 * filter does: a filter is not a place, and Back from the screen should leave
 * it rather than step through the URL it corrected itself from.
 *
 * This is the goals board's `use-status-filter.ts`, for two single-select
 * params instead of one multi-select one.
 */

import { useEffect } from "react"
import { useSearchParams } from "react-router-dom"

import type { Role } from "@/api"
import { useSettingsStore } from "@/stores/settings"

import {
  type FilterParam,
  readRoleFilter,
  readStatusFilter,
  restoreSessionFilters,
  STATUS_PARAM,
  type StatusValue,
  withFilter,
} from "./filters"

export interface SessionFiltersState {
  /** The status the screen is narrowed to, or `null` for every status. */
  status: StatusValue | null
  /** The role the screen is narrowed to, or `null` for every role. */
  role: Role | null
  /** Apply a selection: to the URL, and to what the next visit will restore. */
  filterBy: (param: FilterParam, value: string) => void
}

export function useSessionFilters(): SessionFiltersState {
  const [search, setSearch] = useSearchParams()
  const rememberedStatus = useSettingsStore((state) => state.sessionStatusFilter)
  const rememberedRole = useSettingsStore((state) => state.sessionRoleFilter)
  const rememberStatus = useSettingsStore((state) => state.setSessionStatusFilter)
  const rememberRole = useSettingsStore((state) => state.setSessionRoleFilter)

  // Read the restored params rather than waiting for the effect below to put
  // them in the URL: the list asks the daemon for the right status on its
  // first render, instead of loading the unfiltered one and replacing it.
  const restored = restoreSessionFilters(search, {
    status: rememberedStatus,
    role: rememberedRole,
  })
  const status = readStatusFilter(restored ?? search)
  const role = readRoleFilter(restored ?? search)
  const restoreTo = restored?.toString() ?? null

  useEffect(() => {
    if (restoreTo !== null) setSearch(restoreTo, { replace: true })
  }, [restoreTo, setSearch])

  // Covers the selections that arrive through the URL rather than through
  // `filterBy`: a deep link, a Back step. What the screen is showing is what
  // it will show next time, however it got there — and a param the daemon no
  // longer defines is remembered as the no filter it already reads as.
  useEffect(() => {
    rememberStatus(status ?? "")
  }, [status, rememberStatus])

  useEffect(() => {
    rememberRole(role ?? "")
  }, [role, rememberRole])

  function filterBy(param: FilterParam, value: string) {
    const next = withFilter(search, param, value)
    // Remembered first, and synchronously: the render this navigation causes
    // asks `restoreSessionFilters` what to do, and a cleared filter would be
    // put straight back if the store still held the old one.
    const remember = param === STATUS_PARAM ? rememberStatus : rememberRole
    remember(next.get(param) ?? "")
    setSearch(next, { replace: true })
  }

  return { status, role, filterBy }
}

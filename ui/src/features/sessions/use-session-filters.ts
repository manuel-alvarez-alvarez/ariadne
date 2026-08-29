/**
 * The sessions screen's filters: what it is showing, and how to change it.
 *
 * The URL is the source of truth while the user is on the screen — `?status=`,
 * `?role=`, `?goal=` and `?task=` are what the list reads, what a link can be
 * shared as, and what Back walks. But the sidebar entry is a plain `/sessions`,
 * so leaving the screen and coming back would otherwise drop every selection on
 * the floor. They are therefore mirrored into the persisted settings as they
 * change, and a route entry carrying none of them is rewritten to the
 * remembered ones.
 *
 * The rewrite replaces rather than pushes, for the same reason changing a
 * filter does: a filter is not a place, and Back from the screen should leave
 * it rather than step through the URL it corrected itself from.
 *
 * This is the goals board's `use-status-filter.ts`, for four single-select
 * params instead of one multi-select one. Two of them — `?goal=` and `?task=`
 * — are the ones the daemon's own list endpoint takes, and are shown as chips
 * rather than dropdowns: there is no list of every goal worth putting in a
 * menu, and a scope usually arrives as a link from the work itself.
 */

import { useEffect } from "react"
import { useSearchParams } from "react-router-dom"

import type { Role } from "@/api"
import { useSettingsStore } from "@/stores/settings"

import {
  type FilterParam,
  GOAL_PARAM,
  ROLE_PARAM,
  readRoleFilter,
  readScopeFilter,
  readStatusFilter,
  restoreSessionFilters,
  STATUS_PARAM,
  type StatusValue,
  TASK_PARAM,
  withFilter,
} from "./filters"

interface SessionFiltersState {
  /** The status the screen is narrowed to, or `null` for every status. */
  status: StatusValue | null
  /** The role the screen is narrowed to, or `null` for every role. */
  role: Role | null
  /** The goal the screen is narrowed to, or `null` for every goal. */
  goal: string | null
  /** The task the screen is narrowed to, or `null` for every task. */
  task: string | null
  /** Apply a selection: to the URL, and to what the next visit will restore. */
  filterBy: (param: FilterParam, value: string) => void
}

export function useSessionFilters(): SessionFiltersState {
  const [search, setSearch] = useSearchParams()
  const rememberedStatus = useSettingsStore((state) => state.sessionStatusFilter)
  const rememberedRole = useSettingsStore((state) => state.sessionRoleFilter)
  const rememberedGoal = useSettingsStore((state) => state.sessionGoalFilter)
  const rememberedTask = useSettingsStore((state) => state.sessionTaskFilter)
  const rememberStatus = useSettingsStore((state) => state.setSessionStatusFilter)
  const rememberRole = useSettingsStore((state) => state.setSessionRoleFilter)
  const rememberGoal = useSettingsStore((state) => state.setSessionGoalFilter)
  const rememberTask = useSettingsStore((state) => state.setSessionTaskFilter)

  // Read the restored params rather than waiting for the effect below to put
  // them in the URL: the list asks the daemon for the right status on its
  // first render, instead of loading the unfiltered one and replacing it.
  const restored = restoreSessionFilters(search, {
    status: rememberedStatus,
    role: rememberedRole,
    goal: rememberedGoal,
    task: rememberedTask,
  })
  const params = restored ?? search
  const status = readStatusFilter(params)
  const role = readRoleFilter(params)
  const goal = readScopeFilter(params, GOAL_PARAM)
  const task = readScopeFilter(params, TASK_PARAM)
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

  useEffect(() => {
    rememberGoal(goal ?? "")
  }, [goal, rememberGoal])

  useEffect(() => {
    rememberTask(task ?? "")
  }, [task, rememberTask])

  function filterBy(param: FilterParam, value: string) {
    const next = withFilter(search, param, value)
    // Remembered first, and synchronously: the render this navigation causes
    // asks `restoreSessionFilters` what to do, and a cleared filter would be
    // put straight back if the store still held the old one.
    const remember = {
      [STATUS_PARAM]: rememberStatus,
      [ROLE_PARAM]: rememberRole,
      [GOAL_PARAM]: rememberGoal,
      [TASK_PARAM]: rememberTask,
    }[param]
    remember(next.get(param) ?? "")
    setSearch(next, { replace: true })
  }

  return { status, role, goal, task, filterBy }
}

/**
 * The sessions screen's two filters, as URL search params.
 *
 * They live in the URL for the reason the goals board's does: every other bit
 * of screen state is already there, a reload is routine in a hash-router
 * desktop app, and a narrowed screen is worth linking to. Each is one value —
 * `?status=failed`, `?role=engineer` — and an absent param is no filter.
 *
 * The URL only lasts as long as the user is on the screen, and the sidebar
 * links to a bare `/sessions`. So both selections are mirrored into the
 * persisted settings and put back on an entry that carries neither of its own
 * — see `restoreSessionFilters` and `use-session-filters.ts`, which follow the
 * goals board's `filters.ts` / `use-status-filter.ts` pair.
 */

import type { Role, SessionStatus } from "@/api"
import { ROLE_LABELS } from "@/lib/format"

import type { SessionListFilters } from "./queries"
import { SESSION_STATUS_META } from "./session-display"

/** The params the two filters travel in, alongside `?session=`. */
export const STATUS_PARAM = "status"
export const ROLE_PARAM = "role"

/** The params this screen remembers, and the only ones `filterBy` writes. */
export type FilterParam = typeof STATUS_PARAM | typeof ROLE_PARAM

/** No filter, in both dropdowns: the value a param that is absent stands for. */
export const ALL = "all"

/**
 * The one choice the daemon cannot answer on its own: the three statuses a
 * session with a live pane can be in. `GET /v1/sessions` takes a single status,
 * so this one is narrowed client-side — see `SessionListFilters.live`.
 */
export const LIVE = "live"

/** Every status, in the order the badge ramp declares them (live ones first). */
export const STATUSES = Object.keys(SESSION_STATUS_META) as SessionStatus[]

export const ROLES = Object.keys(ROLE_LABELS) as Role[]

/** A `?status=` this screen understands: a daemon status, or `live`. */
export type StatusValue = SessionStatus | typeof LIVE

/**
 * A status out of the form the param travels in — which is also the form it is
 * remembered in between visits. Anything the daemon does not define is no
 * filter, the way an unknown `?status=` is on the board.
 */
export function parseStatusFilter(value: string | null): StatusValue | null {
  if (value === LIVE) return LIVE
  return STATUSES.find((known) => known === value) ?? null
}

/** The same, for a role: one of the daemon's, or no filter. */
function parseRoleFilter(value: string | null): Role | null {
  return ROLES.find((known) => known === value) ?? null
}

/** What a `?status=` asks for, or `null` for no filter. */
export function readStatusFilter(params: URLSearchParams): StatusValue | null {
  return parseStatusFilter(params.get(STATUS_PARAM))
}

/** What a `?role=` asks for, or `null` for no filter. */
export function readRoleFilter(params: URLSearchParams): Role | null {
  return parseRoleFilter(params.get(ROLE_PARAM))
}

/**
 * The same params with one filter applied, every other one kept — an open
 * panel survives a change of filter.
 *
 * "All" drops the param rather than spelling a sentinel out: the unfiltered
 * screen is `/sessions`, not `/sessions?status=all`.
 */
export function withFilter(
  params: URLSearchParams,
  param: FilterParam,
  value: string,
): URLSearchParams {
  const next = new URLSearchParams(params)
  if (value === ALL || value === "") next.delete(param)
  else next.set(param, value)
  return next
}

/** `?status=` as the list's filters: `live` here, anything else at the daemon. */
export function statusFilters(
  value: StatusValue | null,
): Pick<SessionListFilters, "status" | "live"> {
  if (value === LIVE) return { live: true }
  return value ? { status: value } : {}
}

/** What the status trigger says. */
export function statusLabel(value: StatusValue | null): string {
  if (value === LIVE) return "Live"
  return value ? SESSION_STATUS_META[value].label : "All statuses"
}

/** What the role trigger says. */
export function roleLabel(value: Role | null): string {
  return value ? ROLE_LABELS[value] : "All roles"
}

/** The selections the screen was last left with, spelled as their params. */
interface RememberedFilters {
  status: string
  role: string
}

/**
 * The params the sessions route should be rewritten to when it is entered, or
 * `null` when the URL it was entered with already says everything.
 *
 * The URL stays the source of truth: a param on it — a deep link, a Back step,
 * a filter the user just set — is the answer, and the remembered value is only
 * consulted for the params it carries none of, which is what a sidebar link to
 * bare `/sessions` is. The two are restored independently, so an explicit
 * `?status=` still lets the remembered role back in. Whatever else the URL
 * carries (an open `?session=` panel) is kept, so restoring a filter never
 * closes a panel.
 *
 * A remembered "all" restores to nothing rather than to a param, and a value
 * the daemon no longer defines is dropped: a filter the user cleared stays
 * cleared, and a stale one is no filter at all.
 */
export function restoreSessionFilters(
  params: URLSearchParams,
  remembered: RememberedFilters,
): URLSearchParams | null {
  let next = params
  if (!params.has(STATUS_PARAM)) {
    const status = parseStatusFilter(remembered.status)
    if (status) next = withFilter(next, STATUS_PARAM, status)
  }
  if (!params.has(ROLE_PARAM)) {
    const role = parseRoleFilter(remembered.role)
    if (role) next = withFilter(next, ROLE_PARAM, role)
  }
  return next === params ? null : next
}

/**
 * The sessions screen's filters, as URL search params.
 *
 * They live in the URL for the reason the goals board's does: every other bit
 * of screen state is already there, a reload is routine in a hash-router
 * desktop app, and a narrowed screen is worth linking to. Each is one value —
 * `?status=failed`, `?role=engineer`, `?goal=<id>`, `?task=<id>` — and an
 * absent param is no filter.
 *
 * The last two are the daemon's own list filters, and on this screen they are
 * the screen's: `#/sessions?goal=<id>` is every agent that has run for one
 * goal, with a chip above the table saying so. Everywhere else those two params
 * open panels, which is why `components/detail-panels.tsx` has to know that this
 * screen claims them — the one place in the app where a param means two things.
 *
 * The URL only lasts as long as the user is on the screen, and the sidebar
 * links to a bare `/sessions`. So every selection is mirrored into the
 * persisted settings and put back on an entry that carries none of its own
 * — see `restoreSessionFilters` and `use-session-filters.ts`, which follow the
 * goals board's `filters.ts` / `use-status-filter.ts` pair.
 */

import type { Role, SessionStatus } from "@/api"
import { ROLE_LABELS } from "@/lib/format"

import type { SessionListFilters } from "./queries"
import { SESSION_STATUS_META } from "./session-display"

/** The params the filters travel in, alongside `?session=`. */
export const STATUS_PARAM = "status"
export const ROLE_PARAM = "role"
/** The two the daemon answers itself, and the ones a chip stands for. */
export const GOAL_PARAM = "goal"
export const TASK_PARAM = "task"

/** The params this screen remembers, and the only ones `filterBy` writes. */
export type FilterParam =
  | typeof STATUS_PARAM
  | typeof ROLE_PARAM
  | typeof GOAL_PARAM
  | typeof TASK_PARAM

/** The two that narrow the list to one piece of work, in the chips' order. */
const SCOPE_PARAMS = [GOAL_PARAM, TASK_PARAM] as const

/** Which of the two a scope filter is about. */
type ScopeParam = (typeof SCOPE_PARAMS)[number]

/** No filter, in both dropdowns: the value a param that is absent stands for. */
export const ALL = "all"

/**
 * The one choice the daemon cannot answer on its own: the three statuses a
 * session with a live pane can be in. `GET /v1/sessions` takes a single status,
 * so this one is narrowed client-side — see `SessionListFilters.live`.
 */
export const LIVE = "live"

/**
 * Not a status either, and not even about one: the sessions the daemon has
 * raised a reason on. It sits in the status dropdown because it is what that
 * dropdown is opened for — "show me the ones I have to do something about" —
 * and because it is exclusive with picking a status, being a cut across all of
 * them. Narrowed client-side by the strip's own rule; see
 * `SessionListFilters.attention`.
 */
export const ATTENTION = "attention"

/** Every status, in the order the badge ramp declares them (live ones first). */
export const STATUSES = Object.keys(SESSION_STATUS_META) as SessionStatus[]

export const ROLES = Object.keys(ROLE_LABELS) as Role[]

/** A `?status=` this screen understands: a daemon status, `live`, or `attention`. */
export type StatusValue = SessionStatus | typeof LIVE | typeof ATTENTION

/**
 * A status out of the form the param travels in — which is also the form it is
 * remembered in between visits. Anything the daemon does not define is no
 * filter, the way an unknown `?status=` is on the board.
 */
export function parseStatusFilter(value: string | null): StatusValue | null {
  if (value === LIVE) return LIVE
  if (value === ATTENTION) return ATTENTION
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
 * The id a `?goal=` or `?task=` narrows the list to, or `null`.
 *
 * There is no vocabulary to check it against the way a status or a role is
 * checked — it is an id, and only the daemon knows which ones exist. An id
 * nothing answers for is a list with nothing in it and a chip that clears it,
 * which is a better answer than silently dropping what the URL asked for.
 */
export function readScopeFilter(params: URLSearchParams, param: ScopeParam): string | null {
  return parseScopeFilter(params.get(param))
}

function parseScopeFilter(value: string | null): string | null {
  const trimmed = value?.trim() ?? ""
  return trimmed.length > 0 ? trimmed : null
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

/**
 * `?status=` as the list's filters: the two that are not statuses are narrowed
 * here, anything else at the daemon.
 */
export function statusFilters(
  value: StatusValue | null,
): Pick<SessionListFilters, "status" | "live" | "attention"> {
  if (value === LIVE) return { live: true }
  if (value === ATTENTION) return { attention: true }
  return value ? { status: value } : {}
}

/** What the status trigger says. */
export function statusLabel(value: StatusValue | null): string {
  if (value === LIVE) return "Live"
  if (value === ATTENTION) return "Needs attention"
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
  goal: string
  task: string
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
  for (const param of SCOPE_PARAMS) {
    if (params.has(param)) continue
    const scope = parseScopeFilter(param === GOAL_PARAM ? remembered.goal : remembered.task)
    if (scope) next = withFilter(next, param, scope)
  }
  return next === params ? null : next
}

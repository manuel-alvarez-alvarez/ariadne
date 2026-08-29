/**
 * Which goal lanes the board has folded away, remembered across reloads.
 *
 * The board has an opinion of its own — a finished goal opens collapsed, as
 * the one-line summary in its header — so "collapsed" is not a set of ids but
 * two of them: the lanes the user folded away, and the lanes the user opened
 * *against* that opinion. Anything in neither takes the board's default. A
 * single set could not tell "never touched" from "deliberately expanded", and
 * an expanded finished lane would fold itself away again on the next reload.
 *
 * Local rather than URL state on purpose: a collapsed lane is how *this* user
 * is reading the board right now, not something a shared `?goal=` link should
 * carry with it. It lives in `localStorage` next to the theme, and a store
 * that cannot be read or written (a webview with storage disabled) degrades to
 * "every lane takes the board's default" rather than breaking the board.
 */

import { useCallback, useState } from "react"

const COLLAPSED_LANES_KEY = "ariadne.goals.collapsed-lanes"

/** What the user has said about individual lanes, either way. */
export interface LaneCollapse {
  /** Lanes folded away by hand. */
  collapsed: ReadonlySet<string>
  /** Lanes opened by hand, whatever the board would have done with them. */
  expanded: ReadonlySet<string>
}

/** Nothing said about any lane: every one of them takes the board's default. */
const NOTHING: LaneCollapse = { collapsed: new Set(), expanded: new Set() }

/**
 * What was stored; anything that is not one of the two shapes reads as nothing.
 *
 * The older form was a plain list of collapsed ids, which is still what a
 * board written by a previous version left behind — it reads as "these were
 * folded away, nothing was expanded", which is exactly what it meant.
 */
export function parseCollapsed(raw: string | null): LaneCollapse {
  if (!raw) return NOTHING
  try {
    const parsed: unknown = JSON.parse(raw)
    if (Array.isArray(parsed)) return { collapsed: ids(parsed), expanded: new Set() }
    if (!parsed || typeof parsed !== "object") return NOTHING
    const record = parsed as { collapsed?: unknown; expanded?: unknown }
    return { collapsed: ids(record.collapsed), expanded: ids(record.expanded) }
  } catch {
    return NOTHING
  }
}

/** The stored form: two id lists, stable in order so writes stay diffable. */
export function serializeCollapsed(state: LaneCollapse): string {
  return JSON.stringify({
    collapsed: [...state.collapsed].sort(),
    expanded: [...state.expanded].sort(),
  })
}

/**
 * The state with one lane's answer written down — always explicitly, whichever
 * way it went, so a lane the user opened stays open and a lane the user folded
 * stays folded even where the board's default is the other one.
 */
export function setLaneCollapsed(
  state: LaneCollapse,
  id: string,
  collapsed: boolean,
): LaneCollapse {
  const next = {
    collapsed: new Set(state.collapsed),
    expanded: new Set(state.expanded),
  }
  next.collapsed.delete(id)
  next.expanded.delete(id)
  ;(collapsed ? next.collapsed : next.expanded).add(id)
  return next
}

/** Whether this lane is folded away: what the user said, else the board's default. */
export function isLaneCollapsed(state: LaneCollapse, id: string, byDefault: boolean): boolean {
  if (state.collapsed.has(id)) return true
  if (state.expanded.has(id)) return false
  return byDefault
}

export function useCollapsedLanes(): {
  /** `byDefault` is the board's own opinion for this lane; the user overrides it. */
  isCollapsed: (goalId: string, byDefault: boolean) => boolean
  setCollapsed: (goalId: string, collapsed: boolean) => void
} {
  const [state, setState] = useState(load)

  const setCollapsed = useCallback((goalId: string, collapsed: boolean) => {
    setState((current) => {
      const next = setLaneCollapsed(current, goalId, collapsed)
      save(next)
      return next
    })
  }, [])

  const isCollapsed = useCallback(
    (goalId: string, byDefault: boolean) => isLaneCollapsed(state, goalId, byDefault),
    [state],
  )

  return { isCollapsed, setCollapsed }
}

/** The ids out of a stored list; anything that is not one is dropped. */
function ids(value: unknown): Set<string> {
  if (!Array.isArray(value)) return new Set()
  return new Set(value.filter((id): id is string => typeof id === "string"))
}

function load(): LaneCollapse {
  try {
    return parseCollapsed(localStorage.getItem(COLLAPSED_LANES_KEY))
  } catch {
    return NOTHING
  }
}

function save(state: LaneCollapse): void {
  try {
    localStorage.setItem(COLLAPSED_LANES_KEY, serializeCollapsed(state))
  } catch {
    // A board that cannot remember its collapsed lanes still works.
  }
}

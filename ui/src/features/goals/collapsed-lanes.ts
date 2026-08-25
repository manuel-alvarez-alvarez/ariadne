/**
 * Which goal lanes the board has folded away, remembered across reloads.
 *
 * Local rather than URL state on purpose: a collapsed lane is how *this* user
 * is reading the board right now, not something a shared `?goal=` link should
 * carry with it. It lives in `localStorage` next to the theme, and a store
 * that cannot be read or written (a webview with storage disabled) degrades to
 * "nothing is collapsed" rather than breaking the board.
 */

import { useCallback, useState } from "react"

const COLLAPSED_LANES_KEY = "ariadne.goals.collapsed-lanes"

/** Ids out of a stored value; anything that is not a list of ids reads as none. */
export function parseCollapsed(raw: string | null): Set<string> {
  if (!raw) return new Set()
  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((id): id is string => typeof id === "string"))
  } catch {
    return new Set()
  }
}

/** The stored form: a plain id list, stable in order so writes stay diffable. */
export function serializeCollapsed(ids: Iterable<string>): string {
  return JSON.stringify([...ids].sort())
}

/** `ids` with `id` flipped, as a new set — the collapse toggle's whole model. */
export function toggleCollapsed(ids: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(ids)
  if (!next.delete(id)) next.add(id)
  return next
}

export function useCollapsedLanes(): {
  collapsed: ReadonlySet<string>
  toggle: (goalId: string) => void
} {
  const [collapsed, setCollapsed] = useState(load)

  const toggle = useCallback(
    (goalId: string) => {
      const next = toggleCollapsed(collapsed, goalId)
      setCollapsed(next)
      save(next)
    },
    [collapsed],
  )

  return { collapsed, toggle }
}

function load(): Set<string> {
  try {
    return parseCollapsed(localStorage.getItem(COLLAPSED_LANES_KEY))
  } catch {
    return new Set()
  }
}

function save(ids: Set<string>): void {
  try {
    localStorage.setItem(COLLAPSED_LANES_KEY, serializeCollapsed(ids))
  } catch {
    // A board that cannot remember its collapsed lanes still works.
  }
}

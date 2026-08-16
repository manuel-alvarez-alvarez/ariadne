/**
 * Every URL in the app, in one place. Link with these helpers instead of
 * hand-writing paths so a feature can move its routes without breaking the
 * links other features have to it.
 *
 * Goals and tasks have no pages of their own: their details open in side
 * panels driven by search params (`?goal=` on the goals board, `?task=` on
 * any screen), which `src/components/detail-panels.tsx` reads.
 */

import { useSearchParams } from "react-router-dom"

export const paths = {
  goals: () => "/goals",
  /** The goals board with this goal's panel open. */
  goal: (goalId: string) => `/goals?goal=${goalId}`,
  sessions: () => "/sessions",
  session: (sessionId: string) => `/sessions/${sessionId}`,
  profiles: () => "/profiles",
} as const

/**
 * Link target that opens the task's panel over the current screen: same
 * pathname, `?task=` added, every other filter or panel param kept — so a
 * task opened from a goal's lane stacks on that goal's panel.
 */
export function taskPanelTo(current: URLSearchParams, taskId: string): { search: string } {
  const next = new URLSearchParams(current)
  next.set("task", taskId)
  next.delete("tab")
  return { search: `?${next.toString()}` }
}

/** `taskPanelTo` against the current location, for links outside a list. */
export function useTaskPanelTo(taskId: string): { search: string } {
  const [search] = useSearchParams()
  return taskPanelTo(search, taskId)
}

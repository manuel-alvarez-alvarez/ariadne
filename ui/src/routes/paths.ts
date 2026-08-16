/**
 * Every URL in the app, in one place. Link with these helpers instead of
 * hand-writing paths so a feature can move its routes without breaking the
 * links other features have to it.
 *
 * Goals, tasks and sessions have no pages of their own: their details open in
 * side panels driven by search params (`?goal=` on the goals board, `?task=`
 * on any screen, `?tab=sessions&session=` inside either panel), which
 * `src/components/detail-panels.tsx` reads.
 */

import { useSearchParams } from "react-router-dom"

export const paths = {
  goals: () => "/goals",
  /** The goals board with this goal's panel open. */
  goal: (goalId: string) => `/goals?goal=${goalId}`,
  attention: () => "/attention",
  sessions: () => "/sessions",
  profiles: () => "/profiles",
  /**
   * The goals board with this goal's panel open on one of its sessions.
   *
   * The only way to show a planner session, which belongs to no task: the goal
   * panel opens on the board and nowhere else (see `detail-panels.tsx`), so
   * this leaves whatever screen the link was on.
   */
  goalSession: (goalId: string, sessionId: string) =>
    `/goals?goal=${goalId}&tab=sessions&session=${sessionId}`,
} as const

/**
 * Link target that opens the task's panel over the current screen: same
 * pathname, `?task=` added, every other filter or panel param kept — so a
 * task opened from a goal's lane stacks on that goal's panel.
 *
 * The panel's own params go: `tab` and `session` belong to whichever panel
 * put them there, and would otherwise open the new one on a tab or a session
 * that is not its.
 */
export function taskPanelTo(current: URLSearchParams, taskId: string): { search: string } {
  const next = new URLSearchParams(current)
  next.set("task", taskId)
  next.delete("tab")
  next.delete("session")
  return { search: `?${next.toString()}` }
}

/** `taskPanelTo` against the current location, for links outside a list. */
export function useTaskPanelTo(taskId: string): { search: string } {
  const [search] = useSearchParams()
  return taskPanelTo(search, taskId)
}

/**
 * Link target that shows a session inside the panel that is already open:
 * everything else is kept, `tab` and `session` point the panel at it.
 *
 * This is how a session id mentioned somewhere in a panel — a message's
 * author, a review's session — becomes a way to watch that agent.
 */
export function panelSessionTo(current: URLSearchParams, sessionId: string): { search: string } {
  const next = new URLSearchParams(current)
  next.set("tab", "sessions")
  next.set("session", sessionId)
  return { search: `?${next.toString()}` }
}

/** `panelSessionTo` against the current location. */
export function usePanelSessionTo(sessionId: string): { search: string } {
  const [search] = useSearchParams()
  return panelSessionTo(search, sessionId)
}

/**
 * Link target that opens a session from a list that is not inside any panel —
 * the sessions screen, the attention screen — by opening its *task's* panel on
 * it: `?task=` first, then the panel's own `?tab=sessions&session=`.
 *
 * The screen underneath keeps its filters, and stays on screen behind the
 * panel, which is what makes the row the panel came from worth highlighting.
 */
export function taskSessionPanelTo(
  current: URLSearchParams,
  taskId: string,
  sessionId: string,
): { search: string } {
  const withTask = new URLSearchParams(taskPanelTo(current, taskId).search)
  return panelSessionTo(withTask, sessionId)
}

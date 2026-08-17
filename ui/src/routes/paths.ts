/**
 * Every URL in the app, in one place. Link with these helpers instead of
 * hand-writing paths so a feature can move its routes without breaking the
 * links other features have to it.
 *
 * Goals, tasks and sessions have no pages of their own: their details open in
 * side panels driven by search params (`?goal=` on the goals board, `?task=`
 * on any screen, `?session=` on its own for a session's own panel, and
 * `?tab=sessions&session=` for a session *inside* a goal's or a task's panel),
 * which `src/components/detail-panels.tsx` reads.
 */

import { useSearchParams } from "react-router-dom"

/** The param {@link paths.profile} carries, read by the profiles screen. */
export const PROFILE_EXPAND_PARAM = "expand"

export const paths = {
  goals: () => "/goals",
  /** The goals board with this goal's panel open. */
  goal: (goalId: string) => `/goals?goal=${goalId}`,
  attention: () => "/attention",
  sessions: () => "/sessions",
  profiles: () => "/profiles",
  /**
   * The profiles screen, opened on one profile: a row expands in place instead
   * of having a page of its own, so the link asks the screen to expand it and
   * scroll to it (see `features/profiles/profiles-page.tsx`).
   */
  profile: (profileId: string) => `/profiles?${PROFILE_EXPAND_PARAM}=${profileId}`,
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
 * Link target that opens a session's own panel over the current screen: same
 * pathname, `?session=` added, every filter the screen owns kept — so the list
 * it was picked from stays behind it, with the picked row still marked.
 *
 * Unlike {@link panelSessionTo}, the session here is not inside anything: a
 * `?session=` with no `?goal=` and no `?task=` around it *is* the panel (see
 * `detail-panels.tsx`). The screens this is used from carry none of those
 * three, and clearing them is what keeps that true wherever it is used.
 */
export function sessionPanelTo(current: URLSearchParams, sessionId: string): { search: string } {
  const next = new URLSearchParams(current)
  next.set("session", sessionId)
  next.delete("goal")
  next.delete("task")
  next.delete("tab")
  return { search: `?${next.toString()}` }
}

/**
 * Link target that shows a session inside the panel that is already open:
 * everything else is kept, `tab` and `session` point the panel at it. `null`
 * is the way back out of the session, onto the list it came from.
 *
 * This is how a session id mentioned somewhere in a panel — a message's
 * author, a review's session — becomes a way to watch that agent.
 */
export function panelSessionTo(
  current: URLSearchParams,
  sessionId: string | null,
): { search: string } {
  const next = new URLSearchParams(current)
  next.set("tab", "sessions")
  if (sessionId === null) next.delete("session")
  else next.set("session", sessionId)
  return { search: `?${next.toString()}` }
}

/** `panelSessionTo` against the current location. */
export function usePanelSessionTo(sessionId: string): { search: string } {
  const [search] = useSearchParams()
  return panelSessionTo(search, sessionId)
}

/**
 * Drilling the open panel into a session (and back out of it with `null`) as
 * something to call rather than to link: the sessions list hands back the row
 * that was clicked, not a target.
 *
 * Replaces rather than pushes — where the user is *inside* a panel is not a
 * step of its own, and Back should leave the panel, not walk its tabs.
 */
export function usePanelSessionNavigation(): (sessionId: string | null) => void {
  const [search, setSearch] = useSearchParams()
  return (sessionId) => setSearch(panelSessionTo(search, sessionId).search, { replace: true })
}

/**
 * Link target that opens a session inside its *task's* panel from outside any
 * panel — the command palette: `?task=` first, then the panel's own
 * `?tab=sessions&session=`.
 *
 * The screen underneath keeps its filters, and stays on screen behind the
 * panel. Where the session is the thing being opened rather than the task it
 * ran, {@link sessionPanelTo} gives it a panel of its own instead.
 */
export function taskSessionPanelTo(
  current: URLSearchParams,
  taskId: string,
  sessionId: string,
): { search: string } {
  const withTask = new URLSearchParams(taskPanelTo(current, taskId).search)
  return panelSessionTo(withTask, sessionId)
}

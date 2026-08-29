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

import { useEffect, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"

/**
 * Which profile row is expanded, on the profiles screen: what
 * {@link paths.profile} points at, and where that screen keeps the expansion
 * it is showing.
 */
export const PROFILE_EXPAND_PARAM = "expand"

/**
 * What a link asks the screen it opens to hand the keyboard to, under
 * `?focus=`: the compose box of a thread, or a session's terminal pane.
 *
 * The attention list is where these come from — a row that says an agent is
 * waiting on you has to land on the box the answer is typed into, not merely
 * on the screen that contains it. It is a request rather than a state: it is
 * read once by whichever control it names and dropped from the URL there (see
 * {@link useComposerRequest}), so a reload, a tab switch or a second panel
 * opened later never takes the keyboard again.
 */
const FOCUS_PARAM = "focus"

/**
 * Whom a compose box opened by a link starts addressed to, under `?to=` — the
 * profile of the agent that asked. Travels with {@link FOCUS_PARAM} and is
 * dropped with it.
 */
const ADDRESSEE_PARAM = "to"

/** The controls {@link FOCUS_PARAM} can name. */
type FocusTarget = "composer" | "terminal"

export const paths = {
  goals: () => "/goals",
  /** The goals board with this goal's panel open. */
  goal: (goalId: string) => `/goals?goal=${goalId}`,
  profiles: () => "/profiles",
  /**
   * The profiles screen, opened on one profile: a row expands in place instead
   * of having a page of its own, so the link asks the screen to expand it and
   * scroll to it (see `features/profiles/profiles-page.tsx`).
   *
   * It carries no `?role=`, which is what keeps the row it names out of the
   * role tab that happened to be up when the link was followed.
   */
  profile: (profileId: string) => `/profiles?${PROFILE_EXPAND_PARAM}=${profileId}`,
  /**
   * Every session there is, filtered on the screen itself. A session's own
   * details are a `?session=` panel over whatever screen picked it (see
   * {@link sessionPanelTo}), this being the one that lists them all.
   */
  sessions: () => "/sessions",
  agents: () => "/agents",
  repositories: () => "/repositories",
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
  const next = withoutArrival(current)
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
  const next = withoutArrival(current)
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
  const next = withoutArrival(current)
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

/**
 * The same params with any unread arrival request dropped.
 *
 * Every panel link goes through this: a request belongs to the one link that
 * made it, and one left behind — the panel it named failed to load, so nothing
 * ever read it — must not be picked up by the next panel opened over it.
 */
function withoutArrival(current: URLSearchParams): URLSearchParams {
  const next = new URLSearchParams(current)
  next.delete(FOCUS_PARAM)
  next.delete(ADDRESSEE_PARAM)
  return next
}

/** A panel link that also asks for a control on the way in. */
function arriving(
  target: { search: string },
  focus: FocusTarget,
  addressee?: string | null,
): { search: string } {
  const next = new URLSearchParams(target.search)
  next.set(FOCUS_PARAM, focus)
  if (addressee) next.set(ADDRESSEE_PARAM, addressee)
  return { search: `?${next.toString()}` }
}

/**
 * Link target that opens the task's panel on its conversation, with the
 * compose box focused and addressed to `addressee`.
 *
 * Where an agent waiting on the *user* is answered: what it asked is a message
 * in this thread, and the answer is another one — so the row that says so
 * lands on the box that writes it, already addressed to whoever asked.
 */
export function taskConversationTo(
  current: URLSearchParams,
  taskId: string,
  addressee?: string | null,
): { search: string } {
  const next = new URLSearchParams(taskPanelTo(current, taskId).search)
  next.set("tab", "conversation")
  return arriving({ search: `?${next.toString()}` }, "composer", addressee)
}

/**
 * {@link taskConversationTo} for a planner, whose thread is the goal's: it has
 * no task to be answered in, and the goal panel only opens on the board — so
 * this one carries a pathname of its own.
 */
export function goalThreadTo(
  current: URLSearchParams,
  goalId: string,
  addressee?: string | null,
): { pathname: string; search: string } {
  const next = withoutArrival(current)
  next.set("goal", goalId)
  next.set("tab", "thread")
  next.delete("task")
  next.delete("session")
  return {
    pathname: paths.goals(),
    ...arriving({ search: `?${next.toString()}` }, "composer", addressee),
  }
}

/**
 * Link target that opens a session's own panel on its terminal, with the pane
 * focused — where an agent blocked on a prompt is answered, since what it is
 * waiting for is a keystroke in that pane and nothing else.
 */
export function sessionTerminalTo(current: URLSearchParams, sessionId: string): { search: string } {
  const next = new URLSearchParams(sessionPanelTo(current, sessionId).search)
  next.set("tab", "terminal")
  return arriving({ search: `?${next.toString()}` }, "terminal")
}

/**
 * Whether the link that opened this screen asked for a control, read once.
 *
 * Frozen at mount and dropped from the URL in the same breath, which is what
 * makes it a request and not a state: the control it names may not exist yet
 * (a terminal has a snapshot to wait for), so the answer has to survive until
 * it does — and once it has been given, a re-render, a tab switch or a reload
 * must not give it again.
 */
function useArrival(target: FocusTarget): boolean {
  const [search, setSearch] = useSearchParams()
  const [asked] = useState(() => search.get(FOCUS_PARAM) === target)
  // The effect below runs once, and the params it rewrites are whatever they
  // are by then — not the ones this render closed over.
  const latest = useRef({ search, setSearch })
  latest.current = { search, setSearch }

  useEffect(() => {
    if (!asked) return
    const { search, setSearch } = latest.current
    setSearch(withoutArrival(search), { replace: true })
  }, [asked])

  return asked
}

/**
 * What a link asked of the compose box below a thread: to take the keyboard,
 * and to open addressed to the agent that asked.
 */
export function useComposerRequest(): { focus: boolean; to: string | null } {
  const [search] = useSearchParams()
  const focus = useArrival("composer")
  const [to] = useState(() => search.get(ADDRESSEE_PARAM))
  return { focus, to: focus ? to : null }
}

/** Whether a link asked this session's pane to take the keyboard. */
export function useTerminalFocusRequest(): boolean {
  return useArrival("terminal")
}

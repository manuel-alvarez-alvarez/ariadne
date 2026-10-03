/**
 * The URL-driven side panel: `?goal=` (on the goals board) opens the goal
 * panel, `?task=` (on any screen but the sessions one) opens the task panel,
 * and a `?session=` with neither of those around it opens that session's own
 * panel. One pane, one panel at a time — a `?task=` takes it whatever else the
 * URL also names, since a task replaces its goal rather than sitting over it
 * (see `features/tasks/task-card.tsx` and `task-panel.tsx`'s breadcrumb for
 * the other side of that navigation).
 * Mounted once by the shell, so a task or a session can be opened from the
 * board, a list or another panel without leaving the screen it was on.
 *
 * Inside a goal's or a task's panel, `?session=` means something else — the
 * session the panel is drilled into (`?tab=sessions&session=`) — which is why
 * the standalone one is exactly the case where no panel owns it.
 *
 * Closing a panel unwinds the history entry that opened it — see
 * `routes/panel-history.ts` for why that is not the same as rewriting the URL.
 */

import { Navigate, useLocation, useNavigate, useSearchParams } from "react-router-dom"

import { GoalPanel } from "@/features/goals/goal-panel"
import { SessionPanel } from "@/features/sessions/session-panel"
import { TaskPanel } from "@/features/tasks"
import { closePanel, type Panel } from "@/routes/panel-history"
import { paths } from "@/routes/paths"

export function DetailPanels() {
  const location = useLocation()
  const navigate = useNavigate()
  const [search, setSearch] = useSearchParams()

  const goalId = search.get("goal")
  const taskId = search.get("task")
  const sessionId = search.get("session")
  const onGoalsBoard = location.pathname === paths.goals()

  function close(panel: Panel) {
    const step = closePanel(panel, search, window.history.state)
    if (step.kind === "back") navigate(-1)
    else setSearch(step.search, { replace: true })
  }

  // The sessions screen is the one screen that owns `?goal=` and `?task=`
  // itself: there they are what the list is narrowed to, with a chip above the
  // table saying so (`features/sessions/filters.ts`). So nothing is opened over
  // it except the session panel — which is what a row there opens, filters and
  // all still behind it.
  if (location.pathname === paths.sessions()) {
    return sessionId ? (
      <SessionPanel sessionId={sessionId} onClose={() => close("session")} />
    ) : null
  }

  // A task names itself regardless of what else the URL carries — a stray
  // `?goal=` included, which a hand-written link can still hold even though
  // nothing in the app opens the two together.
  if (taskId) {
    return <TaskPanel taskId={taskId} onClose={() => close("task")} />
  }

  // The goal panel belongs to the board, so a `?goal=` link followed from
  // anywhere else goes there rather than doing nothing where it was clicked.
  // Replaced, so Back returns to the screen the link was on.
  if (goalId && !onGoalsBoard) {
    return <Navigate to={{ pathname: paths.goals(), search: location.search }} replace />
  }

  if (goalId) {
    return <GoalPanel goalId={goalId} onClose={() => close("goal")} />
  }

  // Nothing else claims this `?session=`, so it is a session in its own right:
  // its panel opens over whichever screen it was picked from, list and filters
  // still behind it.
  if (sessionId) {
    return <SessionPanel sessionId={sessionId} onClose={() => close("session")} />
  }

  return null
}

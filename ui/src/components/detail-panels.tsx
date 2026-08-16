/**
 * The URL-driven side panels: `?goal=` (on the goals board) opens the goal
 * panel, `?task=` (on any screen) opens the task panel on top of it. Mounted
 * once by the shell, so a task can be opened from the board or a session
 * without leaving that screen.
 *
 * Open together, they are one stack rather than two panels: the task panel is
 * handed to the goal panel, which renders it inside its own dialog (see
 * `goal-panel.tsx`), so the goal keeps showing behind it, the screen is
 * darkened once, and Escape closes only the sheet on top.
 */

import { Navigate, useLocation, useSearchParams } from "react-router-dom"

import { GoalPanel } from "@/features/goals/goal-panel"
import { TaskPanel } from "@/features/tasks"
import { paths } from "@/routes/paths"

export function DetailPanels() {
  const location = useLocation()
  const [search, setSearch] = useSearchParams()

  const goalId = search.get("goal")
  const taskId = search.get("task")
  const onGoalsBoard = location.pathname === paths.goals()

  // Closing takes the panel's state with it: `tab` and `session` say where
  // *inside* a panel the user was, and mean nothing once it is gone.
  //
  // It replaces rather than pushes: a closed panel is where the user already
  // was, so Back should leave that screen, not put the panel back.
  function close(...params: string[]) {
    const next = new URLSearchParams(search)
    for (const param of params) next.delete(param)
    setSearch(next, { replace: true })
  }

  // The goal panel belongs to the board, so a `?goal=` link followed from
  // anywhere else goes there rather than doing nothing where it was clicked.
  // Replaced, so Back returns to the screen the link was on.
  if (goalId && !onGoalsBoard) {
    return <Navigate to={{ pathname: paths.goals(), search: location.search }} replace />
  }

  const taskPanel = taskId ? (
    <TaskPanel
      taskId={taskId}
      stackedOnGoal={goalId ?? undefined}
      onClose={() => close("task", "tab", "session")}
    />
  ) : null

  if (goalId) {
    return (
      <GoalPanel
        goalId={goalId}
        onClose={() => close("goal", "task", "tab", "session")}
        stackedPanel={taskPanel}
      />
    )
  }

  return taskPanel
}

/**
 * The URL-driven side panels: `?goal=` (on the goals board) opens the goal
 * panel, `?task=` (on any screen) opens the task panel on top of it. Mounted
 * once by the shell, so a task can be opened from the board or a session
 * without leaving that screen.
 */

import { useLocation, useSearchParams } from "react-router-dom"

import { GoalPanel } from "@/features/goals/goal-panel"
import { TaskPanel } from "@/features/tasks"
import { paths } from "@/routes/paths"

export function DetailPanels() {
  const location = useLocation()
  const [search, setSearch] = useSearchParams()

  const goalId = location.pathname === paths.goals() ? search.get("goal") : null
  const taskId = search.get("task")

  function close(...params: string[]) {
    const next = new URLSearchParams(search)
    for (const param of params) next.delete(param)
    setSearch(next)
  }

  return (
    <>
      {goalId && <GoalPanel goalId={goalId} onClose={() => close("goal", "task", "tab")} />}
      {taskId && <TaskPanel taskId={taskId} onClose={() => close("task", "tab")} />}
    </>
  )
}

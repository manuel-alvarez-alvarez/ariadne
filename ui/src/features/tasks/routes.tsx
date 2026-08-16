/**
 * Routes owned by the tasks feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 *
 * Tasks have no screens of their own: a task opens as a side panel over the
 * screen it was clicked on. These routes only keep old `/tasks/...` deep
 * links working by redirecting them to the goals board (with the task's
 * panel open, when the link named one).
 */

import { Navigate, type RouteObject, useParams } from "react-router-dom"

import { paths } from "@/routes/paths"

function TaskPanelRedirect() {
  const { taskId = "" } = useParams<{ taskId: string }>()
  return <Navigate to={`${paths.goals()}?task=${taskId}`} replace />
}

export const taskRoutes: RouteObject[] = [
  { path: "tasks", element: <Navigate to={paths.goals()} replace /> },
  { path: "tasks/:taskId", element: <TaskPanelRedirect /> },
]

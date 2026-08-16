/**
 * Routes owned by the tasks feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { TaskDetailPage } from "./task-detail-page"
import { TasksListPage } from "./tasks-list-page"

export const taskRoutes: RouteObject[] = [
  { path: "tasks", element: <TasksListPage /> },
  { path: "tasks/:taskId", element: <TaskDetailPage /> },
]

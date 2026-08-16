/**
 * Routes owned by the goals feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { GoalDetailPage } from "./goal-detail-page"
import { GoalsListPage } from "./goals-list-page"

export const goalRoutes: RouteObject[] = [
  { path: "goals", element: <GoalsListPage /> },
  { path: "goals/:goalId", element: <GoalDetailPage /> },
]

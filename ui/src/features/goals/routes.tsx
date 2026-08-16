/**
 * Routes owned by the goals feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 *
 * There is no goal page: a goal opens as a side panel on the board, so the
 * old `/goals/:goalId` deep links redirect to the panel URL.
 */

import { Navigate, type RouteObject, useParams } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"
import { paths } from "@/routes/paths"
import { GoalsListPage } from "./goals-list-page"

function GoalPanelRedirect() {
  const { goalId = "" } = useParams<{ goalId: string }>()
  return <Navigate to={paths.goal(goalId)} replace />
}

export const goalRoutes: RouteObject[] = [
  { path: "goals", element: <GoalsListPage />, handle: pageTitle("Goals") },
  { path: "goals/:goalId", element: <GoalPanelRedirect /> },
]

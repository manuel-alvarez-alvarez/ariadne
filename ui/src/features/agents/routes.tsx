/**
 * Routes owned by the agents feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"

import { AgentsPage } from "./agents-page"

export const agentRoutes: RouteObject[] = [
  { path: "agents", element: <AgentsPage />, handle: pageTitle("Agents") },
]

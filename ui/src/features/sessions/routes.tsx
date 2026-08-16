/**
 * Routes owned by the sessions feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { SessionDetailPage } from "./session-detail-page"
import { SessionsListPage } from "./sessions-list-page"

export const sessionRoutes: RouteObject[] = [
  { path: "sessions", element: <SessionsListPage /> },
  { path: "sessions/:sessionId", element: <SessionDetailPage /> },
]

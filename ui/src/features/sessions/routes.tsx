/**
 * Routes owned by the sessions feature. Mounted by `src/routes/router.tsx`;
 * add sub-routes here rather than in the router.
 *
 * A session has no page of its own: it opens in a side panel (`?session=`,
 * see `session-panel.tsx`) over whatever screen the link was on.
 */

import type { RouteObject } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"

import { SessionsPage } from "./sessions-page"

export const sessionRoutes: RouteObject[] = [
  { path: "sessions", element: <SessionsPage />, handle: pageTitle("Sessions") },
]

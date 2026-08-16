/**
 * Routes owned by the profiles feature. Mounted by `src/routes/router.tsx`; add
 * sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"

import { ProfilesPage } from "./profiles-page"

export const profileRoutes: RouteObject[] = [
  { path: "profiles", element: <ProfilesPage />, handle: pageTitle("Profiles") },
]

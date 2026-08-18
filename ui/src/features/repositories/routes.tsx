/**
 * Routes owned by the repositories feature. Mounted by `src/routes/router.tsx`;
 * add sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"

import { RepositoriesPage } from "./repositories-page"

export const repositoryRoutes: RouteObject[] = [
  { path: "repositories", element: <RepositoriesPage />, handle: pageTitle("Repositories") },
]

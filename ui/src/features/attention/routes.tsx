/**
 * Routes owned by the attention feature. Mounted by `src/routes/router.tsx`;
 * add sub-routes here rather than in the router.
 */

import type { RouteObject } from "react-router-dom"

import { pageTitle } from "@/routes/page-title"

import { AttentionPage } from "./attention-page"

export const attentionRoutes: RouteObject[] = [
  { path: "attention", element: <AttentionPage />, handle: pageTitle("Attention") },
]

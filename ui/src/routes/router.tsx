/**
 * The app's route table.
 *
 * Each feature owns a `routes.tsx` in its own directory and exports the routes
 * it is responsible for; this file only mounts them under the shell. Feature
 * tasks add screens there, not here.
 *
 * A hash router is used on purpose: in a packaged Tauri build the frontend is
 * served straight off the asset protocol with no history fallback, so a reload
 * on a deep link has to resolve client-side.
 */

import { createHashRouter, Navigate } from "react-router-dom"

import { AppShell } from "@/components/app-shell"
import { attentionRoutes } from "@/features/attention/routes"
import { goalRoutes } from "@/features/goals/routes"
import { profileRoutes } from "@/features/profiles/routes"
import { sessionRoutes } from "@/features/sessions/routes"
import { taskRoutes } from "@/features/tasks/routes"
import { NotFoundPage } from "@/routes/not-found-page"
import { paths } from "@/routes/paths"

export const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    errorElement: <NotFoundPage />,
    children: [
      { index: true, element: <Navigate to={paths.goals()} replace /> },
      ...goalRoutes,
      ...attentionRoutes,
      ...sessionRoutes,
      ...taskRoutes,
      ...profileRoutes,
      { path: "*", element: <NotFoundPage /> },
    ],
  },
])

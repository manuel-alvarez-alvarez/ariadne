/**
 * The app's route table, and what the header calls each screen.
 *
 * Every route is here rather than one file per feature: there are eight of
 * them, half are a single line, and a `routes.tsx` that mounts one page said
 * less about the feature than the line it held.
 *
 * Goals, tasks and sessions have no pages of their own — they open as side
 * panels driven by search params (see `paths.ts`) — so what is left of their
 * old `/goals/:goalId` and `/tasks/:taskId` deep links is a redirect onto the
 * board with the panel open.
 *
 * What the header calls each screen rides on the route's own `handle` — a
 * screen is named where it is mounted, and `AppShell` reads the deepest one
 * that declares a title.
 *
 * A hash router is used on purpose: in a packaged Tauri build the frontend is
 * served straight off the asset protocol with no history fallback, so a reload
 * on a deep link has to resolve client-side.
 */

import { createHashRouter, Navigate, type RouteObject, useParams } from "react-router-dom"

import { AppShell, type PageHandle } from "@/components/app-shell"
import { AgentsPage } from "@/features/agents/agents-page"
import { GoalsListPage } from "@/features/goals/goals-list-page"
import { ProfilesPage } from "@/features/profiles/profiles-page"
import { RepositoriesPage } from "@/features/repositories/repositories-page"
import { SessionsPage } from "@/features/sessions/sessions-page"
import { NotFoundPage } from "@/routes/not-found-page"
import { paths } from "@/routes/paths"

function GoalPanelRedirect() {
  const { goalId = "" } = useParams<{ goalId: string }>()
  return <Navigate to={paths.goal(goalId)} replace />
}

function TaskPanelRedirect() {
  const { taskId = "" } = useParams<{ taskId: string }>()
  return <Navigate to={`${paths.goals()}?task=${taskId}`} replace />
}

const routes: RouteObject[] = [
  { index: true, element: <Navigate to={paths.goals()} replace /> },
  { path: "goals", element: <GoalsListPage />, handle: { title: "Goals" } satisfies PageHandle },
  { path: "goals/:goalId", element: <GoalPanelRedirect /> },
  { path: "tasks", element: <Navigate to={paths.goals()} replace /> },
  { path: "tasks/:taskId", element: <TaskPanelRedirect /> },
  { path: "sessions", element: <SessionsPage />, handle: { title: "Sessions" } },
  { path: "profiles", element: <ProfilesPage />, handle: { title: "Profiles" } },
  { path: "agents", element: <AgentsPage />, handle: { title: "Agents" } },
  { path: "repositories", element: <RepositoriesPage />, handle: { title: "Repositories" } },
  { path: "*", element: <NotFoundPage /> },
]

export const router = createHashRouter([
  { path: "/", element: <AppShell />, errorElement: <NotFoundPage />, children: routes },
])

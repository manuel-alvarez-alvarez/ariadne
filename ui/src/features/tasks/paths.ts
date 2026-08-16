/**
 * URLs of the routes this feature owns and that `src/routes/paths.ts` does not
 * spell out: the flat task list and its filters. Task detail links go through
 * the shared helper (`paths.task`) like everyone else's.
 */

import type { TaskStatus } from "@/api"

/** The flat list, mounted by this feature's `routes.tsx`. */
export const TASKS_PATH = "/tasks"

export interface TaskListSearch {
  goal?: string
  status?: TaskStatus
}

export function tasksPath(search: TaskListSearch = {}): string {
  const query = new URLSearchParams()
  if (search.goal) query.set("goal", search.goal)
  if (search.status) query.set("status", search.status)
  const suffix = query.toString()
  return suffix ? `${TASKS_PATH}?${suffix}` : TASKS_PATH
}

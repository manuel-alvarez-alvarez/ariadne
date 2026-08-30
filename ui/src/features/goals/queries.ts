/**
 * Everything the goal screens ask the daemon for.
 *
 * Keys are built from the `qk` helpers, so the SSE dispatcher keeps reaching
 * them: it invalidates `qk.goals.lists()` and patches `qk.goals.detail(id)`,
 * and every key below is either exactly one of those or nested under it. The
 * cache work each write does is `@/api`'s — see `cacheRow`, `dropRow` and
 * `useRowAction` — and what is left here is what only a goal knows: which
 * endpoint, and what else it moved.
 *
 * Two lists here are filtered server-side (`?status=`, `?role=planner`) but the
 * shared filter types carry no `status` / `role` field, so their filter segment
 * is appended to `qk.<entity>.lists()` rather than produced by
 * `qk.<entity>.list()`. Same shape, same prefix.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateGoalRequest,
  cacheRow,
  dropRow,
  type GoalDto,
  type GoalStatus,
  type ProfileDto,
  qk,
  unwrap,
  useRowAction,
} from "@/api"

interface GoalListFilters {
  /** Statuses to match, any of them; empty or absent is every status. */
  statuses?: readonly GoalStatus[]
}

/** `["goals", "list", {statuses?}]` — under `qk.goals.lists()`. */
function goalListKey({ statuses }: GoalListFilters) {
  // An empty selection is no filter at all, so it keys the same as `{}`: the
  // unfiltered board and the callers that pass nothing share one cache entry.
  return [...qk.goals.lists(), statuses?.length ? { statuses: [...statuses] } : {}] as const
}

export function goalsQueryOptions(filters: GoalListFilters = {}) {
  // `GET /v1/goals` takes the selection as one comma-separated `status`, and
  // matches a goal in any of them.
  const status = filters.statuses?.length ? filters.statuses.join(",") : undefined
  return queryOptions({
    queryKey: goalListKey(filters),
    queryFn: () => unwrap(api().GET("/v1/goals", { params: { query: { status } } })),
    // The daemon orders by id (creation order); the screen shows newest first.
    select: (goals: GoalDto[]) => [...goals].sort((a, b) => b.id.localeCompare(a.id)),
  })
}

export function goalQueryOptions(goalId: string) {
  return queryOptions({
    queryKey: qk.goals.detail(goalId),
    queryFn: () => unwrap(api().GET("/v1/goals/{id}", { params: { path: { id: goalId } } })),
  })
}

/** Planner profiles, for the planner picker on the create form. */
export function plannerProfilesQueryOptions() {
  return queryOptions({
    // `?role=planner` is the daemon's filter; the segment sits under
    // `qk.profiles.lists()` like any other.
    queryKey: [...qk.profiles.lists(), { role: "planner" }] as const,
    queryFn: () => unwrap(api().GET("/v1/profiles", { params: { query: { role: "planner" } } })),
    select: (profiles: ProfileDto[]) => [...profiles].sort((a, b) => a.name.localeCompare(b.name)),
  })
}

export function useCreateGoal() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateGoalRequest) => unwrap(api().POST("/v1/goals", { body })),
    onSuccess: (goal) => cacheRow(queryClient, qk.goals, goal),
  })
}

/**
 * Cancelling is optimistic on the goal itself: the user confirmed it, the
 * status it lands in is known, and the goal header should stop saying "active"
 * on the click. The tear-down it triggers (sessions, unfinished tasks) is not
 * guessed at — those lists are invalidated once the daemon has answered.
 */
export function useCancelGoal(goalId: string) {
  return useRowAction(
    qk.goals,
    goalId,
    () => unwrap(api().POST("/v1/goals/{id}/cancel", { params: { path: { id: goalId } } })),
    {
      optimistic: "cancelled" satisfies GoalStatus,
      alsoInvalidates: (queryClient) => {
        // Cancelling tears the goal's sessions and tasks down.
        void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
        void queryClient.invalidateQueries({ queryKey: qk.sessions.lists() })
      },
    },
  )
}

/**
 * `DELETE /v1/goals/{id}` — the goal and its tasks, in one write that nothing
 * undoes.
 *
 * Nothing here is optimistic, unlike cancelling: the daemon refuses a goal that
 * is not terminal with a 409, and a goal taken off the board before that answer
 * arrived would have to be put back. The cache work afterwards is what the
 * dispatcher does for `goal_deleted` — this goal's own entries go, and the
 * lists that name it goal-first are refetched.
 */
export function useDeleteGoal(goalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => unwrap(api().DELETE("/v1/goals/{id}", { params: { path: { id: goalId } } })),
    onSuccess: () => {
      dropRow(queryClient, qk.goals, goalId)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.all() })
      void queryClient.invalidateQueries({ queryKey: qk.sessions.all() })
    },
  })
}

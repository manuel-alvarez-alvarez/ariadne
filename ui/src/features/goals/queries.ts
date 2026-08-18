/**
 * Everything the goal screens ask the daemon for.
 *
 * Keys follow the convention in `src/api/query-keys.ts` and are built from the
 * `qk` helpers, so the SSE dispatcher keeps reaching them: it invalidates
 * `qk.goals.lists()` and `qk.goals.messages(id)` and patches
 * `qk.goals.detail(id)`, and every key below is either exactly one of those or
 * nested under it.
 *
 * Two lists here are filtered server-side (`?status=`, `?role=planner`) but the
 * shared filter types carry no `status` / `role` field, so their filter segment
 * is appended to `qk.<entity>.lists()` rather than produced by
 * `qk.<entity>.list()`. Same shape, same prefix — see the note posted on the
 * task thread about widening the shared filter types.
 *
 * The mutations invalidate what they touch even though the daemon publishes an
 * event for each of them: the stream may be down, and an action the user just
 * took is the last thing that should need a manual refresh.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CacheSnapshot,
  type CreateGoalRequest,
  type GoalDto,
  type GoalStatus,
  type MessageDto,
  optimisticStatus,
  type ProfileDto,
  qk,
  restoreCache,
  unwrap,
} from "@/api"

/** Page size for the goal thread; the daemon caps `limit` at 200. */
const MESSAGE_PAGE_SIZE = 200

/** Stop walking the thread eventually, however long it grew. */
const MAX_MESSAGE_PAGES = 20

export interface GoalListFilters {
  /** Statuses to match, any of them; empty or absent is every status. */
  statuses?: readonly GoalStatus[]
}

/** `["goals", "list", {statuses?}]` — under `qk.goals.lists()`. */
export function goalListKey({ statuses }: GoalListFilters) {
  // An empty selection is no filter at all, so it keys the same as `{}`: the
  // unfiltered board and the callers that pass nothing share one cache entry.
  return [...qk.goals.lists(), statuses?.length ? { statuses: [...statuses] } : {}] as const
}

/** `["profiles", "list", {role: "planner"}]` — under `qk.profiles.lists()`. */
export function plannerProfileListKey() {
  return [...qk.profiles.lists(), { role: "planner" }] as const
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

export function goalMessagesQueryOptions(goalId: string) {
  return queryOptions({
    queryKey: qk.goals.messages(goalId),
    queryFn: () => fetchGoalThread(goalId),
  })
}

/**
 * The whole goal thread, oldest first.
 *
 * `GET /v1/goals/{id}/messages` pages forward from the oldest message and caps
 * a page at 200, and there is no "give me the last N" — so a long thread has to
 * be walked to its end or the screen would show only its beginning.
 */
async function fetchGoalThread(goalId: string): Promise<MessageDto[]> {
  const thread: MessageDto[] = []
  let after: string | undefined
  for (let page = 0; page < MAX_MESSAGE_PAGES; page += 1) {
    const batch = await unwrap(
      api().GET("/v1/goals/{id}/messages", {
        params: { path: { id: goalId }, query: { after, limit: MESSAGE_PAGE_SIZE } },
      }),
    )
    thread.push(...batch)
    const last = batch.at(-1)
    if (batch.length < MESSAGE_PAGE_SIZE || !last) break
    after = last.id
  }
  return thread
}

/** Planner profiles, for the planner picker on the create form. */
export function plannerProfilesQueryOptions() {
  return queryOptions({
    queryKey: plannerProfileListKey(),
    queryFn: () => unwrap(api().GET("/v1/profiles", { params: { query: { role: "planner" } } })),
    select: (profiles: ProfileDto[]) => [...profiles].sort((a, b) => a.name.localeCompare(b.name)),
  })
}

/**
 * `POST /v1/goals/{id}/messages` — the thread tab's compose box.
 *
 * The daemon answers with the created message, which is appended straight to
 * the cached thread: it is the newest by construction (ids are ordered and it
 * was just minted), so the send shows up without waiting for the
 * `message_created` event to invalidate. The invalidation still runs for the
 * messages an offline stream may have missed; the append is deduped by id, so
 * event and refetch land on the same thread.
 */
export function usePostGoalMessage(goalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: string) =>
      unwrap(
        api().POST("/v1/goals/{id}/messages", {
          params: { path: { id: goalId } },
          body: { body },
        }),
      ),
    onSuccess: (message) => {
      queryClient.setQueryData<MessageDto[]>(qk.goals.messages(goalId), (thread) =>
        thread && !thread.some((existing) => existing.id === message.id)
          ? [...thread, message]
          : thread,
      )
      void queryClient.invalidateQueries({ queryKey: qk.goals.messages(goalId) })
    },
  })
}

export function useCreateGoal() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateGoalRequest) => unwrap(api().POST("/v1/goals", { body })),
    onSuccess: (goal) => {
      queryClient.setQueryData(qk.goals.detail(goal.id), goal)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
    },
  })
}

export function useFinalizeGoalPlan(goalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (summary: string) =>
      unwrap(
        api().POST("/v1/goals/{id}/finalize", {
          params: { path: { id: goalId } },
          body: { summary },
        }),
      ),
    onSuccess: (goal) => {
      queryClient.setQueryData(qk.goals.detail(goalId), goal)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      // Finalizing records a message in the thread and readies the tasks.
      void queryClient.invalidateQueries({ queryKey: qk.goals.messages(goalId) })
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
    },
  })
}

/**
 * Cancelling is optimistic on the goal itself: the user confirmed it, the
 * status it lands in is known, and the goal header should stop saying "active"
 * on the click. The tear-down it triggers (sessions, unfinished tasks) is not
 * guessed at — those lists are invalidated once the daemon has answered.
 */
export function useCancelGoal(goalId: string) {
  const queryClient = useQueryClient()
  return useMutation<GoalDto, Error, void, CacheSnapshot | undefined>({
    mutationFn: () =>
      unwrap(api().POST("/v1/goals/{id}/cancel", { params: { path: { id: goalId } } })),
    onMutate: () =>
      optimisticStatus(queryClient, {
        detailKey: qk.goals.detail(goalId),
        listsKey: qk.goals.lists(),
        id: goalId,
        status: "cancelled" satisfies GoalStatus,
      }),
    onError: (_error, _variables, snapshot) => restoreCache(queryClient, snapshot),
    onSuccess: (goal) => {
      queryClient.setQueryData(qk.goals.detail(goalId), goal)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      // Cancelling tears the goal's sessions and tasks down.
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
      void queryClient.invalidateQueries({ queryKey: qk.sessions.lists() })
    },
  })
}

/**
 * `DELETE /v1/goals/{id}` — the goal, its tasks and its messages, in one write
 * that nothing undoes.
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
      queryClient.removeQueries({ queryKey: qk.goals.detail(goalId) })
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      void queryClient.invalidateQueries({ queryKey: qk.tasks.all() })
      void queryClient.invalidateQueries({ queryKey: qk.sessions.all() })
    },
  })
}

/**
 * Every daemon call the task screens make.
 *
 * Reads are `queryOptions` so a screen and its children can share one key, and
 * writes are hooks that patch/invalidate the same keys the event dispatcher
 * uses — the stream will say the same thing a moment later, but an action
 * should not look like it did nothing until then.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateTaskRequest,
  cacheRow,
  qk,
  type TaskDto,
  type TaskStatus,
  type UpdateTaskRequest,
  unwrap,
  usePostMessage,
  useRowAction,
} from "@/api"

/** The filters `GET /v1/tasks` actually supports. */
interface TaskListFilters {
  goal?: string
  status?: TaskStatus
}

export function taskListQueryOptions(filters: TaskListFilters = {}) {
  return queryOptions({
    queryKey: qk.tasks.list(filters),
    queryFn: () =>
      unwrap(
        api().GET("/v1/tasks", {
          params: { query: { goal: filters.goal, status: filters.status } },
        }),
      ),
  })
}

export function taskQueryOptions(taskId: string) {
  return queryOptions({
    queryKey: qk.tasks.detail(taskId),
    queryFn: () => unwrap(api().GET("/v1/tasks/{id}", { params: { path: { id: taskId } } })),
  })
}

/** The cap the daemon enforces on a page of messages. */
const MESSAGE_PAGE_LIMIT = 200

export function taskMessagesQueryOptions(taskId: string) {
  return queryOptions({
    queryKey: qk.tasks.messages(taskId),
    queryFn: () =>
      unwrap(
        api().GET("/v1/tasks/{id}/messages", {
          params: { path: { id: taskId }, query: { limit: MESSAGE_PAGE_LIMIT } },
        }),
      ),
  })
}

export function taskReviewsQueryOptions(taskId: string) {
  return queryOptions({
    queryKey: qk.tasks.reviews(taskId),
    queryFn: () =>
      unwrap(api().GET("/v1/tasks/{id}/reviews", { params: { path: { id: taskId } } })),
  })
}

export function taskTransitionsQueryOptions(taskId: string) {
  return queryOptions({
    queryKey: qk.tasks.transitions(taskId),
    queryFn: () =>
      unwrap(api().GET("/v1/tasks/{id}/transitions", { params: { path: { id: taskId } } })),
  })
}

/**
 * The branch diff, as `git diff base...branch` prints it.
 *
 * `text/plain`, so the client is told not to parse it as JSON; an empty diff
 * comes back with no body at all, hence the `?? ""`. It is refetched on
 * `task_branch_updated`, the daemon's word that a commit landed on the branch,
 * and on the transition that lands the task — so it follows the app's default
 * staleness like every other read, with no refresh of its own.
 */
export function taskDiffQueryOptions(taskId: string) {
  return queryOptions({
    queryKey: qk.tasks.diff(taskId),
    queryFn: async () =>
      (await unwrap(
        api().GET("/v1/tasks/{id}/diff", {
          params: { path: { id: taskId } },
          parseAs: "text",
        }),
      )) ?? "",
  })
}

/**
 * `POST /v1/goals/{goal_id}/tasks` — the create-task form's submit.
 *
 * The daemon owns the validation (profile roles, repo membership, dep cycles,
 * `max_tasks`), so a failure surfaces as the `ApiError` the form renders. On
 * success the new task is cached and the lists refetched — the `task_created`
 * event will say the same thing, but the stream may be down.
 */
export function useCreateTask(goalId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateTaskRequest) =>
      unwrap(
        api().POST("/v1/goals/{goal_id}/tasks", {
          params: { path: { goal_id: goalId } },
          body,
        }),
      ),
    onSuccess: (task) => cacheRow(queryClient, qk.tasks, task),
  })
}

/**
 * `PATCH /v1/tasks/{id}` — the edit-task form's submit.
 *
 * Only legal while the task is pending/ready; once it has moved on the daemon
 * answers `409` and the form shows that envelope. On success the answered task
 * replaces the cached one — the `task_updated` event will say the same thing,
 * but the stream may be down. Transitions are invalidated too: adding a
 * dependency to a `ready` task moves it back to `pending`.
 */
export function useUpdateTask(taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateTaskRequest) =>
      unwrap(api().PATCH("/v1/tasks/{id}", { params: { path: { id: taskId } }, body })),
    onSuccess: (task) => {
      cacheRow(queryClient, qk.tasks, task)
      // Adding a dependency to a `ready` task moves it back to `pending`.
      void queryClient.invalidateQueries({ queryKey: qk.tasks.transitions(taskId) })
    },
  })
}

/** `POST /v1/tasks/{id}/messages` — the conversation tab's compose box, the
 * web's `ariadne task msg`. */
export function usePostTaskMessage(taskId: string) {
  return usePostMessage(qk.tasks.messages(taskId), (body) =>
    unwrap(api().POST("/v1/tasks/{id}/messages", { params: { path: { id: taskId } }, body })),
  )
}

/**
 * Cancel is optimistic: it is a confirmed, terminal state flip, so the card
 * moves to `cancelled` on the click and only comes back if the daemon refuses
 * (it does, with a `409`, when the task already left a cancellable state).
 */
export function useCancelTask(taskId: string) {
  return useTaskAction(
    taskId,
    () => unwrap(api().POST("/v1/tasks/{id}/cancel", { params: { path: { id: taskId } } })),
    "cancelled",
  )
}

/**
 * Retry is not optimistic. The daemon does not just flip a status: it schedules
 * a fresh engineer session, and the task's next status is its answer, not ours.
 */
export function useRetryTask(taskId: string) {
  return useTaskAction(taskId, () =>
    unwrap(api().POST("/v1/tasks/{id}/retry", { params: { path: { id: taskId } } })),
  )
}

/** Both user actions answer with the updated task and move it between columns. */
function useTaskAction(
  taskId: string,
  mutationFn: () => Promise<TaskDto>,
  /** The status the task lands in, when the client can know it in advance. */
  optimistic?: TaskStatus,
) {
  return useRowAction(qk.tasks, taskId, mutationFn, {
    optimistic,
    // A cancel or a retry is a transition, and the history tab shows it.
    alsoInvalidates: (queryClient) =>
      void queryClient.invalidateQueries({ queryKey: qk.tasks.transitions(taskId) }),
  })
}

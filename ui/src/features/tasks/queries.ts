/**
 * Every daemon call the task screens make.
 *
 * Reads are `queryOptions` so a screen and its children can share one key, and
 * writes are hooks that patch/invalidate the same keys the event dispatcher
 * uses — the stream will say the same thing a moment later, but an action
 * should not look like it did nothing until then.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, qk, type TaskDto, type TaskStatus, unwrap } from "@/api"

/** The filters `GET /v1/tasks` actually supports. */
export interface TaskListFilters {
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
 * comes back with no body at all, hence the `?? ""`. No event invalidates this
 * — commits in the worktree are not daemon state — so the view offers a
 * refresh instead of pretending to be live.
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
    staleTime: 0,
  })
}

export function usePostTaskMessage(taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: string) =>
      unwrap(
        api().POST("/v1/tasks/{id}/messages", {
          params: { path: { id: taskId } },
          body: { body },
        }),
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.tasks.messages(taskId) })
    },
  })
}

export function useCancelTask(taskId: string) {
  return useTaskAction(taskId, () =>
    unwrap(api().POST("/v1/tasks/{id}/cancel", { params: { path: { id: taskId } } })),
  )
}

export function useRetryTask(taskId: string) {
  return useTaskAction(taskId, () =>
    unwrap(api().POST("/v1/tasks/{id}/retry", { params: { path: { id: taskId } } })),
  )
}

/** Both user actions answer with the updated task and move it between columns. */
function useTaskAction(taskId: string, mutationFn: () => Promise<TaskDto>) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn,
    onSuccess: (task) => {
      queryClient.setQueryData(qk.tasks.detail(taskId), task)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
      void queryClient.invalidateQueries({ queryKey: qk.tasks.transitions(taskId) })
    },
  })
}

/**
 * The goal's own tasks, inside its panel.
 *
 * They are on the board too — but the board is behind the panel that covers
 * it, so a goal that is open has to be able to show what it is made of. The
 * cards are the board's own {@link TaskCard}, which links a task into the
 * panel that opens on top of this one, so opening one from here is the same
 * gesture as opening it from a lane.
 *
 * The list is one query with the rest of the tasks screens
 * (`GET /v1/tasks?goal=`), which the SSE dispatcher invalidates on
 * `task_created` / `task_updated`, so a status changes here on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Skeleton } from "@/components/ui/skeleton"
import { compareByAttention, TaskCard, taskListQueryOptions } from "@/features/tasks"

export function GoalTasks({ goalId }: { goalId: string }) {
  const tasks = useQuery(taskListQueryOptions({ goal: goalId }))
  // What needs a person comes first: the panel is a small window, and the task
  // that is stuck should not be the one below the fold.
  const ordered = useMemo(() => [...(tasks.data ?? [])].sort(compareByAttention), [tasks.data])

  if (tasks.error) {
    return (
      <ErrorState
        showIcon
        title="Could not load the tasks"
        error={tasks.error}
        onRetry={() => void tasks.refetch()}
      />
    )
  }

  if (tasks.isPending) {
    return (
      <div className="flex flex-col gap-2">
        {[0, 1, 2].map((row) => (
          <Skeleton key={row} className="h-14 w-full" />
        ))}
      </div>
    )
  }

  if (ordered.length === 0) {
    return <EmptyState emphasis="quiet" title="No tasks yet." />
  }

  return (
    <ul className="flex flex-col gap-2">
      {ordered.map((task) => (
        <li key={task.id}>
          <TaskCard task={task} showStatus />
        </li>
      ))}
    </ul>
  )
}

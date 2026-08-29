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
import { PlusIcon } from "lucide-react"
import { useMemo } from "react"

import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { compareByAttention, TaskCard, taskListQueryOptions } from "@/features/tasks"

import { useBoardAttention } from "./attention"

export function GoalTasks({
  goalId,
  onNewTask,
}: {
  goalId: string
  /** Opens the create-task dialog; absent when the goal no longer takes one. */
  onNewTask?: () => void
}) {
  const tasks = useQuery(taskListQueryOptions({ goal: goalId }))
  // The board's own badges, from the board's own sessions query: a card in the
  // panel says the same thing about a blocked agent as the card in the lane
  // behind it.
  const attention = useBoardAttention()
  // What needs a person comes first: the panel is a small window, and the task
  // that is stuck should not be the one below the fold.
  const ordered = useMemo(() => [...(tasks.data ?? [])].sort(compareByAttention), [tasks.data])

  if (tasks.error) {
    return (
      <ErrorState
        showIcon
        title="Could not load tasks"
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
    return (
      <EmptyState
        emphasis="quiet"
        title="No tasks yet"
        action={
          onNewTask ? (
            <Button variant="outline" size="sm" onClick={onNewTask}>
              <PlusIcon />
              New task
            </Button>
          ) : undefined
        }
      />
    )
  }

  return (
    <ul className="flex flex-col gap-2">
      {ordered.map((task) => (
        <li key={task.id}>
          <TaskCard task={task} showStatus attention={attention.byTask.get(task.id)} />
        </li>
      ))}
    </ul>
  )
}

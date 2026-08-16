/**
 * `ariadne goal ls` as a board: every goal, newest first, filtered server-side
 * by status — each one a horizontal swimlane of its tasks under shared
 * pipeline columns.
 *
 * Nothing here polls. The lanes come out of the query cache, which the single
 * SSE connection patches and invalidates, so a goal created or cancelled
 * anywhere — this window, another one, the CLI, the daemon itself — shows up
 * without a refresh.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useState } from "react"

import type { GoalStatus } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { CreateGoalDialog } from "./create-goal-dialog"
import { GoalSwimlanes } from "./goal-swimlanes"
import { goalsQueryOptions } from "./queries"
import { GOAL_STATUS_META, GOAL_STATUSES } from "./status"

/** Sentinel for "no status filter" — `Select` needs a value for every item. */
const ALL = "all"

/** `items` is what makes the trigger show the label rather than the raw value. */
const STATUS_ITEMS = [
  { label: "All statuses", value: ALL },
  ...GOAL_STATUSES.map((status) => ({ label: GOAL_STATUS_META[status].label, value: status })),
]

export function GoalsListPage() {
  const [status, setStatus] = useState<GoalStatus | typeof ALL>(ALL)
  const [createOpen, setCreateOpen] = useState(false)

  const goals = useQuery(goalsQueryOptions(status === ALL ? {} : { status }))

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h1 className="font-heading text-lg font-semibold">Goals</h1>
          <p className="text-sm text-muted-foreground">
            What Ariadne is working on, and what it has finished.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Select
            value={status}
            onValueChange={(value) => setStatus((value as GoalStatus | typeof ALL) ?? ALL)}
            items={STATUS_ITEMS}
          >
            <SelectTrigger aria-label="Filter by status" className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STATUS_ITEMS.map((item) => (
                <SelectItem key={item.value} value={item.value}>
                  {item.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button onClick={() => setCreateOpen(true)}>
            <PlusIcon />
            New goal
          </Button>
        </div>
      </div>

      {goals.error ? (
        <ErrorState
          showIcon
          title="Could not load goals"
          error={goals.error}
          onRetry={() => void goals.refetch()}
        />
      ) : null}

      {goals.isPending ? <GoalsSkeleton /> : null}

      {goals.data?.length === 0 ? (
        <EmptyState
          title={
            status === ALL
              ? "No goals yet"
              : `No ${GOAL_STATUS_META[status].label.toLowerCase()} goals`
          }
          description={
            status === ALL
              ? "Create one and the planner will break it into tasks."
              : "Try another status filter."
          }
        />
      ) : null}

      {goals.data?.length ? <GoalSwimlanes goals={goals.data} /> : null}

      <CreateGoalDialog open={createOpen} onOpenChange={setCreateOpen} />
    </div>
  )
}

function GoalsSkeleton() {
  return (
    <div className="flex flex-col gap-2 rounded-lg border p-4">
      {[0, 1, 2].map((row) => (
        <Skeleton key={row} className="h-10 w-full" />
      ))}
    </div>
  )
}

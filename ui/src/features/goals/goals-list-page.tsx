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
import { AlertCircleIcon, PlusIcon } from "lucide-react"
import { useState } from "react"

import { ApiError, type GoalStatus } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import { GOAL_STATUSES } from "./goal-status-badge"
import { GoalSwimlanes } from "./goal-swimlanes"
import { goalsQueryOptions } from "./queries"

/** Sentinel for "no status filter" — `Select` needs a value for every item. */
const ALL = "all"

/** `items` is what makes the trigger show the label rather than the raw value. */
const STATUS_ITEMS = [
  { label: "All statuses", value: ALL },
  ...GOAL_STATUSES.map((status) => ({ label: status, value: status })),
]

export function GoalsListPage() {
  const [status, setStatus] = useState<GoalStatus | typeof ALL>(ALL)
  const [createOpen, setCreateOpen] = useState(false)

  const goals = useQuery(goalsQueryOptions(status === ALL ? {} : { status }))
  const error = ApiError.is(goals.error) ? goals.error : null

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

      {error ? (
        <Alert variant="destructive">
          <AlertCircleIcon />
          <AlertTitle>Could not load goals</AlertTitle>
          <AlertDescription>{error.message}</AlertDescription>
          <AlertAction>
            <Button variant="outline" size="sm" onClick={() => void goals.refetch()}>
              Retry
            </Button>
          </AlertAction>
        </Alert>
      ) : null}

      {goals.isPending ? <GoalsSkeleton /> : null}

      {goals.data?.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center">
          <p className="text-sm font-medium">
            {status === ALL ? "No goals yet" : `No ${status} goals`}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            {status === ALL
              ? "Create one and the planner will break it into tasks."
              : "Try another status filter."}
          </p>
        </div>
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

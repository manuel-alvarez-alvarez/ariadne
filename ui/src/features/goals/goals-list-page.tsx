/**
 * `ariadne goal ls` as a board: every goal, newest first, filtered server-side
 * by status — each one a horizontal swimlane of its tasks under shared
 * pipeline columns, under a strip of everything that is stuck.
 *
 * Nothing here polls. The lanes come out of the query cache, which the single
 * SSE connection patches and invalidates, so a goal created or cancelled
 * anywhere — this window, another one, the CLI, the daemon itself — shows up
 * without a refresh.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon, TargetIcon } from "lucide-react"
import { useState } from "react"
import { useSearchParams } from "react-router-dom"

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
import { AttentionStrip } from "./attention-strip"
import { CreateGoalDialog } from "./create-goal-dialog"
import { ALL, readStatusFilter, type StatusFilter, withStatusFilter } from "./filters"
import { BoardSkeleton, GoalSwimlanes } from "./goal-swimlanes"
import { goalsQueryOptions } from "./queries"
import { GOAL_STATUS_META, GOAL_STATUSES } from "./status"

/** `items` is what makes the trigger show the label rather than the raw value. */
const STATUS_ITEMS = [
  { label: "All statuses", value: ALL },
  ...GOAL_STATUSES.map((status) => ({ label: GOAL_STATUS_META[status].label, value: status })),
]

export function GoalsListPage() {
  const [search, setSearch] = useSearchParams()
  const status = readStatusFilter(search)
  const [createOpen, setCreateOpen] = useState(false)

  const goals = useQuery(goalsQueryOptions(status === ALL ? {} : { status }))

  /** A filter is not a place: it replaces the entry rather than piling up back steps. */
  function filterBy(value: StatusFilter) {
    setSearch(withStatusFilter(search, value), { replace: true })
  }

  /** The goal just created opens its own panel — no hunting for it on the board. */
  function openGoal(goalId: string) {
    const next = new URLSearchParams(search)
    next.set("goal", goalId)
    setSearch(next)
  }

  // The board owns the scrolling (its headers stick to it), so the screen is a
  // fixed-height column rather than a page that grows.
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
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
            onValueChange={(value) => filterBy((value as StatusFilter) ?? ALL)}
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

      {/* Above the lanes and outside the filter: what is stuck stays in sight
          whatever the board is narrowed to. */}
      <AttentionStrip />

      {goals.error ? (
        <ErrorState
          showIcon
          title="Could not load goals"
          error={goals.error}
          onRetry={() => void goals.refetch()}
        />
      ) : null}

      {goals.isPending ? <BoardSkeleton /> : null}

      {goals.data?.length === 0 ? (
        <EmptyState
          icon={<TargetIcon className="size-5" />}
          title={
            status === ALL
              ? "No goals yet"
              : `No ${GOAL_STATUS_META[status].label.toLowerCase()} goals`
          }
          description={
            status === ALL
              ? "A goal is what Ariadne works on: describe one and the planner breaks it into tasks."
              : "Nothing at this status. Try another filter, or start something new."
          }
          // The board is the screen the user is here to fill, so its empty
          // state carries the way to fill it rather than only naming the gap.
          action={
            <Button variant="outline" size="sm" onClick={() => setCreateOpen(true)}>
              <PlusIcon />
              New goal
            </Button>
          }
        />
      ) : null}

      {goals.data?.length ? <GoalSwimlanes goals={goals.data} /> : null}

      <CreateGoalDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(goal) => openGoal(goal.id)}
      />
    </div>
  )
}

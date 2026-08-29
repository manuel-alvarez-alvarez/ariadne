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
import { ChevronDownIcon, PlusIcon, TargetIcon } from "lucide-react"
import { useState } from "react"
import { useSearchParams } from "react-router-dom"

import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { AttentionStrip } from "./attention-strip"
import { CreateGoalDialog } from "./create-goal-dialog"
import {
  NO_STATUS_FILTER,
  type StatusFilter,
  showsFinished,
  toggleStatusFilter,
  withFinished,
} from "./filters"
import { BoardSkeleton, GoalSwimlanes } from "./goal-swimlanes"
import { goalsQueryOptions } from "./queries"
import { GOAL_STATUS_META, GOAL_STATUSES } from "./status"
import { useStatusFilter } from "./use-status-filter"

/** What the trigger says: the one status by name, several by count. */
function summarize(filter: StatusFilter): string {
  const [only] = filter
  if (!only) return "All statuses"
  if (filter.length === 1) return GOAL_STATUS_META[only].label
  return `${filter.length} statuses`
}

/** The board came back empty: whether that is a filter or an empty Ariadne. */
function emptyBoardCopy(filter: StatusFilter): { title: string; description: string } {
  const [only] = filter
  if (!only) {
    return {
      title: "No goals yet",
      description:
        "A goal is what Ariadne works on: describe one and the planner breaks it into tasks.",
    }
  }
  return {
    title:
      filter.length === 1
        ? `No ${GOAL_STATUS_META[only].label.toLowerCase()} goals`
        : "No goals match this filter",
    description: `Nothing at ${filter.length === 1 ? "this status" : "these statuses"}. Try another filter, or start something new.`,
  }
}

export function GoalsListPage() {
  const [search, setSearch] = useSearchParams()
  const { statuses, filterBy } = useStatusFilter()
  const [createOpen, setCreateOpen] = useState(false)

  const goals = useQuery(goalsQueryOptions({ statuses }))
  const empty = emptyBoardCopy(statuses)
  const finished = showsFinished(statuses)

  /** The goal just created opens its own panel — no hunting for it on the board. */
  function openGoal(goalId: string) {
    const next = new URLSearchParams(search)
    next.set("goal", goalId)
    setSearch(next)
  }

  // The board owns the scrolling (its headers stick to it), so the screen is a
  // fixed-height column rather than a page that grows: `h-full` against the
  // shell's `<main>`, which is itself a `flex-1 min-h-0` item, and both engines
  // resolve it — measured in the Tauri window, the board comes out exactly as
  // tall as what is left. Keeping `<main>` from scrolling behind it is the
  // board's own job, on `BOARD_BOX`: what a scrollport hides is still overflow
  // for the scroller above it unless the scrollport says otherwise.
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Goals"
        description="What Ariadne is working on, and what it has finished."
        actions={
          <>
            {/* The one narrowing anybody makes by hand, as one click rather
                than two checkboxes: the board opens on the work that is still
                moving (see `DEFAULT_GOAL_STATUS_FILTER`), and this is how the
                rest of it comes back. The menu beside it still spells any
                selection. */}
            <Button
              variant={finished ? "secondary" : "outline"}
              aria-pressed={finished}
              className="font-normal"
              onClick={() => filterBy(withFinished(statuses, !finished))}
            >
              Show finished
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by status"
                    className="w-40 justify-between font-normal"
                  />
                }
              >
                {summarize(statuses)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              {/* Checkbox items stay open on a click, which is what a filter
                built out of several of them needs. */}
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuCheckboxItem
                  checked={statuses.length === 0}
                  onCheckedChange={() => filterBy(NO_STATUS_FILTER)}
                >
                  All statuses
                </DropdownMenuCheckboxItem>
                <DropdownMenuSeparator />
                {GOAL_STATUSES.map((status) => (
                  <DropdownMenuCheckboxItem
                    key={status}
                    checked={statuses.includes(status)}
                    onCheckedChange={() => filterBy(toggleStatusFilter(statuses, status))}
                  >
                    {GOAL_STATUS_META[status].label}
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <Button onClick={() => setCreateOpen(true)}>
              <PlusIcon />
              New goal
            </Button>
          </>
        }
      />

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
          title={empty.title}
          description={empty.description}
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

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

import { FilterIcon, PlusIcon, SearchIcon, TargetIcon } from "lucide-react"
import { useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"

import { LOCAL_HOST, useHostLists } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { HostListErrors } from "@/components/error-state"
import { HostCoverage } from "@/components/host-coverage"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { useHostSelection } from "@/hooks/use-host-filter"
import { useRowsShowHost } from "@/hooks/use-host-label"
import { HOST_PARAM } from "@/routes/paths"
import { CreateGoalDialog } from "./create-goal-dialog"
import {
  DEFAULT_GOAL_STATUS_FILTER,
  NO_STATUS_FILTER,
  readStatusFilter,
  type StatusFilter,
  searchGoals,
  serializeStatusFilter,
  toggleStatusFilter,
} from "./filters"
import { BoardSkeleton, GoalSwimlanes } from "./goal-swimlanes"
import { goalsQueryOptions } from "./queries"
import { SavedViews } from "./saved-views"
import { GOAL_STATUS_META, GOAL_STATUSES, isTerminalGoalStatus } from "./status"
import { useStatusFilter } from "./use-status-filter"

const ACTIVE_FILTER = readStatusFilter(new URLSearchParams({ status: DEFAULT_GOAL_STATUS_FILTER }))
const FINISHED_FILTER = GOAL_STATUSES.filter(isTerminalGoalStatus)

function sameFilter(a: StatusFilter, b: StatusFilter): boolean {
  return serializeStatusFilter(a) === serializeStatusFilter(b)
}

/** The board came back empty: whether that is a filter or an empty Ariadne. */
function emptyBoardCopy(filter: StatusFilter): { title: string; description: string } {
  const [only] = filter
  if (!only) {
    return {
      title: "No goals yet",
      description:
        "A goal is what Ariadne works on: describe one and the orchestrator breaks it into tasks.",
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
  const query = search.get("q") ?? ""
  const repository = search.get("repo") ?? ""
  const compact = search.get("presentation") === "compact"
  function changeView(key: string, value: string) {
    const next = new URLSearchParams(search)
    next.set(key, value)
    setSearch(next, { replace: true })
  }

  // Every host the header's picker selected, merged newest first: ids are
  // ULIDs, so their order is their creation order whichever daemon minted them.
  const selection = useHostSelection()
  const hosts = selection.read
  const goals = useHostLists(hosts, (host) => goalsQueryOptions(host, { statuses }))
  const merged = useMemo(
    () => [...goals.rows].sort((a, b) => b.id.localeCompare(a.id) || a.host.localeCompare(b.host)),
    [goals.rows],
  )
  const shown = useMemo(
    () =>
      searchGoals(merged, query).filter(
        (goal) => !repository || goal.repos.some((repo) => repo.path === repository),
      ),
    [merged, query, repository],
  )
  const showHost = useRowsShowHost()
  const failedEverywhere = goals.errors.length > 0 && goals.errors.length === hosts.length
  const empty = emptyBoardCopy(statuses)
  const active = sameFilter(statuses, ACTIVE_FILTER)
  const all = statuses.length === 0
  const finished = sameFilter(statuses, FINISHED_FILTER)
  const custom = !active && !all && !finished

  /** The goal just created opens its own panel, on its host — no hunting for it on the board. */
  function openGoal(goalId: string, host: string) {
    const next = new URLSearchParams(search)
    next.set("goal", goalId)
    if (host === LOCAL_HOST) next.delete(HOST_PARAM)
    else next.set(HOST_PARAM, host)
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
            <div className="relative">
              <SearchIcon className="pointer-events-none absolute top-2.5 left-2 size-4 text-muted-foreground" />
              <Input
                aria-label="Search goals by title"
                placeholder="Search titles"
                className="w-44 pl-8"
                value={query}
                onChange={(event) => changeView("q", event.target.value)}
              />
            </div>
            <ButtonGroup aria-label="Goal status">
              <Button
                variant={active ? "secondary" : "outline"}
                aria-pressed={active}
                className="font-normal"
                onClick={() => filterBy(ACTIVE_FILTER)}
              >
                Active
              </Button>
              <Button
                variant={all ? "secondary" : "outline"}
                aria-pressed={all}
                className="font-normal"
                onClick={() => filterBy(NO_STATUS_FILTER)}
              >
                All
              </Button>
              <Button
                variant={finished ? "secondary" : "outline"}
                aria-pressed={finished}
                className="font-normal"
                onClick={() => filterBy(FINISHED_FILTER)}
              >
                Finished
              </Button>
              <DropdownMenu>
                <DropdownMenuTrigger
                  render={
                    <Button
                      variant={custom ? "secondary" : "outline"}
                      size="icon"
                      aria-label="Filter by status"
                      aria-pressed={custom}
                    />
                  }
                >
                  <FilterIcon />
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
            </ButtonGroup>
            <Button onClick={() => setCreateOpen(true)}>
              <PlusIcon />
              New goal
            </Button>
          </>
        }
      />

      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-sm">
          Repository
          <select
            aria-label="Repository scope"
            className="h-9 max-w-64 rounded-md border bg-background px-2"
            value={repository}
            onChange={(event) => changeView("repo", event.target.value)}
          >
            <option value="">All repositories</option>
            {[
              ...new Set([
                ...merged.flatMap((goal) => goal.repos.map((repo) => repo.path)),
                ...(repository ? [repository] : []),
              ]),
            ]
              .sort()
              .map((path) => (
                <option key={path}>{path}</option>
              ))}
          </select>
        </label>
        <ButtonGroup aria-label="Goal presentation">
          <Button
            variant={!compact ? "secondary" : "outline"}
            aria-pressed={!compact}
            onClick={() => changeView("presentation", "board")}
          >
            Board
          </Button>
          <Button
            variant={compact ? "secondary" : "outline"}
            aria-pressed={compact}
            onClick={() => changeView("presentation", "compact")}
          >
            Compact
          </Button>
        </ButtonGroup>
        <SavedViews search={search} apply={(next) => setSearch(next, { replace: true })} />
      </div>

      <HostListErrors
        what="goals"
        errors={goals.errors}
        hostCount={hosts.length}
        hasRows={merged.length > 0}
        onRetry={goals.refetch}
        showIcon
      />

      {selection.listed.length > 1 ? (
        <HostCoverage absent={selection.absent} loading={goals.pending} hostCount={hosts.length} />
      ) : null}

      {goals.isPending ? <BoardSkeleton /> : null}

      {merged.length && shown.length === 0 ? (
        <EmptyState
          icon={<TargetIcon className="size-5" />}
          title="No goals match"
          description="Nothing matches this search. Clear it to see more."
        />
      ) : null}

      {!goals.isPending &&
      goals.pending.length === 0 &&
      !failedEverywhere &&
      merged.length === 0 ? (
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

      {shown.length ? (
        <GoalSwimlanes
          hosts={hosts}
          goals={shown}
          activeOnly={active}
          showHost={showHost}
          compact={compact}
        />
      ) : null}

      <CreateGoalDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(goal) => openGoal(goal.id, goal.host)}
      />
    </div>
  )
}

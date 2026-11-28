/**
 * `ariadne goal ls` as a board: every goal, filtered server-side by status, as
 * one list of task rows — the rows of one goal side by side, and each task
 * with no goal a row of its own.
 *
 * Nothing here polls. The rows come out of the query cache, which the single
 * SSE connection patches and invalidates, so a goal created or cancelled
 * anywhere — this window, another one, the CLI, the daemon itself — shows up
 * without a refresh.
 */

import {
  ChevronDownIcon,
  FilterIcon,
  ListPlusIcon,
  PlusIcon,
  SearchIcon,
  TargetIcon,
} from "lucide-react"
import { useMemo, useState } from "react"
import { useLocation, useNavigate, useSearchParams } from "react-router-dom"

import { LOCAL_HOST, useHostLists } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { HostListErrors } from "@/components/error-state"
import { HostCoverage } from "@/components/host-coverage"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { CreateStandaloneTaskDialog } from "@/features/tasks/create-standalone-task-dialog"
import { NEW_GOAL_SHORTCUT } from "@/hooks/use-global-shortcuts"
import { useHostSelection } from "@/hooks/use-host-filter"
import { useRowsShowHost } from "@/hooks/use-host-label"
import { keySequenceLabel } from "@/lib/shortcuts"
import { TAB_ICONS } from "@/lib/tab-icons"
import { HOST_PARAM, taskPanelFrom } from "@/routes/paths"
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
import { BoardSkeleton, GoalBoard } from "./goal-board"
import { goalsQueryOptions } from "./queries"
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
  const [createTaskOpen, setCreateTaskOpen] = useState(false)
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const query = search.get("q") ?? ""
  function setQuery(value: string) {
    const next = new URLSearchParams(search)
    if (value) next.set("q", value)
    else next.delete("q")
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
  const shown = useMemo(() => searchGoals(merged, query), [merged, query])
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
                onChange={(event) => setQuery(event.target.value)}
              />
            </div>
            <div className="flex items-center gap-1">
              <Tabs
                value={custom ? "custom" : active ? "active" : all ? "all" : "finished"}
                onValueChange={(value) =>
                  filterBy(
                    value === "active"
                      ? ACTIVE_FILTER
                      : value === "all"
                        ? NO_STATUS_FILTER
                        : FINISHED_FILTER,
                  )
                }
              >
                <TabsList aria-label="Goal status">
                  <TabsTrigger value="active" icon={<TAB_ICONS.active />} label="Active" />
                  <TabsTrigger value="all" icon={<TAB_ICONS.all />} label="All" />
                  <TabsTrigger value="finished" icon={<TAB_ICONS.finished />} label="Finished" />
                </TabsList>
              </Tabs>
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
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button />}>
                <PlusIcon />
                New
                <ChevronDownIcon />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-44">
                <DropdownMenuItem
                  aria-keyshortcuts={NEW_GOAL_SHORTCUT.key}
                  onClick={() => setCreateOpen(true)}
                >
                  <TargetIcon />
                  Goal
                  <kbd aria-hidden className="ml-auto font-sans text-xs text-muted-foreground">
                    {keySequenceLabel(NEW_GOAL_SHORTCUT)}
                  </kbd>
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => setCreateTaskOpen(true)}>
                  <ListPlusIcon />
                  Task
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

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
          description="No goal matches this search. Clear it to see more."
        />
      ) : null}

      {shown.length || !goals.isPending ? (
        <GoalBoard
          hosts={hosts}
          goals={shown}
          activeOnly={active}
          query={query}
          showHost={showHost}
          // With no goal and no task, the board is the empty state alone. The
          // screen is the one the user is here to fill, so the empty state
          // carries the way to fill it rather than only naming the gap.
          empty={
            goals.pending.length === 0 && !failedEverywhere && merged.length === 0 ? (
              <EmptyState
                icon={<TargetIcon className="size-5" />}
                title={empty.title}
                description={empty.description}
                action={
                  <Button variant="outline" size="sm" onClick={() => setCreateOpen(true)}>
                    <PlusIcon />
                    New goal
                  </Button>
                }
              />
            ) : undefined
          }
        />
      ) : null}

      <CreateStandaloneTaskDialog
        open={createTaskOpen}
        onOpenChange={setCreateTaskOpen}
        onCreated={(task) => {
          const target = taskPanelFrom(pathname, search, task.id)
          void navigate(target, { replace: target.replace })
        }}
      />

      <CreateGoalDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(goal) => openGoal(goal.id, goal.host)}
      />
    </div>
  )
}

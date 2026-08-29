/**
 * The sessions screen: every agent Ariadne has run, whatever it was run for.
 *
 * The panels answer "what is this task's engineer doing"; this screen answers
 * the question no panel can — "what is running right now", and "which agent
 * failed while I was away". It is the one place {@link SessionsList} is mounted
 * unscoped, which is what turns its Context column on: with no goal and no task
 * around the table, the row has to say which piece of work it belongs to.
 *
 * A row opens the session's own panel (`?session=`) over this screen rather
 * than navigating anywhere, so the list — and the filters that produced it —
 * stays behind it with the picked row still marked.
 *
 * The filters live in the URL, next to `?session=`, and are remembered between
 * visits: see `filters.ts` and `use-session-filters.ts`, which are the goals
 * board's pair of the same name. Two of them are dropdowns — a status and a
 * role — and two are chips: `?goal=` and `?task=`, the daemon's own list
 * filters, which this screen claims from the panel scheme that owns those
 * params everywhere else (see `components/detail-panels.tsx`).
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, XIcon } from "lucide-react"
import { useNavigate, useSearchParams } from "react-router-dom"

import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { goalsQueryOptions } from "@/features/goals/queries"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { ROLE_LABELS, shortId } from "@/lib/format"
import { paths, sessionPanelFrom } from "@/routes/paths"

import {
  ALL,
  ATTENTION,
  type FilterParam,
  GOAL_PARAM,
  LIVE,
  ROLE_PARAM,
  ROLES,
  roleLabel,
  STATUS_PARAM,
  STATUSES,
  statusFilters,
  statusLabel,
  TASK_PARAM,
} from "./filters"
import type { SessionListFilters } from "./queries"
import { SESSION_STATUS_META } from "./session-display"
import { SessionsList } from "./sessions-list"
import { useSessionFilters } from "./use-session-filters"

export function SessionsPage() {
  const [search] = useSearchParams()
  const navigate = useNavigate()
  const { status, role, goal, task, filterBy } = useSessionFilters()

  const filters: SessionListFilters = {
    ...statusFilters(status),
    role: role ?? undefined,
    goal: goal ?? undefined,
    task: task ?? undefined,
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Sessions"
        description="Every agent Ariadne has run, and the goal or task it was run for. Pick one to watch its terminal."
        actions={
          <>
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
                {statusLabel(status)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuRadioGroup
                  value={status ?? ALL}
                  onValueChange={(value) => filterBy(STATUS_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All statuses</DropdownMenuRadioItem>
                  {/* Above the individual statuses rather than among them:
                      neither is a status — one is three of them at once, the
                      other a cut across all of them — and between them they
                      are what this screen is usually opened for. */}
                  <DropdownMenuRadioItem value={LIVE}>Live</DropdownMenuRadioItem>
                  <DropdownMenuRadioItem value={ATTENTION}>Needs attention</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {STATUSES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {SESSION_STATUS_META[known].label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>

            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by role"
                    className="w-36 justify-between font-normal"
                  />
                }
              >
                {roleLabel(role)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-36">
                <DropdownMenuRadioGroup
                  value={role ?? ALL}
                  onValueChange={(value) => filterBy(ROLE_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All roles</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {ROLES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {ROLE_LABELS[known]}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <ScopeChips goal={goal} task={task} onClear={filterBy} />

      <SessionsList
        filters={filters}
        // The screen is not *inside* the goal or task it is narrowed to — the
        // chip above says which one it is — so the rows keep saying what work
        // they belong to, and an empty list blames the filters that emptied it.
        inside={false}
        onSelect={(session) =>
          void navigate(sessionPanelFrom(paths.sessions(), search, session.id))
        }
      />
    </div>
  )
}

/**
 * What the screen is narrowed to, and the way out of it.
 *
 * A goal or a task is a filter with no menu behind it — there is no list of
 * every goal worth putting in a dropdown, and a scope arrives as a link from
 * the work itself (`#/sessions?goal=<id>`) or as one deep link kept between
 * visits. So it shows as a chip: the thing's own name where the app knows it,
 * its short id where it does not, and one click to drop it.
 */
function ScopeChips({
  goal,
  task,
  onClear,
}: {
  goal: string | null
  task: string | null
  onClear: (param: FilterParam, value: string) => void
}) {
  // The same two lists the table underneath reads for its Context column, so
  // naming the chip costs no request of its own.
  const goals = useQuery({ ...goalsQueryOptions(), enabled: goal !== null })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: task !== null })

  if (!goal && !task) return null
  return (
    <div className="flex flex-wrap items-center gap-2">
      {goal ? (
        <ScopeChip
          what="Goal"
          name={goals.data?.find((one) => one.id === goal)?.title ?? shortId(goal)}
          onClear={() => onClear(GOAL_PARAM, ALL)}
        />
      ) : null}
      {task ? (
        <ScopeChip
          what="Task"
          name={tasks.data?.find((one) => one.id === task)?.title ?? shortId(task)}
          onClear={() => onClear(TASK_PARAM, ALL)}
        />
      ) : null}
    </div>
  )
}

function ScopeChip({
  what,
  name,
  onClear,
}: {
  what: "Goal" | "Task"
  name: string
  onClear: () => void
}) {
  return (
    <Badge variant="outline" className="max-w-80 gap-1.5 pr-1 pl-2.5">
      <span className="text-muted-foreground">{what}</span>
      <span className="min-w-0 truncate" title={name}>
        {name}
      </span>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={`Show sessions for every ${what.toLowerCase()}`}
        onClick={onClear}
        className="size-4 rounded-full text-muted-foreground hover:text-foreground"
      >
        <XIcon />
      </Button>
    </Badge>
  )
}

/**
 * Every agent session the daemon knows about — the screen equivalent of
 * `ariadne session ls --all`.
 *
 * The screen is the filter bar; the table itself is {@link SessionsList},
 * which the goal and task panels embed too. Picking a row here opens the
 * session's own screen.
 *
 * Filters live in the URL so coming back from a session keeps the view you
 * left, and so a filtered list can be reloaded as-is.
 */

import { useQuery } from "@tanstack/react-query"
import { useNavigate, useSearchParams } from "react-router-dom"

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { paths } from "@/routes/paths"

import {
  goalsQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
  tasksQueryOptions,
} from "./queries"
import { SESSION_STATUSES } from "./session-display"
import { SessionsList } from "./sessions-list"

/** Select value standing for "no filter"; an empty value is not selectable. */
const ANY = "any"

/** A selection as a filter: the "any" sentinel and no selection both clear it. */
function filterValue(value: string | null): string | undefined {
  return value === null || value === ANY ? undefined : value
}

export function SessionsListPage() {
  const navigate = useNavigate()
  const [params, setParams] = useSearchParams()

  const goal = params.get("goal") ?? undefined
  const task = params.get("task") ?? undefined
  // Straight from the URL, so it is only a status if the daemon has one by
  // that name — anything else would come back as a 400.
  const statusParam = params.get("status")
  const status = SESSION_STATUSES.find((value) => value === statusParam)
  const filters: SessionListFilters = { goal, task, status }

  // The same query the list runs, for the count next to the filters.
  const sessions = useQuery(sessionsQueryOptions(filters))
  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(tasksQueryOptions(goal))

  function setFilter(next: Partial<Record<"goal" | "task" | "status", string | undefined>>) {
    const updated = new URLSearchParams(params)
    for (const [key, value] of Object.entries(next)) {
      if (value === undefined) updated.delete(key)
      else updated.set(key, value)
    }
    setParams(updated, { replace: true })
  }

  return (
    <div className="space-y-4">
      <header className="space-y-1">
        <h1 className="font-heading text-xl font-semibold tracking-tight">Sessions</h1>
        <p className="text-sm text-muted-foreground">
          Every agent the daemon has spawned, live and finished. Open one to watch its terminal.
        </p>
      </header>

      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={goal ?? ANY}
          onValueChange={(value) =>
            // A task filter from another goal would only ever match nothing.
            setFilter({ goal: filterValue(value), task: undefined })
          }
        >
          <SelectTrigger size="sm" aria-label="Filter by goal">
            <SelectValue>
              {(value: string) =>
                value === ANY
                  ? "All goals"
                  : (goals.data?.find((item) => item.id === value)?.title ?? value)
              }
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>All goals</SelectItem>
            {(goals.data ?? []).map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={task ?? ANY}
          onValueChange={(value) => setFilter({ task: filterValue(value) })}
        >
          <SelectTrigger size="sm" aria-label="Filter by task">
            <SelectValue>
              {(value: string) =>
                value === ANY
                  ? "All tasks"
                  : (tasks.data?.find((item) => item.id === value)?.title ?? value)
              }
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>All tasks</SelectItem>
            {(tasks.data ?? []).map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={status ?? ANY}
          onValueChange={(value) => setFilter({ status: filterValue(value) })}
        >
          <SelectTrigger size="sm" aria-label="Filter by status">
            <SelectValue>{(value: string) => (value === ANY ? "Any status" : value)}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>Any status</SelectItem>
            {SESSION_STATUSES.map((value) => (
              <SelectItem key={value} value={value}>
                {value}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {sessions.data ? (
          <span className="ml-auto text-xs text-muted-foreground tabular-nums">
            {sessions.data.length} session{sessions.data.length === 1 ? "" : "s"}
          </span>
        ) : null}
      </div>

      <SessionsList
        filters={filters}
        onSelect={(session) => void navigate(paths.session(session.id))}
      />
    </div>
  )
}

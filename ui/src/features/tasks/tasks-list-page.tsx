/**
 * Every task the daemon knows about, filtered the way `ariadne task ls` filters
 * them — the flat counterpart to the goal's board, and where a `?goal=` or
 * `?status=` link lands.
 *
 * The filters live in the URL so a filtered list can be linked to and survives
 * a reload.
 */

import { useQuery } from "@tanstack/react-query"
import { TriangleAlertIcon, XIcon } from "lucide-react"
import { Link, useSearchParams } from "react-router-dom"

import type { TaskStatus } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"
import { describeError, formatAbsolute, formatRelative, shortId } from "./format"
import { tasksPath } from "./paths"
import { taskListQueryOptions } from "./queries"
import { ALL_STATUSES, TASK_STATUS_META } from "./status"

const ANY_STATUS = "all"

const STATUS_ITEMS: Record<string, string> = {
  [ANY_STATUS]: "Any status",
  ...Object.fromEntries(ALL_STATUSES.map((status) => [status, TASK_STATUS_META[status].label])),
}

export function TasksListPage() {
  const [search, setSearch] = useSearchParams()
  const goal = search.get("goal") ?? undefined
  const status = parseStatus(search.get("status"))
  const tasks = useQuery(taskListQueryOptions({ goal, status }))

  function setStatus(next: TaskStatus | undefined) {
    const params = new URLSearchParams()
    if (goal) params.set("goal", goal)
    if (next) params.set("status", next)
    setSearch(params, { replace: true })
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="font-heading text-lg font-semibold">Tasks</h1>
        {tasks.data && (
          <span className="text-sm text-muted-foreground">{tasks.data.length} shown</span>
        )}
        <div className="ml-auto flex items-center gap-2">
          {goal && (
            <Button
              variant="secondary"
              size="sm"
              render={<Link to={tasksPath({ status })} />}
              title="Show tasks of every goal"
            >
              goal {shortId(goal)}
              <XIcon />
            </Button>
          )}
          <Select
            items={STATUS_ITEMS}
            value={status ?? ANY_STATUS}
            onValueChange={(value) =>
              setStatus(value === ANY_STATUS ? undefined : (value as TaskStatus))
            }
          >
            <SelectTrigger size="sm" className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ANY_STATUS}>Any status</SelectItem>
              {ALL_STATUSES.map((value) => (
                <SelectItem key={value} value={value}>
                  {TASK_STATUS_META[value].label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </header>

      {goal && (
        <p className="text-sm text-muted-foreground">
          Tasks of goal{" "}
          <Link to={paths.goal(goal)} className="font-mono underline underline-offset-3">
            {goal}
          </Link>
          .
        </p>
      )}

      {tasks.error ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load the tasks</AlertTitle>
          <AlertDescription>{describeError(tasks.error)}</AlertDescription>
        </Alert>
      ) : tasks.isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      ) : tasks.data.length === 0 ? (
        <p className="rounded-lg border border-dashed px-4 py-10 text-center text-sm text-muted-foreground">
          No task matches these filters.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Title</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Round</TableHead>
              <TableHead>Branch</TableHead>
              <TableHead className="text-right">Updated</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {tasks.data.map((task) => {
              const meta = TASK_STATUS_META[task.status]
              return (
                <TableRow key={task.id}>
                  <TableCell className="max-w-xs">
                    <Link
                      to={paths.task(task.id)}
                      className="font-medium underline-offset-3 hover:underline"
                    >
                      {task.title}
                    </Link>
                    {task.stalled && (
                      <span
                        className="ml-2 inline-flex items-center gap-1 align-middle text-xs text-amber-600 dark:text-amber-400"
                        title="The agent went idle without advancing the task."
                      >
                        <TriangleAlertIcon className="size-3" />
                        stalled
                      </span>
                    )}
                  </TableCell>
                  <TableCell>
                    <span
                      className={cn("rounded-full px-1.5 py-0.5 text-xs font-medium", meta.badge)}
                    >
                      {meta.label}
                    </span>
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs">
                    {task.review_round}
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {task.branch}
                  </TableCell>
                  <TableCell
                    className="text-right text-xs text-muted-foreground"
                    title={formatAbsolute(task.updated_at)}
                  >
                    {formatRelative(task.updated_at)}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      )}
    </div>
  )
}

/** A `?status=` the daemon would reject is treated as no filter at all. */
function parseStatus(value: string | null): TaskStatus | undefined {
  return ALL_STATUSES.find((status) => status === value)
}

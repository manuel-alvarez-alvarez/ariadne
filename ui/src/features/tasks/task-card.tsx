/**
 * One task, as it appears on the board and in any other list of tasks.
 *
 * What earns space here is what tells you whether a task needs attention:
 * which round of review it is on, whether its agent went idle, and how many
 * other tasks it is waiting for.
 */

import { LayersIcon, TriangleAlertIcon } from "lucide-react"
import { Link } from "react-router-dom"

import type { TaskDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { useTaskPanelTo } from "@/routes/paths"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"

export function TaskCard({ task, showStatus = false }: { task: TaskDto; showStatus?: boolean }) {
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  const terminal = task.status === "cancelled"
  const to = useTaskPanelTo(task.id)

  return (
    <Link
      to={to}
      className={cn(
        "block rounded-lg border bg-card p-2.5 transition-colors hover:border-foreground/20 hover:bg-muted/50",
        task.stalled && "border-amber-500/40",
      )}
    >
      <p
        className={cn(
          "line-clamp-2 text-sm leading-snug font-medium",
          terminal && "text-muted-foreground line-through decoration-muted-foreground/40",
        )}
      >
        {task.title}
      </p>

      <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
        {showStatus && <StatusBadge size="sm" label={status.label} tone={status.badge} />}
        {sub && <StatusBadge size="sm" label={sub.label} tone={sub.badge} title={sub.hint} />}
        {task.review_round > 0 && (
          <Tooltip>
            <TooltipTrigger render={<span className="font-mono" />}>
              R{task.review_round}
            </TooltipTrigger>
            <TooltipContent>Review round {task.review_round}</TooltipContent>
          </Tooltip>
        )}
        {task.depends_on.length > 0 && (
          <Tooltip>
            <TooltipTrigger render={<span className="flex items-center gap-1" />}>
              <LayersIcon className="size-3" />
              {task.depends_on.length}
            </TooltipTrigger>
            <TooltipContent>
              Waits for {task.depends_on.length} {task.depends_on.length === 1 ? "task" : "tasks"}{" "}
              to merge
            </TooltipContent>
          </Tooltip>
        )}
        {task.stalled && (
          <Tooltip>
            <TooltipTrigger
              render={
                <span className="flex items-center gap-1 font-medium text-amber-600 dark:text-amber-400" />
              }
            >
              <TriangleAlertIcon className="size-3" />
              stalled
            </TooltipTrigger>
            <TooltipContent>The agent went idle without advancing the task.</TooltipContent>
          </Tooltip>
        )}
        <time
          className="ml-auto"
          dateTime={task.updated_at}
          title={`${formatAbsolute(task.updated_at)} · ${task.branch}`}
        >
          {formatRelative(task.updated_at)}
        </time>
      </div>
    </Link>
  )
}

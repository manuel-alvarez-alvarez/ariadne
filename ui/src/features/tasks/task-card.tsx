/**
 * One task, as it appears on the board and in any other list of tasks.
 *
 * What earns space here is what tells you whether a task needs attention:
 * which round of review it is on, whether its agent went idle, and how many
 * other tasks it is waiting for — plus the branch, which is the one string an
 * engineer actually wants off the card and into a terminal. It sits outside
 * the link on purpose: a copy button nested in an anchor is neither valid nor
 * clickable without hijacking the navigation.
 *
 * Everything explanatory is a real `Tooltip`. `title=` attributes were the
 * cheaper way to say the same things, and they are unreachable by keyboard.
 */

import { GitBranchIcon, LayersIcon, TriangleAlertIcon } from "lucide-react"
import { Link } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { StatusBadge } from "@/components/status-badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { plural } from "@/lib/plural"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { useTaskPanelTo } from "@/routes/paths"
import { STALLED_META } from "./stalled"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"

export function TaskCard({ task, showStatus = false }: { task: TaskDto; showStatus?: boolean }) {
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  const terminal = task.status === "cancelled"
  const to = useTaskPanelTo(task.id)

  return (
    <div
      className={cn(
        "rounded-lg border bg-card transition-colors hover:border-foreground/20 hover:bg-muted/50",
        task.stalled && STALLED_META.border,
      )}
    >
      <Link
        to={to}
        className="block rounded-lg px-2.5 pt-2.5 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
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
          {sub && (
            <Tooltip>
              <TooltipTrigger render={<span className="flex" />}>
                <StatusBadge size="sm" label={sub.label} tone={sub.badge} />
              </TooltipTrigger>
              <TooltipContent>{sub.hint}</TooltipContent>
            </Tooltip>
          )}
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
                Waits for {plural(task.depends_on.length, "task")} to merge
              </TooltipContent>
            </Tooltip>
          )}
          {task.stalled && (
            <Tooltip>
              <TooltipTrigger
                render={
                  <span className={cn("flex items-center gap-1 font-medium", STALLED_META.text)} />
                }
              >
                <TriangleAlertIcon className="size-3" />
                {STALLED_META.label}
              </TooltipTrigger>
              <TooltipContent>{STALLED_META.hint}</TooltipContent>
            </Tooltip>
          )}
          <Tooltip>
            <TooltipTrigger render={<time className="ml-auto" dateTime={task.updated_at} />}>
              {formatRelative(task.updated_at)}
            </TooltipTrigger>
            <TooltipContent>updated {formatAbsolute(task.updated_at)}</TooltipContent>
          </Tooltip>
        </div>
      </Link>

      <div className="px-2.5 pt-1.5 pb-2.5">
        <span className="flex w-fit max-w-full items-center gap-1 rounded-md bg-muted/60 px-1.5 py-0.5 text-xs text-muted-foreground">
          <GitBranchIcon className="size-3 shrink-0" />
          <CopyableId value={task.branch} label="branch" />
        </span>
      </div>
    </div>
  )
}

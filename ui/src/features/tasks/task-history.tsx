/**
 * The transition audit log — the `task history` equivalent.
 *
 * Every status change the store accepted, in order, with who asked for it. It
 * is the answer to "why is this task where it is", so the reason the daemon
 * recorded is shown in full rather than truncated.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowRightIcon } from "lucide-react"

import type { TaskStatus, TaskTransitionDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { StatusBadge } from "@/components/status-badge"
import { Skeleton } from "@/components/ui/skeleton"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { taskTransitionsQueryOptions } from "./queries"
import { displayLabel, TASK_STATUS_META } from "./status"

export function TaskHistory({ taskId }: { taskId: string }) {
  const transitions = useQuery(taskTransitionsQueryOptions(taskId))

  if (transitions.isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    )
  }

  if (transitions.error) {
    return (
      <ErrorState
        title="Could not load the history"
        error={transitions.error}
        onRetry={() => void transitions.refetch()}
      />
    )
  }

  if (transitions.data.length === 0) {
    return <EmptyState emphasis="quiet" title="The task has not moved yet." />
  }

  return (
    <ol className="relative space-y-0 border-l pl-5">
      {transitions.data.map((transition) => (
        <TransitionRow key={transition.id} transition={transition} />
      ))}
    </ol>
  )
}

function TransitionRow({ transition }: { transition: TaskTransitionDto }) {
  return (
    <li className="relative py-2">
      <span
        className={cn(
          "absolute top-3.5 -left-[calc(0.25rem+1px+1.25rem)] size-2 rounded-full ring-2 ring-background",
          dotFor(transition.to_status),
        )}
      />
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-muted-foreground">{label(transition.from_status)}</span>
        <ArrowRightIcon className="size-3 text-muted-foreground" />
        <span className="font-medium">{label(transition.to_status)}</span>
        <StatusBadge size="sm" label={transition.actor} tone="bg-muted text-muted-foreground" />
        <time
          className="ml-auto text-xs text-muted-foreground"
          dateTime={transition.created_at}
          title={formatAbsolute(transition.created_at)}
        >
          {formatRelative(transition.created_at)}
        </time>
      </div>
      {transition.reason && (
        <p className="mt-0.5 text-xs text-muted-foreground">{transition.reason}</p>
      )}
    </li>
  )
}

/**
 * Transitions carry their statuses as plain strings, so an unknown one — a
 * daemon newer than this build — falls back to the raw value.
 */
function meta(status: string) {
  return TASK_STATUS_META[status as TaskStatus] as (typeof TASK_STATUS_META)[TaskStatus] | undefined
}

function label(status: string): string {
  return meta(status) ? displayLabel(status as TaskStatus) : status
}

function dotFor(status: string): string {
  return meta(status)?.dot ?? "bg-muted-foreground"
}

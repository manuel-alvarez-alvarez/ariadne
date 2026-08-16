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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { describeError, formatAbsolute, formatRelative } from "./format"
import { taskTransitionsQueryOptions } from "./queries"
import { TASK_STATUS_META } from "./status"

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
      <Alert variant="destructive">
        <AlertTitle>Could not load the history</AlertTitle>
        <AlertDescription>{describeError(transitions.error)}</AlertDescription>
      </Alert>
    )
  }

  if (transitions.data.length === 0) {
    return (
      <p className="rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
        The task has not moved yet.
      </p>
    )
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
        <span className="rounded-full bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
          {transition.actor}
        </span>
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
  return meta(status)?.label ?? status
}

function dotFor(status: string): string {
  return meta(status)?.dot ?? "bg-muted-foreground"
}

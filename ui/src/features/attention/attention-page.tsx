/**
 * Everything that is stuck, on one screen, grouped by the goal it belongs to.
 *
 * The rows are links into the panel scheme the rest of the app uses — `?task=`
 * over this screen for a task, `?task=&tab=sessions&session=` for a session
 * that belongs to one, the goals board for a planner session — so reading the
 * list and acting on it are the same gesture, and the list stays underneath.
 *
 * Nothing here polls; see `queries.ts`.
 */

import { CheckCircle2Icon } from "lucide-react"
import { Link, useSearchParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { StatusBadge } from "@/components/status-badge"
import { Skeleton } from "@/components/ui/skeleton"
import { ROLE_LABELS, SessionStatusBadge } from "@/features/sessions/session-display"
import { TASK_STATUS_META } from "@/features/tasks"
import { shortId } from "@/lib/ids"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { paths, taskPanelTo, taskSessionPanelTo } from "@/routes/paths"

import { type AttentionGroup, type AttentionTask, useAttention } from "./queries"

export function AttentionPage() {
  const attention = useAttention()

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Attention"
        description="Tasks that stalled, failed or came back with changes requested, and agents that died."
      />

      {attention.error ? (
        <ErrorState
          showIcon
          title="Could not load what needs attention"
          error={attention.error}
          onRetry={attention.refetch}
        />
      ) : null}

      {attention.isPending ? <AttentionSkeleton /> : null}

      {!attention.isPending && attention.groups.length === 0 ? (
        <EmptyState
          icon={<CheckCircle2Icon className="size-5" />}
          title="Nothing needs attention"
          description="No stalled, failed or changes-requested tasks, and no failed sessions."
        />
      ) : null}

      {attention.groups.map((group) => (
        <GoalGroup key={group.goalId} group={group} />
      ))}
    </div>
  )
}

function GoalGroup({ group }: { group: AttentionGroup }) {
  const count = group.tasks.length + group.sessions.length

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <h2 className="font-heading text-sm font-semibold">
          <Link to={paths.goal(group.goalId)} className="underline-offset-3 hover:underline">
            {group.goal?.title ?? `Goal ${shortId(group.goalId)}`}
          </Link>
        </h2>
        <span className="text-xs text-muted-foreground">
          {count} {count === 1 ? "item" : "items"}
        </span>
      </div>

      <ul className="divide-y rounded-lg border">
        {group.tasks.map((item) => (
          <TaskRow key={item.task.id} item={item} />
        ))}
        {group.sessions.map((session) => (
          <SessionRow key={session.id} session={session} />
        ))}
      </ul>
    </section>
  )
}

function TaskRow({ item: { task, reason } }: { item: AttentionTask }) {
  const [search] = useSearchParams()
  const meta = TASK_STATUS_META[task.status]

  return (
    <li>
      <Link
        to={taskPanelTo(search, task.id)}
        className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm transition-colors hover:bg-muted/50"
      >
        <StatusBadge box="badge" label={meta.label} tone={meta.badge} title={meta.hint} />
        <span className="min-w-0 flex-1 truncate font-medium">{task.title}</span>
        {/* The status pill already names the other two reasons; a stall is a
            flag on top of whatever status the task is sitting in. */}
        {reason === "stalled" ? (
          <StatusBadge
            box="badge"
            label="Stalled"
            tone="bg-status-warn-soft text-status-warn-fg"
            title="The agent went idle without advancing the task."
          />
        ) : null}
        <span className="font-mono text-xs text-muted-foreground" title={task.id}>
          {shortId(task.id)}
        </span>
      </Link>
    </li>
  )
}

function SessionRow({ session }: { session: SessionDto }) {
  const [search] = useSearchParams()
  // A planner session belongs to no task, and the goal panel only opens on the
  // board — so that one link leaves this screen, and the rest do not.
  const to = session.task_id
    ? taskSessionPanelTo(search, session.task_id, session.id)
    : paths.goalSession(session.goal_id, session.id)

  return (
    <li>
      <Link
        to={to}
        className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm transition-colors hover:bg-muted/50"
      >
        <SessionStatusBadge status={session.status} />
        <span className="min-w-0 flex-1 truncate">
          {ROLE_LABELS[session.role]} session
          {session.task_id ? null : <span className="text-muted-foreground"> · planner</span>}
        </span>
        <span
          className="text-xs text-muted-foreground"
          title={formatAbsolute(session.ended_at ?? session.created_at)}
        >
          {formatRelative(session.ended_at ?? session.created_at)}
        </span>
        <span className="font-mono text-xs text-muted-foreground" title={session.id}>
          {shortId(session.id)}
        </span>
      </Link>
    </li>
  )
}

function AttentionSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      {[0, 1, 2].map((row) => (
        <Skeleton key={row} className="h-12 w-full" />
      ))}
    </div>
  )
}

/**
 * Everything that is stuck, on one screen, grouped by the goal it belongs to.
 *
 * The rows are links into the panel scheme the rest of the app uses — `?task=`
 * over this screen for a task, `?session=` for a session, whichever goal or
 * task that one belongs to — so reading the list and acting on it are the same
 * gesture, and the list stays underneath.
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
import { StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { describeError } from "@/lib/errors"
import { shortId } from "@/lib/ids"
import { ROLE_LABELS } from "@/lib/labels"
import { plural } from "@/lib/plural"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { paths, sessionPanelTo, taskPanelTo } from "@/routes/paths"

import { type AttentionGroup, type AttentionTask, useAttention } from "./queries"

export function AttentionPage() {
  const attention = useAttention()

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Attention"
        description="Tasks that stalled, failed or came back with changes requested, and failed sessions."
      />

      {/* One of the three queries can fail while the others answered, and what
          is then on screen is a real list with holes in it — not a screen that
          failed to load. Saying so is the difference between "read this, some
          of it is missing" and "there is nothing here". */}
      {attention.error ? (
        attention.partial ? (
          <ErrorState
            title="Some of this list could not be loaded"
            error={attention.error}
            description={`${describeError(attention.error)} — what is below is everything that did load.`}
            onRetry={attention.refetch}
          />
        ) : (
          <ErrorState
            showIcon
            title="Could not load what needs attention"
            error={attention.error}
            onRetry={attention.refetch}
          />
        )
      ) : null}

      {attention.isPending ? <AttentionSkeleton /> : null}

      {/* Only once everything answered: an empty list under a failed query is
          not "nothing needs attention", it is nothing loaded. */}
      {!attention.isPending && !attention.error && attention.groups.length === 0 ? (
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
        <span className="text-xs text-muted-foreground">{plural(count, "item")}</span>
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

/** The two row kinds sit under one another, so they carry the same three tails. */
const ROW_LINK =
  "flex flex-wrap items-center gap-2 px-3 py-2 text-sm transition-colors hover:bg-muted/50"

function TaskRow({ item: { task, reason } }: { item: AttentionTask }) {
  const [search] = useSearchParams()
  const meta = TASK_STATUS_META[task.status]

  return (
    <li>
      <Link to={taskPanelTo(search, task.id)} className={ROW_LINK}>
        <StatusBadge box="badge" label={meta.label} tone={meta.badge} hint={meta.hint} />
        <span className="min-w-0 flex-1 truncate font-medium">{task.title}</span>
        {/* The status pill already names the other two reasons; a stall is a
            flag on top of whatever status the task is sitting in. */}
        {reason === "stalled" ? <StalledBadge /> : null}
        <Age at={task.updated_at} />
        <RowId id={task.id} />
      </Link>
    </li>
  )
}

function SessionRow({ session }: { session: SessionDto }) {
  const [search] = useSearchParams()
  const meta = TASK_STATUS_META.failed

  return (
    <li>
      {/* A session opens in a panel of its own, over this list — the same one
          for a planner session, which belongs to no task. */}
      <Link to={sessionPanelTo(search, session.id)} className={ROW_LINK}>
        {/* This list holds failed sessions only, and the failed task above it
            is already a tinted badge — so "Failed" is spelled the one way on
            this screen rather than being the session badge here and the task
            badge one row up. `SessionStatusBadge` stays what the sessions
            feature shows, where a badge has five statuses to tell apart. */}
        <StatusBadge box="badge" label={meta.label} tone={meta.badge} hint="The agent died." />
        <span className="min-w-0 flex-1 truncate">
          {ROLE_LABELS[session.role]} session
          {session.task_id ? null : <span className="text-muted-foreground"> · planner</span>}
        </span>
        <Age at={session.ended_at ?? session.created_at} />
        <RowId id={session.id} />
      </Link>
    </li>
  )
}

/** When this row last moved, the one way for both kinds. */
function Age({ at }: { at: string }) {
  return (
    <time dateTime={at} className="text-xs text-muted-foreground" title={formatAbsolute(at)}>
      {formatRelative(at)}
    </time>
  )
}

function RowId({ id }: { id: string }) {
  return (
    <span className="font-mono text-xs text-muted-foreground" title={id}>
      {shortId(id)}
    </span>
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

/**
 * Why the task is where it is, as a card above its technical facts.
 *
 * Only what the daemon recorded is said: the status and step, the failure
 * reason, the stalled flag, the dependencies' own statuses and the attention
 * reason of the task's live sessions. A task with nothing recorded says its
 * cause is unknown — never a guess from silence or from how long it has sat.
 *
 * The card reads top down: a headline with the status dot, a muted line of
 * detail, then one row per thing the user can open — each dependency still
 * unfinished, each session asking for someone — with the stall, the one
 * recorded condition with nothing to open, as a row of its own.
 */

import { useQueries } from "@tanstack/react-query"
import { ArrowUpRightIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"

import type { SessionDto, TaskDto, WorkflowStepDto } from "@/api"
import {
  isLiveStatus,
  SESSION_ATTENTION_META,
  sessionAttention,
} from "@/features/sessions/session-display"
import { cn } from "@/lib/format"
import { usePanelSessionTo, useTaskPanelTo } from "@/routes/paths"
import { taskQueryOptions } from "./queries"
import { STALLED_META } from "./stalled"
import { TASK_STATUS_META } from "./status"
import { stepTitle } from "./steps"

type Dependency = { id: string; task?: TaskDto }

export function TaskSummary({
  task,
  steps = [],
  sessions,
}: {
  task: TaskDto
  steps?: WorkflowStepDto[]
  /** The task's sessions, once loaded. */
  sessions?: SessionDto[]
}) {
  const results = useQueries({ queries: task.depends_on.map((id) => taskQueryOptions(id)) })
  const dependencies: Dependency[] = task.depends_on.map((id, index) => ({
    id,
    task: results[index]?.data,
  }))
  // Only a dependency whose status has loaded is a recorded blocker; one that
  // is still loading, or failed to load, says nothing either way.
  const unmet = dependencies.filter(
    (dependency) => dependency.task && dependency.task.status !== "finished",
  )
  const unresolved = dependencies.filter((dependency) => !dependency.task)
  const asking = (sessions ?? []).filter(
    (session) => isLiveStatus(session.status) && sessionAttention(session) !== null,
  )
  const stalled = task.stalled && task.status !== "finished" && task.status !== "cancelled"

  let headline: string
  let detail: string | null = null
  // A task that already moved on or ended still names what it depends on.
  let listed: { label: string; items: Dependency[] } | null =
    unmet.length > 0 ? { label: "Unfinished dependency", items: unmet } : null

  if (task.status === "failed" || task.status === "cancelled") {
    headline = TASK_STATUS_META[task.status].label
    detail = task.reason ?? "No reason was recorded for it."
  } else if (task.status === "finished") {
    headline = "Finished"
    detail = "Its workflow completed."
    listed = null
  } else if (task.status === "in_progress") {
    headline = task.step ? `Running the ${stepTitle(steps, task.step)} column` : "Running"
  } else if (unmet.length > 0) {
    headline = `Waiting for ${unmet.length === 1 ? "a dependency" : `${unmet.length} dependencies`}`
    listed = { label: "Waiting for", items: unmet }
  } else if (unresolved.length > 0) {
    headline = "Cause unknown"
    detail = `${TASK_STATUS_META[task.status].label}. Dependency status is not loaded, so no blocker can be named.`
    listed = { label: "Not loaded", items: unresolved }
  } else if (task.status === "ready") {
    headline = "Ready"
    detail = "Waiting for its workflow to start. No blocker is recorded."
  } else {
    headline = "Cause unknown"
    detail =
      "Pending. Its dependencies are finished or it has none, and no other cause is recorded."
  }

  const warn = stalled || asking.length > 0
  return (
    <section
      aria-label="Execution summary"
      className={cn(
        "rounded-lg border bg-card text-sm",
        TASK_STATUS_META[task.status].border ?? (warn && STALLED_META.border),
      )}
    >
      <div className="space-y-0.5 px-3 py-2.5">
        <p className="flex items-center gap-2 font-medium">
          <span className={cn("size-2 shrink-0 rounded-full", TASK_STATUS_META[task.status].dot)} />
          {headline}
        </p>
        {detail ? <p className="pl-4 text-xs text-muted-foreground">{detail}</p> : null}
      </div>
      {listed || stalled || asking.length > 0 ? (
        <ul className="divide-y border-t text-xs">
          {stalled ? (
            <Row tone={STALLED_META.text}>
              {STALLED_META.label}: {STALLED_META.hint}
            </Row>
          ) : null}
          {asking.map((session) => (
            <SessionRow key={session.id} session={session} />
          ))}
          {listed?.items.map((dependency) => (
            <DependencyRow key={dependency.id} label={listed.label} {...dependency} />
          ))}
        </ul>
      ) : null}
    </section>
  )
}

function Row({ tone, children }: { tone?: string; children: ReactNode }) {
  return <li className={cn("flex min-w-0 items-center gap-2 px-3 py-1.5", tone)}>{children}</li>
}

function DependencyRow({ id, task, label }: Dependency & { label: string }) {
  const to = useTaskPanelTo(id)
  const status = task ? TASK_STATUS_META[task.status] : undefined
  return (
    <Row>
      <span className="w-32 shrink-0 text-muted-foreground">{label}</span>
      <span className={cn("size-1.5 shrink-0 rounded-full", status?.dot ?? "bg-border")} />
      <Link to={to} replace className="min-w-0 truncate underline-offset-3 hover:underline">
        {task?.title ?? id}
      </Link>
      <span className="ml-auto shrink-0 text-muted-foreground">
        {status ? status.label : "Status unknown"}
      </span>
    </Row>
  )
}

function SessionRow({ session }: { session: SessionDto }) {
  const to = usePanelSessionTo(session.id)
  const reason = sessionAttention(session)
  if (reason === null) return null
  return (
    <Row tone="text-status-warn-fg">
      A session is {SESSION_ATTENTION_META[reason].label.toLowerCase()}.
      <Link
        to={to}
        replace
        className="ml-auto flex shrink-0 items-center gap-1 font-medium underline-offset-3 hover:underline"
      >
        Open the session
        <ArrowUpRightIcon className="size-3.5" />
      </Link>
    </Row>
  )
}

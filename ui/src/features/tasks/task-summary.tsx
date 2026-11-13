/**
 * Why the task is where it is, in a sentence or two above its technical facts.
 *
 * Only what the daemon recorded is said: the status and step, the failure
 * reason, the stalled flag, the dependencies' own statuses and the attention
 * reason of the task's live sessions. A task with nothing recorded says its
 * cause is unknown — never a guess from silence or from how long it has sat.
 */

import { useQueries } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"

import type { SessionDto, TaskDto, WorkflowStepDto } from "@/api"
import {
  isLiveStatus,
  SESSION_ATTENTION_META,
  sessionAttention,
} from "@/features/sessions/session-display"
import { usePanelSessionTo, useTaskPanelTo } from "@/routes/paths"
import { taskQueryOptions } from "./queries"
import { STALLED_META } from "./stalled"
import { TASK_STATUS_META } from "./status"
import { stepTitle } from "./steps"

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
  const dependencies = task.depends_on.map((id, index) => ({ id, task: results[index]?.data }))
  // Only a dependency whose status has loaded is a recorded blocker; one that
  // is still loading, or failed to load, says nothing either way.
  const unmet = dependencies.filter(
    (dependency) => dependency.task && dependency.task.status !== "finished",
  )
  const unresolved = dependencies.filter((dependency) => !dependency.task)
  const asking = (sessions ?? []).filter(
    (session) => isLiveStatus(session.status) && sessionAttention(session) !== null,
  )
  const lines: ReactNode[] = []

  if (task.status === "failed" || task.status === "cancelled") {
    const word = task.status === "failed" ? "Failed" : "Cancelled"
    lines.push(task.reason ? `${word}: ${task.reason}` : `${word}. No reason was recorded for it.`)
  } else if (task.status === "finished") {
    lines.push("Finished: its workflow completed.")
  } else if (task.status === "in_progress") {
    lines.push(task.step ? `Running the ${stepTitle(steps, task.step)} column.` : "Running.")
  } else if (unmet.length > 0) {
    lines.push(<DependencyList lead="Waiting for" items={unmet} />)
  } else if (unresolved.length > 0) {
    lines.push(
      <>
        {task.status === "ready" ? "Ready." : "Pending."} Dependency status is not loaded, so the
        cause is unknown. <DependencyList lead="Check" items={unresolved} />
      </>,
    )
  } else if (task.status === "ready") {
    lines.push("Ready for its workflow to start. No blocker is recorded.")
  } else {
    lines.push(
      "Pending. Its dependencies are finished or it has none, and no other cause is recorded. The cause is unknown.",
    )
  }

  // A task that already moved on or ended still names what it depends on.
  if (["in_progress", "failed", "cancelled"].includes(task.status) && unmet.length > 0) {
    lines.push(<DependencyList lead="Unfinished dependencies:" items={unmet} />)
  }

  if (task.stalled && task.status !== "finished" && task.status !== "cancelled") {
    lines.push(`${STALLED_META.label}: ${STALLED_META.hint}`)
  }

  return (
    // Set like the facts below it: the same small muted label over `text-xs`
    // text, inside the strip's own width, with a rule down its side to read as
    // the lead of that strip rather than as stray paragraphs.
    <section aria-label="Execution summary" className="space-y-0.5 border-l-2 pl-3 text-xs">
      <h3 className="text-muted-foreground">Status</h3>
      <div className="space-y-1 leading-snug">
        {lines.map((line, index) => (
          // Fixed, derived from the task alone, never reordered.
          // biome-ignore lint/suspicious/noArrayIndexKey: see above
          <p key={index}>{line}</p>
        ))}
        {asking.map((session) => (
          <SessionAsking key={session.id} session={session} />
        ))}
      </div>
    </section>
  )
}

function DependencyList({
  lead,
  items,
}: {
  lead: string
  items: { id: string; task?: TaskDto }[]
}) {
  return (
    <>
      {lead}{" "}
      {items.map((dependency, index) => (
        <span key={dependency.id}>
          {index > 0 ? ", " : ""}
          <DependencyLink id={dependency.id} task={dependency.task} />
        </span>
      ))}
      .
    </>
  )
}

function DependencyLink({ id, task }: { id: string; task?: TaskDto }) {
  const to = useTaskPanelTo(id)
  return (
    <>
      <Link to={to} replace className="underline underline-offset-3">
        {task?.title ?? id}
      </Link>{" "}
      <span className="text-muted-foreground">
        ({task ? TASK_STATUS_META[task.status].label.toLowerCase() : "status unknown"})
      </span>
    </>
  )
}

function SessionAsking({ session }: { session: SessionDto }) {
  const to = usePanelSessionTo(session.id)
  const reason = sessionAttention(session)
  if (reason === null) return null
  return (
    <p>
      A session is {SESSION_ATTENTION_META[reason].label.toLowerCase()}.{" "}
      <Link to={to} replace className="underline underline-offset-3">
        Open the session
      </Link>
    </p>
  )
}

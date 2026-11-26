/**
 * One task as one line of the goals board: the facts of the {@link TaskCard}
 * — title, attention badge, state badge, host, step track, tokens, pull
 * request and last activity — laid out on {@link TASK_ROW_GRID}, whose fixed
 * columns line up from one row to the next.
 *
 * The row wears the card's status outline, and like the card it is one link
 * to the task panel and one tab stop. Every hint is read off the link's
 * description, and the PR link is a real anchor laid over its own column,
 * outside the row's link, with no tab stop of its own.
 */

import { useId } from "react"
import { Link } from "react-router-dom"

import type { Host, TaskDto, WorkflowStepDto } from "@/api"
import { HostLabel } from "@/components/host-label"
import { PrLink } from "@/components/pr-link"
import { StatusBadge } from "@/components/status-badge"
import { stepUsageRows, TokenFigure } from "@/components/token-figure"
import { When } from "@/components/when"
import { type SessionAttention, SessionAttentionBadge } from "@/features/sessions/session-display"
import { cn } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { useSourceHost } from "./source-host"
import { StepTrack } from "./step-track"
import { findStep, stepTitle } from "./steps"
import { cardBorder, cardHints, cardState, repeatsCard } from "./task-card"

/** Nothing inside the row's link takes a tab stop of its own. */
const NOT_A_STOP = -1

/**
 * The columns of every row: title, badges, host, steps, tokens, pull request
 * and last activity. Fixed widths past the title, so the facts of one kind
 * sit in one column down the board.
 */
const TASK_ROW_GRID =
  "grid grid-cols-[minmax(10rem,1fr)_8rem_6rem_minmax(6rem,12rem)_7rem_4.5rem_3rem] items-center gap-x-3"

export function TaskRow({
  host,
  task,
  steps,
  attention,
  lastActivityAt,
}: {
  /** The host the task runs on, which its panel link carries. */
  host: Host
  task: TaskDto
  /** The workflow columns the step track draws one dot for each. */
  steps: WorkflowStepDto[]
  /** Why one of this task's sessions wants a person, as on the card. */
  attention?: SessionAttention | null
  /** The latest session activity of the task, as on the card. */
  lastActivityAt?: string | null
}) {
  const executionHost = useSourceHost(task.execution_context?.execution_owner)
  const displayedHost = task.execution_context
    ? (executionHost ?? "Unconfigured worker")
    : task.host
  const activity =
    lastActivityAt && Date.parse(lastActivityAt) > Date.parse(task.updated_at)
      ? lastActivityAt
      : task.updated_at
  const state = cardState(task)
  const to = useTaskPanelTo(task.id, host)
  const flagged = attention && !repeatsCard(attention, state) ? attention : null
  const hintsId = useId()
  const currentStep = task.status === "in_progress" ? findStep(steps, task.step) : undefined
  const currentAgent = currentStep
    ? task.agents.find((agent) => agent.step === currentStep.id)
    : undefined
  const hints = cardHints(
    { ...task, host: displayedHost },
    { state, flagged, currentStep, currentAgent, activity, steps },
  )

  return (
    <div
      data-task-row
      className={cn(
        "relative min-w-0 rounded-md border bg-card text-xs text-muted-foreground transition-colors hover:border-foreground/20 hover:bg-muted",
        cardBorder(task, attention),
      )}
    >
      <Link
        to={to}
        replace={to.replace}
        aria-describedby={hints.length > 0 ? hintsId : undefined}
        className={cn(
          TASK_ROW_GRID,
          "h-8 rounded-md px-2.5 whitespace-nowrap focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
        )}
      >
        <span
          className={cn(
            "min-w-0 truncate text-sm font-medium text-foreground",
            task.status === "cancelled" &&
              "text-muted-foreground line-through decoration-muted-foreground/40",
          )}
        >
          {task.title}
        </span>
        <span className="flex min-w-0 items-center gap-1 overflow-hidden">
          {flagged && (
            <SessionAttentionBadge size="sm" attention={flagged} hintTabIndex={NOT_A_STOP} />
          )}
          {state && (
            <StatusBadge
              size="sm"
              label={state.label}
              tone={state.tone}
              hint={state.hint}
              hintTabIndex={NOT_A_STOP}
            />
          )}
        </span>
        <span className="min-w-0">
          {task.execution_context && !executionHost ? (
            <span className="block truncate">Unconfigured worker</span>
          ) : displayedHost && (displayedHost !== "local" || task.execution_context) ? (
            <HostLabel
              host={displayedHost}
              className="block max-w-full truncate"
              title={
                task.host_unreachable
                  ? `Host ${displayedHost} cannot be reached; progress may be stale`
                  : displayedHost
              }
            />
          ) : null}
        </span>
        {/* Its own container: the cell's width, not the row's, decides
            which step names the track draws. */}
        <span className="@container flex min-w-0 items-center">
          <StepTrack task={task} steps={steps} />
        </span>
        <span className="min-w-0 truncate">
          <TokenFigure
            usage={task.usage.total}
            rows={stepUsageRows(task.usage, (step) => stepTitle(steps, step))}
            tabIndex={NOT_A_STOP}
          />
        </span>
        {/* The PR column: the anchor itself is laid over it below. */}
        <span aria-hidden />
        <span className="min-w-0 truncate text-right">
          <When at={activity} format="age" label="last activity" hintTabIndex={NOT_A_STOP} />
        </span>
      </Link>

      {hints.length > 0 && (
        <p id={hintsId} className="sr-only">
          {hints.join(". ")}
        </p>
      )}

      {/* A real anchor cannot sit inside the row's link, so it is laid over
          the row on the same grid, in the PR column. */}
      {task.pr_url && (
        <div className={cn(TASK_ROW_GRID, "pointer-events-none absolute inset-0 px-2.5 text-xs")}>
          <PrLink
            url={task.pr_url}
            context={task.title}
            tabIndex={NOT_A_STOP}
            className="pointer-events-auto col-start-6 h-4 min-w-0 text-muted-foreground hover:text-foreground"
          />
        </div>
      )}
    </div>
  )
}

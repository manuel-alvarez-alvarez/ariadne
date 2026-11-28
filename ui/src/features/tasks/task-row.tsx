/**
 * One task as one line of the goals board: the facts of the {@link TaskCard}
 * — title, attention badge, state badge, host, step track, tokens, pull
 * request and last activity — in the columns of {@link BOARD_COLUMNS}.
 *
 * The board is one grid and every row a subgrid of it, so a column is as wide
 * as the widest cell in it down the whole board: the token column fits the
 * largest figure whole. A narrow board hides whole columns, by its own width
 * and not the window's: the step names first, then the last activity, the
 * host and the PR link. The title, the state, the step dots and the tokens
 * stay at every width.
 *
 * The row has no border and no fill; hover and keyboard focus tint it. Its
 * rail segment at the left edge takes the card's status colour, and the
 * segments of one goal's rows join into one rail. Like the card it is one
 * link to the task panel and one tab stop. Every hint is read off the link's
 * description, and the PR link is a real anchor in its own column, outside
 * the row's link, with no tab stop of its own.
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
import { StepTrack, stepDotsWidth } from "./step-track"
import { findStep, stepTitle } from "./steps"
import { cardBorder, cardHints, cardState, repeatsCard } from "./task-card"

/** Nothing inside the row's link takes a tab stop of its own. */
const NOT_A_STOP = -1

/**
 * The board's columns: title, badges, host, steps, tokens, pull request and
 * last activity. The step column narrows to its dots once the step names
 * hide. A hidden cell leaves its `auto` column empty, so the column takes no
 * width; there is no column gap, and each cell past the title pads itself.
 *
 * The badges, the dots and the tokens never shrink below their content: the
 * badges and the tokens are `auto` and `max-content`, and the step column is
 * at least `--board-dots` wide, the widest dots of any workflow on the board
 * ({@link boardDotsWidth}): a folded long workflow can need less than an
 * unfolded shorter one. The title alone gives up its width, and ends in an
 * ellipsis. On the narrowest board the app draws (33.9rem, a pinned
 * inspector beside it) all four fit with the widest figure and badges.
 */
export const BOARD_COLUMNS =
  "grid grid-cols-[minmax(0,1fr)_auto_auto_minmax(var(--board-dots,0rem),12rem)_max-content_auto_auto] @max-[60rem]/board:grid-cols-[minmax(0,1fr)_auto_auto_minmax(var(--board-dots,0rem),9.5rem)_max-content_auto_auto] gap-x-0"

/** The step column's floor, in rem, for a board whose workflows have these step counts. */
export function boardDotsWidth(workflows: readonly number[]): number {
  // The widest track, folded or not, and the cell's own `pl-3` on top of it.
  const dots = Math.max(0, ...workflows.map(stepDotsWidth))
  return dots === 0 ? 0 : dots + 0.75
}

/** The container whose width the columns follow: the board, not the window. */
export const BOARD_CONTAINER = "@container/board"

/** A line that takes the board's columns: a row, its link, a goal's own line. */
export const BOARD_LINE = "col-span-full grid grid-cols-subgrid items-center"

/** The token column, which a goal's own line puts its total in too. */
export const TOKEN_CELL = "col-start-5 pl-3 whitespace-nowrap"

/** Step names hide first: the board's widest width that still drops them. */
const STEP_NAMES = "@max-[60rem]/board:[&_[data-step-name]]:hidden"
/**
 * On the board, only the current step gives up width, by its name: the other
 * steps keep their dots whole, which the column's floor makes room for.
 */
const STEP_DOTS = "[&_li:not([aria-current])]:shrink-0"
const ACTIVITY_CELL = "col-start-7 pl-3 @max-[54rem]/board:hidden"
/** The host hides with its column, on a row and on a goal's own line alike. */
export const HOST_HIDES = "@max-[48rem]/board:hidden"
const HOST_CELL = cn("col-start-3 max-w-[6.75rem] pl-3", HOST_HIDES)
const PR_CELL = "col-start-6 row-start-1 pl-3 @max-[42rem]/board:hidden"

export function TaskRow({
  host,
  task,
  steps,
  attention,
  lastActivityAt,
  showHost = false,
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
  /**
   * Name the row's host also where it runs on that host itself, as a goal
   * line names its goal's: a task with no goal has no goal line to do it.
   */
  showHost?: boolean
}) {
  const executionHost = useSourceHost(task.execution_context?.execution_owner)
  const runsHere = !task.host || task.host === "local"
  const displayedHost = task.execution_context
    ? (executionHost ?? "Unconfigured worker")
    : runsHere && showHost
      ? host
      : task.host
  // A task that runs on its own host names none, unless the caller asks.
  const labelled = task.execution_context || !runsHere || showHost
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
        BOARD_LINE,
        "relative min-w-0 rounded-r-md text-xs text-muted-foreground transition-colors hover:bg-muted/60 has-[a:focus-visible]:bg-muted/60",
      )}
    >
      {/* The row's piece of its goal's rail, in the colour the card's border has. */}
      <span
        aria-hidden
        data-rail
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0 border-l-2",
          cardBorder(task, attention) ?? "border-border",
        )}
      />
      <Link
        to={to}
        replace={to.replace}
        aria-describedby={hints.length > 0 ? hintsId : undefined}
        className={cn(
          BOARD_LINE,
          "row-start-1 h-8 rounded-r-md pr-2.5 pl-3 whitespace-nowrap focus-visible:outline-none",
        )}
      >
        <span
          data-column="title"
          className={cn(
            "col-start-1 min-w-0 truncate text-sm font-medium text-foreground",
            task.status === "cancelled" &&
              "text-muted-foreground line-through decoration-muted-foreground/40",
          )}
        >
          {task.title}
        </span>
        <span data-column="state" className="col-start-2 flex items-center gap-1 pl-3">
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
        <span data-column="host" className={cn(HOST_CELL, "min-w-0")}>
          {task.execution_context && !executionHost ? (
            <span className="block truncate">Unconfigured worker</span>
          ) : displayedHost && labelled ? (
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
            which step names the track draws and which steps it folds. */}
        <span
          data-column="steps"
          className={cn(
            "col-start-4 @container flex min-w-0 items-center pl-3",
            STEP_NAMES,
            STEP_DOTS,
          )}
        >
          <StepTrack task={task} steps={steps} />
        </span>
        {/* Never cut: the column is as wide as the widest figure. */}
        <span data-column="tokens" className={TOKEN_CELL}>
          <TokenFigure
            usage={task.usage.total}
            rows={stepUsageRows(task.usage, (step) => stepTitle(steps, step))}
            tabIndex={NOT_A_STOP}
          />
        </span>
        <span data-column="activity" className={cn(ACTIVITY_CELL, "min-w-0 truncate text-right")}>
          <When at={activity} format="age" label="last activity" hintTabIndex={NOT_A_STOP} />
        </span>
      </Link>

      {hints.length > 0 && (
        <p id={hintsId} className="sr-only">
          {hints.join(". ")}
        </p>
      )}

      {/* A real anchor cannot sit inside the row's link, so it is the row's
          own cell, laid over the link's line in the PR column. */}
      {task.pr_url && (
        <span data-column="pr" className={cn(PR_CELL, "relative z-10 flex min-w-0")}>
          <PrLink
            url={task.pr_url}
            context={task.title}
            tabIndex={NOT_A_STOP}
            className="h-4 min-w-0 text-muted-foreground hover:text-foreground"
          />
        </span>
      )}
    </div>
  )
}

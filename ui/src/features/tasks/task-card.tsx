/**
 * One task, as it appears on the board and in any other list of tasks: three
 * contained rows, whatever the task's status and whatever the width. Row 1 is
 * the title (two lines, ellipsis) with what is wrong or being waited on — one
 * state badge, and the session attention badge when an agent wants a person;
 * the badges shrink to their dot when the card is narrow. Row 2 is the
 * workflow ({@link StepTrack}): circles, step names and arrows on one line,
 * for finished and cancelled tasks too. Row 3 is the task's `TokenFigure`
 * (`↑input share ↓output`, the goal lane header's own rendering, at every
 * width) at the left, the PR link beside it, and the last activity at the
 * right end; the figure is the part that is cut first. The figure's hover names Input and Output and lists one
 * row per workflow step.
 *
 * The card is an `@container`: its own width, not the window's, decides which
 * step names, labels and time wording are drawn, so it holds in a board cell
 * and in the goal panel alike. Models are only in tooltips and the hints.
 *
 * The card says each thing **once**. A session attention badge that repeats
 * the state badge's label is dropped rather than drawn twice (see
 * {@link repeatsCard}), which is what a stalled task saying "Stalled" beside
 * "⚠ Stalled" was.
 *
 * **The whole card is one tab stop.** Everything explanatory is a real
 * `Tooltip` for a pointer, and not one of them is focusable: the tooltip
 * primitive makes every trigger focusable (`components/ui/tooltip.tsx`), which
 * inside an anchor is an interactive node inside an interactive one. Every
 * hint is collected into one hidden line instead ({@link cardHints}) and hung
 * off the link with `aria-describedby`, so what was only in a tooltip is read
 * where the card is landed on. The PR link is the one real anchor on the card
 * and sits outside the link, as a sibling laid over row 3 where the row holds its
 * slot, with no tab stop of its own.
 */

import { GitPullRequestIcon } from "lucide-react"
import { useId } from "react"
import { Link } from "react-router-dom"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { PrLink } from "@/components/pr-link"
import { StatusBadge } from "@/components/status-badge"
import { stepUsageRows, TokenFigure } from "@/components/token-figure"
import { When, whenHint } from "@/components/when"
import {
  SESSION_ATTENTION_META,
  type SessionAttention,
  SessionAttentionBadge,
} from "@/features/sessions/session-display"
import { cn, formatTokens, plural } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { STALLED_META } from "./stalled"
import { TASK_STATUS_META } from "./status"
import { StepTrack } from "./step-track"
import { findStep, stepTitle } from "./steps"

/**
 * The card is one tab stop, so nothing drawn on it takes one of its own.
 * Every hint is read off the link's description instead ({@link cardHints}).
 */
const NOT_A_STOP = -1

/** The last activity time, short on a narrow card and in full on a wide one. */
function ActivityTime({ at, hintTabIndex }: { at: string; hintTabIndex: number }) {
  return (
    <>
      <When
        at={at}
        format="age"
        label="last activity"
        hintTabIndex={hintTabIndex}
        className="@[20rem]:hidden"
      />
      <When
        at={at}
        label="last activity"
        hintTabIndex={hintTabIndex}
        className="hidden @[20rem]:inline"
      />
    </>
  )
}

/** An invisible stand-in the size of the {@link PrLink} for `url`, in the link's own row. */
function PrLinkSlot({ url }: { url: string }) {
  const digits = /\/(?:pulls?|merge_requests)\/(\d+)/.exec(url)?.[1]
  return (
    <span
      aria-hidden
      data-pr-slot
      className="invisible inline-flex h-4 shrink-0 items-center gap-1"
    >
      <GitPullRequestIcon className="size-3.5 shrink-0" />
      <span>{digits ? `#${digits}` : "PR"}</span>
    </span>
  )
}

interface CardState {
  label: string
  hint: string
  tone: string
}

export function TaskCard({
  task,
  steps,
  attention,
  lastActivityAt,
}: {
  task: TaskDto
  /** The goal's workflow columns, which the step track draws one dot for each. */
  steps: WorkflowStepDto[]
  /**
   * Why one of this task's sessions wants a person, when one does — the flag
   * the daemon raised, handed down by whoever is holding the sessions list
   * (`useBoardAttention`) rather than fetched per card.
   */
  attention?: SessionAttention | null
  /**
   * The latest `last_activity_at` among the task's sessions, previous ones
   * included (`useBoardAttention().lastActivity`); the card shows it only
   * when it is later than the task's own `updated_at`.
   */
  lastActivityAt?: string | null
}) {
  const activity =
    lastActivityAt && Date.parse(lastActivityAt) > Date.parse(task.updated_at)
      ? lastActivityAt
      : task.updated_at
  const state = cardState(task)
  const terminal = task.status === "cancelled"
  // `to.replace` is set exactly when a goal panel is open — the card's own
  // link is replacing it rather than stacking on it, on the board behind it
  // or in the goal's own task list alike (`routes/paths.ts`'s `taskPanelTo`).
  const to = useTaskPanelTo(task.id)
  // The reason is still what outlines the card even where the badge for it is
  // dropped: it is the same fact, said once.
  const flagged = attention && !repeatsCard(attention, state) ? attention : null
  const hintsId = useId()
  const currentStep = task.status === "in_progress" ? findStep(steps, task.step) : undefined
  const currentAgent = currentStep
    ? task.agents.find((agent) => agent.step === currentStep.id)
    : undefined
  const hints = cardHints(task, { state, flagged, currentStep, currentAgent, activity, steps })

  return (
    <div
      className={cn(
        "@container relative min-w-0 overflow-hidden rounded-lg border bg-card transition-colors hover:border-foreground/20 hover:bg-muted/50",
        cardBorder(task, attention),
      )}
    >
      <Link
        to={to}
        replace={to.replace}
        aria-describedby={hints.length > 0 ? hintsId : undefined}
        className="flex h-full flex-col rounded-lg px-2.5 pt-2.5 pb-1.5 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
      >
        <div className="flex items-start gap-2">
          <p
            className={cn(
              "line-clamp-2 min-w-0 flex-1 [overflow-wrap:anywhere] text-sm leading-snug font-medium",
              terminal && "text-muted-foreground line-through decoration-muted-foreground/40",
            )}
          >
            {task.title}
          </p>
          {/* First among the badges: it is the one thing on the card that is
              addressed to the reader. Same pill as the attention strip and
              the sessions table, down to the wording. */}
          {flagged && (
            <SessionAttentionBadge
              size="sm"
              compact
              attention={flagged}
              hintTabIndex={NOT_A_STOP}
            />
          )}
          {state && (
            <StatusBadge
              size="sm"
              label={state.label}
              tone={state.tone}
              hint={state.hint}
              hintTabIndex={NOT_A_STOP}
              compact
            />
          )}
        </div>

        <div className="mt-1.5 flex min-w-0 items-center text-xs text-muted-foreground">
          <StepTrack task={task} steps={steps} />
        </div>

        <div className="mt-auto flex min-w-0 items-center gap-x-2 pt-1 text-xs whitespace-nowrap text-muted-foreground">
          <span className="min-w-0 flex-1 truncate">
            <TokenFigure
              usage={task.usage.total}
              rows={stepUsageRows(task.usage, (step) => stepTitle(steps, step))}
              tabIndex={NOT_A_STOP}
            />
          </span>
          {/* Holds the PR link's place in this row, so the token figure is
              the part that gives way; the real anchor is laid over it below. */}
          {task.pr_url && <PrLinkSlot url={task.pr_url} />}
          <span className="shrink-0">
            <ActivityTime at={activity} hintTabIndex={NOT_A_STOP} />
          </span>
        </div>
      </Link>

      {/* Everything the link says only to a pointer, in one line for whoever
          is reading the link itself. Outside the anchor, so it describes the
          link rather than joining its name. */}
      {hints.length > 0 && (
        <p id={hintsId} className="sr-only">
          {hints.join(". ")}
        </p>
      )}

      {/* The PR link sits between the token figure and the time on row 3. A
          real anchor cannot sit inside the card's link, so it is laid over
          the row: this strip repeats the row's own layout, with the link
          where the row holds its slot and the time as an invisible copy that
          gives the link its place. */}
      {task.pr_url && (
        <div className="pointer-events-none absolute inset-x-2.5 bottom-1.5 flex h-4 items-center gap-x-2 text-xs whitespace-nowrap">
          <span className="min-w-0 flex-1" />
          <PrLink
            url={task.pr_url}
            context={task.title}
            tabIndex={NOT_A_STOP}
            className="pointer-events-auto h-4 shrink-0 text-muted-foreground hover:text-foreground"
          />
          <span aria-hidden className="invisible shrink-0">
            <ActivityTime at={activity} hintTabIndex={NOT_A_STOP} />
          </span>
        </div>
      )}
    </div>
  )
}

/**
 * The one state badge of a card, or none: a task under way with no stall says
 * nothing, because its step track already says where it is.
 */
function cardState(task: TaskDto): CardState | null {
  if (task.stalled) {
    return { label: STALLED_META.label, hint: STALLED_META.hint, tone: STALLED_META.badge }
  }
  const meta = TASK_STATUS_META[task.status]
  switch (task.status) {
    case "pending":
    case "ready":
      return {
        label:
          task.depends_on.length > 0
            ? `Waits for ${plural(task.depends_on.length, "task")}`
            : "Waits",
        hint:
          task.depends_on.length > 0
            ? `Waits for ${plural(task.depends_on.length, "task")} to merge`
            : meta.hint,
        tone: meta.badge,
      }
    case "failed":
      return { label: meta.label, hint: task.reason || meta.hint, tone: meta.badge }
    case "cancelled":
      return { label: meta.label, hint: meta.hint, tone: meta.badge }
    default:
      return null
  }
}

/**
 * The outline the card wears, when it wears one — one colour for however many
 * things are wrong with the task at once.
 *
 * In the order `taskAttentionReason` and `compareByAttention` rank them: an
 * agent waiting on a *person* leads, because it is the only one of the three
 * that says what to do about it; then the failure, which is where the task
 * actually is and which has to stand out in a Pending column full of tasks
 * that have simply not started; then the stall, which is a flag on top of
 * whatever status the task is parked in.
 */
function cardBorder(task: TaskDto, attention?: SessionAttention | null): string | undefined {
  if (["finished", "cancelled", "failed"].includes(task.status))
    return TASK_STATUS_META[task.status].border
  if (attention) return SESSION_ATTENTION_META[attention].border
  return TASK_STATUS_META[task.status].border ?? (task.stalled ? STALLED_META.border : undefined)
}

/**
 * Whether the attention badge would only say again what the card already says.
 *
 * Compared by label rather than by naming the one pair that collides today:
 * two vocabularies that share a word will share it again.
 */
function repeatsCard(attention: SessionAttention, state: CardState | null): boolean {
  return state?.label === SESSION_ATTENTION_META[attention].label
}

/**
 * Every hint on the card, in reading order — the whole of what the tooltips
 * say, since none of them can be focused: the state, the current step, the
 * pull request and when the task was updated.
 */
function cardHints(
  task: TaskDto,
  {
    state,
    flagged,
    currentStep,
    currentAgent,
    activity,
    steps,
  }: {
    activity: string
    steps: WorkflowStepDto[]
    state: CardState | null
    flagged: SessionAttention | null
    currentStep?: WorkflowStepDto
    currentAgent?: TaskDto["agents"][number]
  },
): string[] {
  const hints: string[] = []
  if (flagged) hints.push(SESSION_ATTENTION_META[flagged].hint)
  if (state) hints.push(`${state.label}. ${state.hint}`)
  if (steps.length > 0)
    hints.push(
      `Workflow: ${steps.map((step) => (step.id === currentStep?.id ? `${step.title} (current)` : step.title)).join(", ")}`,
    )
  for (const step of steps) {
    const staffed = task.agents.find((agent) => agent.step === step.id)
    hints.push(
      `Step ${step.title}${step.description ? `: ${step.description}` : ""}; ${
        staffed
          ? `${staffed.model} @ ${staffed.effort ?? "default"}; skills ${staffed.skills.length > 0 ? staffed.skills.join(", ") : "none"}${staffed.session_id ? `; session ${staffed.session_id}` : ""}`
          : "none staffed"
      }`,
    )
  }
  if (currentStep) hints.push(`Current step ${currentStep.title}`)
  if (currentAgent)
    hints.push(
      `Current model ${currentAgent.model}; reasoning effort ${currentAgent.effort ?? "default"}`,
    )
  for (const agent of task.usage.agents) {
    const staffed = task.agents.find((item) => item.id === agent.agent_id)
    hints.push(
      `${staffed?.step ?? agent.step ?? "Workflow"} usage: ${formatTokens(agent.usage.input_tokens)} input, ${formatTokens(agent.usage.output_tokens)} output`,
    )
  }
  hints.push(
    `Task tokens: ${formatTokens(task.usage.total.input_tokens)} input, ${formatTokens(task.usage.total.output_tokens)} output`,
  )
  if (task.pr_url) hints.push("Has a pull request")
  hints.push(whenHint(activity, "last activity"))
  return hints
}

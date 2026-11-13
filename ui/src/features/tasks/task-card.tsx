import { GitPullRequestIcon } from "lucide-react"
import { useId } from "react"
import { Link } from "react-router-dom"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { stepUsageRows } from "@/components/token-figure"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When, whenHint } from "@/components/when"
import {
  SESSION_ATTENTION_META,
  type SessionAttention,
  SessionAttentionBadge,
} from "@/features/sessions/session-display"
import { cachedShare, cn, formatTokens, plural } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { STALLED_META } from "./stalled"
import { TASK_STATUS_META } from "./status"
import { StepTrack } from "./step-track"
import { findStep } from "./steps"

/**
 * The title link describes the state. Usage and PR controls are sibling controls.
 * Every hint is read off the link's description instead ({@link cardHints}).
 */
const NOT_A_STOP = -1

interface CardState {
  label: string
  hint: string
  tone: string
}

export function TaskCard({
  task,
  steps,
  attention,
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
}) {
  const state = cardState(task)
  const terminal = task.status === "cancelled" || task.status === "finished"
  // `to.replace` is set exactly when a goal panel is open — the card's own
  // link is replacing it rather than stacking on it, on the board behind it
  // or in the goal's own task list alike (`routes/paths.ts`'s `taskPanelTo`).
  const to = useTaskPanelTo(task.id)
  // The reason is still what outlines the card even where the badge for it is
  // dropped: it is the same fact, said once.
  const flagged = attention && !repeatsCard(attention, state) ? attention : null
  const hintsId = useId()
  const currentStep = !terminal ? findStep(steps, task.step) : undefined
  const hints = cardHints(task, { state, flagged, currentStep })

  const currentAgent = task.agents.find((agent) => agent.step === task.step)
  const rows = stepUsageRows(task.usage, (id) => findStep(steps, id)?.title ?? id)
  const usageDetails = (
    <div className="space-y-2 text-sm">
      <p>{cachedShare(task.usage.total)} cached input</p>
      {rows.map((row) => (
        <p key={row.key}>
          {row.label}: Input {formatTokens(row.usage.input_tokens)} · Output{" "}
          {formatTokens(row.usage.output_tokens)} · {cachedShare(row.usage)} cached
        </p>
      ))}
    </div>
  )

  return (
    <article
      className={cn(
        "min-w-[300px] rounded-lg border bg-card text-card-foreground",
        cardBorder(task, attention),
      )}
    >
      <Link
        to={to}
        replace={to.replace}
        aria-describedby={hintsId}
        className={cn(
          "block rounded-lg px-4 pt-4 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
          terminal ? "pb-2" : "pb-3",
        )}
      >
        <p className="line-clamp-3 text-base leading-snug font-semibold">{task.title}</p>
        <div className="mt-2 flex flex-wrap items-center gap-2 text-sm">
          {flagged && <SessionAttentionBadge attention={flagged} hintTabIndex={NOT_A_STOP} />}
          {terminal ? (
            <span className="font-semibold">
              {task.status === "finished" ? "Finished" : "Cancelled"}
            </span>
          ) : (
            state && (
              <StatusBadge
                label={state.label}
                tone={state.tone}
                hint={state.hint}
                hintTabIndex={NOT_A_STOP}
              />
            )
          )}
        </div>
        {!terminal && (
          <div className="mt-3 space-y-3 text-sm">
            <StepTrack task={task} steps={steps} />
            {task.step && (
              <p className="break-words">
                Current model: {currentAgent?.model || "None staffed"}
                {currentAgent?.effort ? ` · ${currentAgent.effort}` : ""}
              </p>
            )}
          </div>
        )}
        {task.reason && (task.status === "failed" || task.status === "cancelled") && (
          <p className="mt-2 line-clamp-2 text-sm">{task.reason}</p>
        )}
      </Link>
      <div className="px-4 pb-3 text-sm">
        {terminal ? (
          <div className="flex flex-wrap gap-x-4 tabular-nums">
            <span>Input {formatTokens(task.usage.total.input_tokens)}</span>
            <span>Output {formatTokens(task.usage.total.output_tokens)}</span>
          </div>
        ) : (
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  aria-label="Task token usage"
                  className="flex w-full flex-wrap gap-x-4 gap-y-1 rounded text-left tabular-nums focus-visible:ring-2 focus-visible:ring-ring"
                />
              }
            >
              <span>
                Input {formatTokens(task.usage.total.input_tokens)} ·{" "}
                {cachedShare(task.usage.total)} cached
              </span>
              <span>Output {formatTokens(task.usage.total.output_tokens)}</span>
            </TooltipTrigger>
            <TooltipContent role="tooltip" className="max-w-sm flex-col items-start">
              <p>
                Input {formatTokens(task.usage.total.input_tokens)} · Output{" "}
                {formatTokens(task.usage.total.output_tokens)}
              </p>
              {usageDetails}
            </TooltipContent>
          </Tooltip>
        )}
        {terminal ? (
          <details className="mt-2">
            <summary className="cursor-pointer">Usage details</summary>
            {usageDetails}
            <p className="mt-2">
              Updated <When at={task.updated_at} label="updated" hintTabIndex={NOT_A_STOP} />
            </p>
          </details>
        ) : (
          <p className="mt-2 text-muted-foreground">
            Updated <When at={task.updated_at} label="updated" hintTabIndex={NOT_A_STOP} />
          </p>
        )}
        {task.pr_url && (
          <a
            href={task.pr_url}
            aria-label="Pull request"
            data-pr-link
            className="mt-2 inline-flex items-center gap-1 underline underline-offset-4"
          >
            <GitPullRequestIcon className="size-4" />
            Pull request
          </a>
        )}
      </div>
      <p id={hintsId} className="sr-only">
        {hints.join(". ")}
      </p>
    </article>
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
  }: { state: CardState | null; flagged: SessionAttention | null; currentStep?: WorkflowStepDto },
): string[] {
  const hints: string[] = []
  if (flagged) hints.push(SESSION_ATTENTION_META[flagged].hint)
  if (state) hints.push(`${state.label}. ${state.hint}`)
  if (currentStep) hints.push(`Current step ${currentStep.title}`)
  if (task.pr_url) hints.push("Has a pull request")
  hints.push(whenHint(task.updated_at, "updated"))
  return hints
}

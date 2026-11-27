/**
 * One turn of the turn console: its prompt, one activity line and its
 * answer. A closed turn is these three rows.
 *
 * The activity line gives the time of the turn, its steps, the files it
 * changed and its failures, and opens to the step rows of the turn. While
 * the turn runs, its last steps show under the line, live, and its newest
 * message streams as the answer.
 *
 * What waits on a person and the errors of the turn (`attention`) show
 * whatever the fold: an open question is a card, and an error is a row.
 * Where the daemon holds more of them than `attention` carries, one row
 * gives the count and opens the steps of the turn. A pending permission is
 * not drawn here: it is a card above the composer.
 *
 * A warning and a compaction always show as lines of the turn, and an info
 * note is a row of the open fold alone (`notices.tsx`). An answered
 * permission is a mark on the row of its call, and no row of its own where
 * that row shows. A finished plan shows in the turn that last changed it.
 */

import { BotIcon, ChevronDownIcon, ChevronRightIcon, CircleHelpIcon } from "lucide-react"
import { memo } from "react"

import type { Host, QuestionDto, StepDto, TurnDto } from "@/api"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { cn, formatClock } from "@/lib/format"

import { PromptBox } from "../console/blocks"
import { elapsed } from "../console/elapsed"
import { FormCard } from "../console/form-card"
import { formQuestions } from "../console/form-questions"
import {
  answeredPermissions,
  isInfoNote,
  isNotice,
  NoticeLine,
  PermissionMark,
  type PermissionStep,
  permissionOf,
} from "./notices"
import { type Plan, PlanLine, planFoldId } from "./plan"
import { StepDetail } from "./step-detail"
import { contentText, foldSteps, type StepPage } from "./store"

/** How many of its last steps a running turn shows under its activity line. */
const LIVE_STEPS = 3

type QuestionStep = Extract<StepDto, { type: "question" }>

/** Recolours the marker of the shared question card with the app's token, in both themes. */
const QUESTION_MARKER = "[&_.text-purple-500]:text-question-fg"

/** Where the load of a turn's steps stands. */
export type PageLoad = { state: "idle" | "loading" } | { state: "error"; message: string }

interface TurnViewProps {
  host: Host
  sessionId: string
  turn: TurnDto
  /** The steps the steps route gave for the open fold. */
  page?: StepPage
  load: PageLoad
  open: boolean
  live: boolean
  /** Now, as an ISO instant, for the clock of what still runs. */
  now: string
  /** The session's open questions, with their forms. */
  questions: readonly QuestionDto[]
  /** The notices the console read from the steps before the turn's page. */
  notices: readonly StepDto[]
  /** The finished plans that this turn last changed. */
  plans: readonly Plan[]
  /** The open folds of the console, by id: a plan's checklist among them. */
  folds: Readonly<Record<string, boolean>>
  /** Open or close the fold of a turn, or of a plan's checklist, by its id. */
  onToggle: (id: string) => void
  onEarlier: (turnId: string) => void
}

export const TurnView = memo(function TurnView({
  host,
  sessionId,
  turn,
  page,
  load,
  open,
  live,
  now,
  questions,
  notices: older,
  plans,
  folds,
  onToggle,
  onEarlier,
}: TurnViewProps) {
  const running = turn.state === "running"
  // A pending permission is the card above the composer, never a row too.
  const fold = foldSteps(turn, page)
  const answered = answeredPermissions([...fold.steps, ...turn.attention])
  // The notices are lines of their own, and an answered permission is the
  // mark of its call's row where that row shows.
  const calls = new Set(fold.steps.flatMap((step) => (step.type === "tool_call" ? [step.id] : [])))
  const marked = (step: StepDto, shown: ReadonlySet<string>) =>
    step.type === "permission" &&
    fold.steps.some(
      (call) =>
        call.type === "tool_call" && shown.has(call.id) && permissionOf(call, answered) === step,
    )
  const steps = open
    ? fold.steps.filter((step) => !isPending(step) && !isNotice(step) && !marked(step, calls))
    : []
  const answer = running ? liveAnswer(turn) : contentText(turn.answer)
  const shownIds = new Set(open ? steps.map((step) => step.id) : [])
  const liveRows =
    running && !open
      ? foldSteps(turn, undefined).steps.filter(
          (step) =>
            step.type !== "message" && !isPending(step) && !isNotice(step) && !isInfoNote(step),
        )
      : []
  const lastCalls = new Set(liveRows.slice(-LIVE_STEPS).map((step) => step.id))
  const lastRows = liveRows.slice(-LIVE_STEPS).filter((step) => !marked(step, lastCalls))
  for (const step of lastRows) shownIds.add(step.id)
  const errors = turn.attention.filter((step) => step.type === "error" && !shownIds.has(step.id))
  const held = new Set(fold.steps.map((step) => step.id))
  const notices = [
    ...older.filter(isNotice).filter((step) => !held.has(step.id)),
    ...fold.steps.filter(isNotice),
  ]
  const asked = turn.attention.filter(
    (step): step is QuestionStep => step.type === "question" && step.state === "open",
  )
  const earlier = fold.earlier
  return (
    <article
      data-turn-id={turn.id}
      data-block-id={turn.id}
      aria-label={`Turn ${formatClock(turn.started_at)}`}
      className="space-y-1"
    >
      {turn.prompt ? (
        <PromptBox
          text={contentText(turn.prompt.content)}
          time={formatClock(turn.started_at)}
          role={turn.prompt.source === "daemon" ? "Developer" : "You"}
        />
      ) : null}
      <ActivityLine turn={turn} now={now} open={open} onToggle={() => onToggle(turn.id)} />
      {open ? (
        <div className="ml-4 space-y-0.5">
          {earlier > 0 && page ? (
            <Button
              size="xs"
              variant="ghost"
              disabled={load.state === "loading"}
              onClick={() => onEarlier(turn.id)}
              className="font-mono text-muted-foreground"
            >
              Show {earlier} earlier
            </Button>
          ) : null}
          <ul aria-label="Steps of the turn" className="space-y-0.5">
            {steps.map((step) => (
              <li key={step.id}>
                <StepRow step={step} now={now} permission={markOf(step, answered)} />
                <StepDetail host={host} sessionId={sessionId} step={step} />
              </li>
            ))}
          </ul>
          <LoadLine load={load} onRetry={() => onEarlier(turn.id)} />
        </div>
      ) : lastRows.length > 0 ? (
        <ul aria-label="Last steps of the turn" className="ml-4 space-y-0.5">
          {lastRows.map((step) => (
            <li key={step.id}>
              <StepRow step={step} now={now} permission={markOf(step, answered)} />
            </li>
          ))}
        </ul>
      ) : null}
      {errors.map((step) => (
        <div key={step.id} className="ml-4">
          <StepRow step={step} now={now} />
        </div>
      ))}
      {notices.map((step) => (
        <NoticeLine key={step.id} step={step} />
      ))}
      {turn.attention_more > 0 ? (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => onToggle(turn.id)}
          className="ml-4 flex w-[calc(100%-1rem)] cursor-pointer items-center rounded-md px-2 py-1 text-left font-mono text-xs text-status-warn-fg outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          {turn.attention_more} more {turn.attention_more === 1 ? "needs" : "need"} attention
        </button>
      ) : null}
      {asked.map((step) => (
        <QuestionCard
          key={step.id}
          host={host}
          sessionId={sessionId}
          step={step}
          live={live}
          questions={questions}
        />
      ))}
      {plans.map((plan) => (
        <div key={plan.plan_id} className="ml-4">
          <PlanLine
            plan={plan}
            open={folds[planFoldId(plan)] === true}
            onToggle={() => onToggle(planFoldId(plan))}
          />
        </div>
      ))}
      {answer ? (
        <div className="rounded-md bg-muted/30 px-2 py-1.5 ring-1 ring-inset ring-border/40">
          <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold">
            <BotIcon aria-hidden className="size-3.5 shrink-0" />
            Assistant
          </div>
          <Markdown>{answer}</Markdown>
        </div>
      ) : null}
    </article>
  )
})

/** The answered permission of a call's row, for its mark. */
function markOf(
  step: StepDto,
  answered: ReadonlyMap<string, PermissionStep>,
): PermissionStep | undefined {
  return step.type === "tool_call" ? permissionOf(step, answered) : undefined
}

function isPending(step: StepDto): boolean {
  return step.type === "permission" && step.state === "pending"
}

/** The newest message of the turn's own lane while it runs, else the answer so far. */
function liveAnswer(turn: TurnDto): string {
  const message = turn.steps.findLast((step) => step.type === "message" && step.lane === turn.lane)
  return message?.type === "message" ? contentText(message.content) : contentText(turn.answer)
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`
}

function ActivityLine({
  turn,
  now,
  open,
  onToggle,
}: {
  turn: TurnDto
  now: string
  open: boolean
  onToggle: () => void
}) {
  const time = elapsed(turn.started_at, turn.ended_at ?? now) ?? "0s"
  const { steps, files_changed: files, failed } = turn.activity
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon
  return (
    <button
      type="button"
      aria-expanded={open}
      onClick={onToggle}
      className="ml-4 flex w-[calc(100%-1rem)] min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left font-mono text-xs text-muted-foreground outline-none hover:bg-muted/60 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
    >
      <Chevron aria-hidden className="size-3.5 shrink-0" />
      <span>{time}</span>
      <span aria-hidden>·</span>
      <span>{plural(steps, "step", "steps")}</span>
      <span aria-hidden>·</span>
      <span>{plural(files, "file changed", "files changed")}</span>
      <span aria-hidden>·</span>
      <span className={cn(failed > 0 && "text-status-danger-fg")}>{failed} failed</span>
      {turn.state === "running" ? null : turn.state === "completed" ? null : (
        <span className="ml-auto">{turn.state}</span>
      )}
    </button>
  )
}

function LoadLine({ load, onRetry }: { load: PageLoad; onRetry: () => void }) {
  if (load.state === "loading")
    return <p className="px-2 py-1 font-mono text-xs text-muted-foreground">Loading the steps…</p>
  if (load.state === "error")
    return (
      <p
        role="alert"
        className="flex items-center gap-2 px-2 py-1 font-mono text-xs text-status-danger-fg"
      >
        Could not load the steps: {load.message}
        <Button size="xs" variant="outline" onClick={onRetry}>
          Retry
        </Button>
      </p>
    )
  return null
}

/** What a row of a step says: its kind, its status, its title, its subject and its duration. */
function stepFacts(
  step: StepDto,
  now: string,
): { kind: string; status: string; title: string; subject?: string; duration?: string } {
  switch (step.type) {
    case "tool_call":
      return {
        kind: step.kind ?? "tool",
        status: step.status,
        title: step.title,
        subject: step.subject && step.subject !== step.title ? firstLine(step.subject) : undefined,
        duration: elapsed(step.started_at, step.ended_at ?? now),
      }
    case "message":
    case "thought":
      return {
        kind: step.type,
        status: step.streaming ? "running" : "completed",
        title: firstLine(contentText(step.content)),
      }
    case "permission":
      return {
        kind: "permission",
        status: step.answer ? `${step.state} · ${step.answer.label}` : step.state,
        title: step.tool.title,
        subject: step.tool.subject ? firstLine(step.tool.subject) : undefined,
      }
    case "question":
      return {
        kind: "question",
        status: step.state,
        title: firstLine(step.question),
        subject: step.hint || undefined,
      }
    case "note":
      return { kind: "note", status: step.severity, title: firstLine(step.text) }
    case "error":
      return {
        kind: "error",
        status: "failed",
        title: firstLine(step.text),
        subject: step.diagnosis ? firstLine(step.diagnosis) : undefined,
      }
    case "subagent":
      return {
        kind: "subagent",
        status: step.state ?? "running",
        title: step.title,
        subject: step.task ? firstLine(step.task) : undefined,
        duration: elapsed(step.started_at, step.ended_at ?? now),
      }
    case "compaction":
      return {
        kind: "compaction",
        status: step.status,
        title: "compaction",
        subject: step.error ?? undefined,
      }
  }
}

function firstLine(text: string): string {
  return (
    text
      .split("\n")
      .find((line) => line.trim().length > 0)
      ?.trim() ?? ""
  )
}

const STATUS_TONES: Record<string, string> = {
  running: "text-status-active-fg",
  pending: "text-status-warn-fg",
  open: "text-status-warn-fg",
  warning: "text-status-warn-fg",
  failed: "text-status-danger-fg",
  interrupted: "text-status-danger-fg",
  completed: "text-status-done-fg",
}

/** One step as a row: the kind, the status, the title, the subject and the duration. */
function StepRow({
  step,
  now,
  permission,
}: {
  step: StepDto
  now: string
  /** The answered permission of a call, drawn as a mark on its row. */
  permission?: PermissionStep
}) {
  const facts = stepFacts(step, now)
  const tone = STATUS_TONES[facts.status.split(" ")[0] ?? ""] ?? "text-muted-foreground"
  return (
    <div
      data-step-id={step.id}
      data-step-kind={step.type}
      title={formatClock(step.created_at)}
      className="flex min-w-0 items-baseline gap-2 rounded-md px-2 py-0.5 font-mono text-xs"
    >
      <span className="shrink-0 text-muted-foreground">{facts.kind}</span>
      <span className={cn("shrink-0", tone)}>{facts.status}</span>
      <span className="min-w-0 truncate text-foreground">{facts.title}</span>
      {facts.subject ? (
        <span className="min-w-0 truncate text-muted-foreground">{facts.subject}</span>
      ) : null}
      {permission ? <PermissionMark step={permission} /> : null}
      {facts.duration ? (
        <span className="ml-auto shrink-0 tabular-nums text-muted-foreground">
          {facts.duration}
        </span>
      ) : null}
    </div>
  )
}

/**
 * An open question of the turn. With its form from the session's open
 * questions it is the card of today, which answers on the question's own
 * route. Until the form is read it shows the question and its choices.
 */
function QuestionCard({
  host,
  sessionId,
  step,
  live,
  questions,
}: {
  host: Host
  sessionId: string
  step: QuestionStep
  live: boolean
  questions: readonly QuestionDto[]
}) {
  const listed = questions.find((one) => one.id === step.request_id)
  const form = listed ? formQuestions([], [listed])[0] : undefined
  // The card is the console's of today, which draws its marker in a fixed
  // colour: here the marker takes the app's question token instead.
  if (form)
    return (
      <div className={QUESTION_MARKER}>
        <FormCard host={host} sessionId={sessionId} question={form} live={live} />
      </div>
    )
  return (
    <section
      aria-label="Question"
      data-step-id={step.id}
      className="rounded-md bg-muted/40 px-3 py-2 text-sm ring-1 ring-inset ring-border/60"
    >
      <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold">
        <CircleHelpIcon aria-hidden className="size-3.5 shrink-0" />
        {step.hint || "Question"}
      </div>
      <Markdown>{step.question}</Markdown>
      {step.options.length > 0 ? (
        <ul className="mt-1 list-disc pl-5 text-xs text-muted-foreground">
          {step.options.map((option) => (
            <li key={option.id}>{option.label}</li>
          ))}
        </ul>
      ) : null}
    </section>
  )
}

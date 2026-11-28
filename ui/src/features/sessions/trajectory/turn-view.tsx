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
import { Fragment, memo, type ReactNode, useState } from "react"

import type { ContentBlockDto, Host, QuestionDto, StepDto, TurnDto } from "@/api"
import { Markdown } from "@/components/markdown"
import { NamedIcon } from "@/components/named-icon"
import { Button } from "@/components/ui/button"
import { cn, formatClock } from "@/lib/format"

import { PromptBox } from "../console/blocks"
import { elapsed } from "../console/elapsed"
import { FormCard } from "../console/form-card"
import { formQuestions } from "../console/form-questions"
import { MediaBlocks } from "./content-blocks"
import {
  answeredPermissions,
  isInfoNote,
  isNotice,
  NoticeLine,
  type PermissionStep,
  permissionOf,
} from "./notices"
import { type Plan, PlanLine, planFoldId } from "./plan"
import { StepDetail } from "./step-detail"
import { groupSteps, type ToolStep } from "./step-groups"
import { StateIcon } from "./step-icons"
import { GroupRow } from "./step-row"
import { contentText, foldSteps, promptText, type StepPage } from "./store"

/** How many of its last steps a running turn shows under its activity line. */
const LIVE_STEPS = 3

type QuestionStep = Extract<StepDto, { type: "question" }>

/** Recolours the marker of the shared question card with the app's token, in both themes. */
const QUESTION_MARKER = "[&_.text-purple-500]:text-question-fg"

/** Where the load of a turn's steps stands. */
export type PageLoad =
  | { state: "idle" | "loading" }
  | { state: "error"; message: string; retry: () => void }

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
    ? fold.steps.filter(
        (step) =>
          !isPending(step) &&
          !isNotice(step) &&
          !marked(step, calls) &&
          step.id !== turn.answer_step_id,
      )
    : []
  const blocks = running ? liveBlocks(turn) : turn.answer
  const answer = contentText(blocks)
  const media = blocks.some((block) => block.type !== "text")
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
  // The turn's own notices, and the ones among the steps the console has read:
  // the opened steps hold the notices `notices_more` counts.
  const held = new Set(turn.notices.map((step) => step.id))
  // A closed fold shows the turn's own notices only: a page it loaded is
  // dropped when the pane mounts again, so its rows would not survive.
  const read = open ? fold.steps.filter((step) => !held.has(step.id)) : []
  const notices = [...turn.notices, ...read]
    .filter(isNotice)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id))
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
          text={promptText(turn.prompt)}
          time={formatClock(turn.started_at)}
          role={turn.prompt.source === "daemon" ? "Developer" : "You"}
          fold={
            turn.prompt.source === "daemon"
              ? {
                  open: folds[promptFoldId(turn.prompt.id)] === true,
                  onToggle: () => onToggle(promptFoldId(turn.prompt?.id ?? turn.id)),
                }
              : undefined
          }
        />
      ) : null}
      <ActivityLine turn={turn} now={now} open={open} onToggle={() => onToggle(turn.id)} />
      {open ? (
        <div className="ml-4 space-y-0.5">
          {earlier > 0 && page ? (
            <Button
              size="xs"
              variant="ghost"
              disabled={load.state !== "idle"}
              onClick={() => onEarlier(turn.id)}
              className="font-mono text-muted-foreground"
            >
              Show {earlier} earlier
            </Button>
          ) : null}
          <StepRows
            label="Steps of the turn"
            host={host}
            sessionId={sessionId}
            steps={steps}
            now={now}
            answered={answered}
          />
          <LoadLine load={load} />
        </div>
      ) : lastRows.length > 0 ? (
        <div className="ml-4">
          <StepRows
            label="Last steps of the turn"
            host={host}
            sessionId={sessionId}
            steps={lastRows}
            now={now}
            answered={answered}
          />
        </div>
      ) : null}
      {!open && load.state === "error" ? <LoadLine load={load} /> : null}
      {errors.map((step) => (
        <div key={step.id} className="ml-4">
          <StepDetail host={host} sessionId={sessionId} step={step} now={now} />
        </div>
      ))}
      {notices.map((step) => (
        <NoticeLine key={step.id} step={step} />
      ))}
      {turn.notices_more > 0 ? (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => onToggle(turn.id)}
          className="ml-4 flex w-[calc(100%-1rem)] cursor-pointer items-center rounded-md px-2 py-1 text-left font-mono text-xs text-muted-foreground outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          {turn.notices_more} more {turn.notices_more === 1 ? "notice" : "notices"}
        </button>
      ) : null}
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
      {answer || media ? (
        <div className="rounded-md bg-muted/30 px-2 py-1.5 ring-1 ring-inset ring-border/40">
          <NamedIcon icon={BotIcon} label="Assistant" focusable className="mb-1 text-foreground" />
          <Markdown>{answer}</Markdown>
          <MediaBlocks host={host} blocks={blocks} />
        </div>
      ) : null}
    </article>
  )
})

/**
 * The rows of the steps shown, with each run of read and search calls folded
 * into one group row (`step-groups.ts`).
 */
function StepRows({
  label,
  host,
  sessionId,
  steps,
  now,
  answered,
}: {
  label: string
  host: Host
  sessionId: string
  steps: readonly StepDto[]
  now: string
  answered: ReadonlyMap<string, PermissionStep>
}) {
  const row = (step: StepDto) => (
    <li key={step.id}>
      <StepDetail
        host={host}
        sessionId={sessionId}
        step={step}
        now={now}
        permission={markOf(step, answered)}
      />
    </li>
  )
  return (
    <ul aria-label={label} className="space-y-0.5">
      {groupSteps(steps).map((entry) =>
        entry.type === "step" ? (
          row(entry.step)
        ) : (
          // A group keeps its first call's id, so it stays open as calls join it.
          <li key={entry.steps[0]?.id}>
            <StepGroup steps={entry.steps}>{entry.steps.map(row)}</StepGroup>
          </li>
        ),
      )}
    </ul>
  )
}

/** A group row, and the rows of its calls while it is open. */
function StepGroup({ steps, children }: { steps: readonly ToolStep[]; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div>
      <GroupRow steps={steps} open={open} onToggle={() => setOpen((was) => !was)} />
      {open ? (
        <ul aria-label="Steps of the group" className="mt-0.5 ml-4 space-y-0.5">
          {children}
        </ul>
      ) : null}
    </div>
  )
}

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

/** While the turn runs, the content of the newest message of the turn's own lane, else the answer so far. */
function liveBlocks(turn: TurnDto): ContentBlockDto[] {
  const message = turn.steps.findLast((step) => step.type === "message" && step.lane === turn.lane)
  return message?.type === "message" ? message.content : turn.answer
}

/** The fold id of a daemon prompt, beside the turns' and the plans' in the console's kept folds. */
function promptFoldId(promptId: string): string {
  return `prompt:${promptId}`
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
  const { steps, files_changed: files, failed, cancelled, subagents } = turn.activity
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon
  // The steps and each count above zero: a quiet turn is its duration alone.
  const facts = [
    steps > 0 ? { text: plural(steps, "step", "steps") } : null,
    files > 0 ? { text: plural(files, "file changed", "files changed") } : null,
    failed > 0 ? { text: `${failed} failed`, danger: true } : null,
    cancelled > 0 ? { text: `${cancelled} cancelled` } : null,
    subagents > 0 ? { text: plural(subagents, "subagent", "subagents") } : null,
  ].filter((fact) => fact !== null)
  return (
    <button
      type="button"
      aria-expanded={open}
      onClick={onToggle}
      className="ml-4 flex w-[calc(100%-1rem)] min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left font-mono text-xs text-muted-foreground outline-none hover:bg-muted/60 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
    >
      <Chevron aria-hidden className="size-3.5 shrink-0" />
      <StateIcon state={turn.state} /> <span>{time}</span>
      {facts.map((fact) => (
        <Fragment key={fact.text}>
          <span aria-hidden>·</span>
          <span className={cn(fact.danger && "text-status-danger-fg")}>{fact.text}</span>
        </Fragment>
      ))}
    </button>
  )
}

function LoadLine({ load }: { load: PageLoad }) {
  if (load.state === "loading")
    return <p className="px-2 py-1 font-mono text-xs text-muted-foreground">Loading the steps…</p>
  if (load.state === "error")
    return (
      <p
        role="alert"
        className="flex items-center gap-2 px-2 py-1 font-mono text-xs text-status-danger-fg"
      >
        Could not load the steps: {load.message}
        <Button size="xs" variant="outline" onClick={load.retry}>
          Retry
        </Button>
      </p>
    )
  return null
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

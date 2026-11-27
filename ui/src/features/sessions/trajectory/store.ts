/**
 * The state of the turn console: what the daemon's trajectory (040) says,
 * held as it says it.
 *
 * The daemon folds the events into turns and steps, so nothing here folds an
 * event. `applySnapshot` replaces the state with a snapshot, and
 * `applyUpdate` applies the ops of one revision: a turn, a step, streamed
 * text, a removal, a plan, a subagent or the session's own state, each by its
 * id, so no item shows two times. A snapshot, the one a reconnect brings
 * included, drops the older turns and the step pages loaded before it.
 *
 * Two things are the console's own: the step pages it loaded for a turn
 * whose fold is open, and the prompts it sent that no turn holds yet.
 */

import type {
  ContentBlockDto,
  StepDto,
  TrajectorySnapshotDto,
  TrajectoryStepsDto,
  TrajectoryUpdateDto,
  TurnDto,
} from "@/api"

type SessionState = TrajectorySnapshotDto["session"]
type Plan = TrajectorySnapshotDto["plans"][number]
type Subagent = TrajectorySnapshotDto["subagents"][number]

/** The steps of one turn that the steps route gave, oldest first. */
export interface StepPage {
  steps: StepDto[]
  /** How many steps of the turn come before `steps`. */
  earlier: number
}

/** A prompt the console sent that no turn holds yet. */
interface QueuedPrompt {
  id: string
  text: string
  /** The newest turn id when it was sent: only a newer turn holds it. */
  after: string
}

export interface TrajectoryState {
  rev: number
  session?: SessionState
  /** Oldest first. */
  turns: TurnDto[]
  /** Whether the daemon holds turns older than the oldest one here. */
  hasOlder: boolean
  plans: Plan[]
  subagents: Subagent[]
  /** The steps the steps route gave, by turn id. */
  pages: Record<string, StepPage>
  queued: QueuedPrompt[]
}

export function emptyTrajectory(): TrajectoryState {
  return { rev: 0, turns: [], hasOlder: false, plans: [], subagents: [], pages: {}, queued: [] }
}

/** The text of a list of content blocks: the text blocks, one after another. */
export function contentText(blocks: readonly ContentBlockDto[]): string {
  return blocks
    .map((block) => (block.type === "text" ? block.text : ""))
    .filter((text) => text.length > 0)
    .join("")
}

/** Replace the state with what a snapshot holds. The prompts still queued stay. */
export function applySnapshot(
  state: TrajectoryState,
  snapshot: TrajectorySnapshotDto,
): TrajectoryState {
  return settle({
    rev: snapshot.rev,
    session: snapshot.session,
    turns: snapshot.turns,
    hasOlder: snapshot.has_older,
    plans: snapshot.plans,
    subagents: snapshot.subagents,
    pages: {},
    queued: state.queued,
  })
}

/** Apply the ops of one revision. A revision already applied changes nothing. */
export function applyUpdate(state: TrajectoryState, update: TrajectoryUpdateDto): TrajectoryState {
  if (update.rev <= state.rev) return state
  let next: TrajectoryState = { ...state, rev: update.rev }
  for (const op of update.ops) {
    switch (op.op) {
      case "turn_upsert":
        next = upsertTurn(next, op.turn)
        break
      case "item_upsert":
        next = changeSteps(next, op.step.turn_id, (steps) => upsertStep(steps, op.step), true)
        break
      case "text_append":
        next = changeSteps(next, op.turn_id, (steps) =>
          steps.map((step) => (step.id === op.item_id ? appendText(step, op.text) : step)),
        )
        break
      case "item_remove":
        next = changeSteps(next, op.turn_id, (steps) =>
          steps.filter((step) => step.id !== op.item_id),
        )
        break
      case "plan_upsert":
        next = {
          ...next,
          plans: replaceBy(next.plans, op.plan, (plan) => plan.plan_id === op.plan.plan_id),
        }
        break
      case "subagent_upsert":
        next = {
          ...next,
          subagents: replaceBy(next.subagents, op.subagent, (one) => one.id === op.subagent.id),
        }
        break
      case "session_patch":
        next = { ...next, session: op.session }
        break
    }
  }
  return settle(next)
}

/** Put a page of older whole turns in front of the turns held. */
export function prependTurns(state: TrajectoryState, page: TrajectorySnapshotDto): TrajectoryState {
  const held = new Set(state.turns.map((turn) => turn.id))
  return {
    ...state,
    turns: [...page.turns.filter((turn) => !held.has(turn.id)), ...state.turns],
    hasOlder: page.has_older,
  }
}

/**
 * Keep a page of a turn's steps. With `before`, the page is the steps older
 * than the ones held, and goes in front of them.
 */
export function addStepPage(
  state: TrajectoryState,
  turnId: string,
  page: TrajectoryStepsDto,
  before?: string,
): TrajectoryState {
  const held = state.pages[turnId]
  const steps =
    before !== undefined && held
      ? [
          ...page.steps.filter((step) => !held.steps.some((one) => one.id === step.id)),
          ...held.steps,
        ]
      : page.steps
  return { ...state, pages: { ...state.pages, [turnId]: { steps, earlier: page.earlier_steps } } }
}

export function queuePrompt(state: TrajectoryState, text: string, id: string): TrajectoryState {
  const after = state.turns.at(-1)?.id ?? ""
  return { ...state, queued: [...state.queued, { id, text, after }] }
}

/** Take back the prompt of the send `id`, which the daemon refused. */
export function dequeuePrompt(state: TrajectoryState, id: string): TrajectoryState {
  return { ...state, queued: state.queued.filter((one) => one.id !== id) }
}

/** The steps a turn shows when its fold is open: the page loaded, then the live ones after it. */
export function turnSteps(turn: TurnDto, page: StepPage | undefined): StepDto[] {
  if (!page) return turn.steps
  const held = new Set(page.steps.map((step) => step.id))
  return [...page.steps, ...turn.steps.filter((step) => !held.has(step.id))]
}

/** The permissions that wait for an answer, oldest first, from the attention of every turn. */
export function pendingPermissions(state: TrajectoryState) {
  return state.turns.flatMap((turn) =>
    turn.attention
      .filter(
        (step): step is Extract<StepDto, { type: "permission" }> =>
          step.type === "permission" && step.state === "pending",
      )
      .reverse(),
  )
}

/** A queued prompt leaves the queue once a newer turn holds its text. */
function settle(state: TrajectoryState): TrajectoryState {
  if (state.queued.length === 0) return state
  const taken = new Set<string>()
  const queued = state.queued.filter((one) => {
    const turn = state.turns.find(
      (turn) =>
        turn.id > one.after &&
        !taken.has(turn.id) &&
        turn.prompt?.source === "console" &&
        contentText(turn.prompt.content) === one.text,
    )
    if (!turn) return true
    taken.add(turn.id)
    return false
  })
  return queued.length === state.queued.length ? state : { ...state, queued }
}

function upsertTurn(state: TrajectoryState, turn: TurnDto): TrajectoryState {
  const at = state.turns.findIndex((one) => one.id === turn.id)
  if (at === -1) {
    const turns = [...state.turns, { ...turn }].sort((a, b) => (a.id < b.id ? -1 : 1))
    return { ...state, turns }
  }
  const held = state.turns[at] as TurnDto
  // An upsert carries the turn with no steps: the live ones stay while it runs.
  const steps = turn.state === "running" ? held.steps : []
  const turns = state.turns.slice()
  turns[at] = { ...turn, steps }
  return { ...state, turns }
}

/**
 * Change the steps of one turn wherever they are held: its live window, its
 * attention and its loaded page. With `open`, a step new to the window joins
 * it while the turn runs; the attention and a page only change the steps
 * they hold, since the turn's own upsert says what its attention is.
 */
function changeSteps(
  state: TrajectoryState,
  turnId: string,
  change: (steps: StepDto[]) => StepDto[],
  open = false,
): TrajectoryState {
  const at = state.turns.findIndex((turn) => turn.id === turnId)
  if (at === -1) return state
  const turn = state.turns[at] as TurnDto
  const turns = state.turns.slice()
  turns[at] = {
    ...turn,
    steps: open && turn.state !== "running" ? turn.steps : change(turn.steps),
    attention: changeHeld(turn.attention, change),
  }
  const page = state.pages[turnId]
  const pages = page
    ? { ...state.pages, [turnId]: { ...page, steps: changeHeld(page.steps, change) } }
    : state.pages
  return { ...state, turns, pages }
}

/** The change of the steps a list holds, with no step added to it. */
function changeHeld(steps: StepDto[], change: (steps: StepDto[]) => StepDto[]): StepDto[] {
  const held = new Set(steps.map((step) => step.id))
  return change(steps).filter((step) => held.has(step.id))
}

function upsertStep(steps: StepDto[], step: StepDto): StepDto[] {
  return replaceBy(steps, step, (one) => one.id === step.id)
}

function replaceBy<T>(list: T[], item: T, same: (one: T) => boolean): T[] {
  const at = list.findIndex(same)
  if (at === -1) return [...list, item]
  const next = list.slice()
  next[at] = item
  return next
}

function appendText(step: StepDto, text: string): StepDto {
  if (step.type !== "message" && step.type !== "thought") return step
  const content = step.content.slice()
  const last = content.at(-1)
  if (last?.type === "text") content[content.length - 1] = { ...last, text: last.text + text }
  else content.push({ type: "text", text })
  return { ...step, content }
}

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
 * Six things are the console's own: step pages, notices, live subagent steps,
 * queued prompts, echoed turns and the history cursor.
 *
 * A turn keeps each step of its own lane that an update brought, after it
 * ends too, and that live version wins over a page that the steps route
 * read before it. So a row seen live never leaves an open fold, and a late
 * page never brings back an older status.
 *
 * The turns shown are the history read whole: the snapshot's and the older
 * pages', from `cursor` on. A turn that the stream brings from before the
 * cursor, such as an old turn whose error a diagnosis names, is held but not
 * shown, and the next older page still starts at the cursor.
 */

import type {
  ContentBlockDto,
  StepDto,
  TrajectorySnapshotDto,
  TrajectoryStepsDto,
  TrajectoryUpdateDto,
  TurnDto,
} from "@/api"

/** The lane of the session's own agent (040). A subagent's lane is its id. */
const MAIN_LANE = "main"

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
  /** Oldest first, the ones held before `cursor` included. */
  turns: TurnDto[]
  /** The oldest turn of the history read whole: an older page reads before it. */
  cursor?: string
  /** Whether the daemon holds turns older than the oldest one here. */
  hasOlder: boolean
  plans: Plan[]
  subagents: Subagent[]
  /** The steps the steps route gave, by turn id. */
  pages: Record<string, StepPage>
  /**
   * The steps of a subagent's lane that an update brought, by step id: no
   * turn holds them, and an open subagent step shows their live version.
   */
  children: Record<string, StepDto>
  /** The notices of the steps before a turn's page, by turn id, oldest first. */
  notices: Record<string, StepDto[]>
  queued: QueuedPrompt[]
  /** The turns that took a queued prompt away: each echo takes one prompt, once. */
  echoed: string[]
}

export function emptyTrajectory(): TrajectoryState {
  return {
    rev: 0,
    turns: [],
    hasOlder: false,
    plans: [],
    subagents: [],
    pages: {},
    children: {},
    notices: {},
    queued: [],
    echoed: [],
  }
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
    cursor: snapshot.turns[0]?.id,
    hasOlder: snapshot.has_older,
    plans: snapshot.plans,
    subagents: snapshot.subagents,
    pages: {},
    children: {},
    notices: {},
    queued: state.queued,
    echoed: state.echoed,
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
        next = changeSteps(next, op.step.turn_id, (steps) => upsertStep(steps, op.step), op.step)
        next = keepChild(next, op.step)
        break
      case "text_append": {
        next = changeSteps(next, op.turn_id, (steps) =>
          steps.map((step) => (step.id === op.item_id ? appendText(step, op.text) : step)),
        )
        const child = next.children[op.item_id]
        if (child)
          next = { ...next, children: { ...next.children, [child.id]: appendText(child, op.text) } }
        break
      }
      case "item_remove":
        next = changeSteps(next, op.turn_id, (steps) =>
          steps.filter((step) => step.id !== op.item_id),
        )
        if (next.children[op.item_id]) {
          const { [op.item_id]: _gone, ...children } = next.children
          next = { ...next, children }
        }
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

/**
 * Put a page of older whole turns in front of the history read whole. A turn
 * the stream already brought keeps its live state and attention, and takes
 * the prompt and answer of the page where the stream's header had none.
 */
export function prependTurns(state: TrajectoryState, page: TrajectorySnapshotDto): TrajectoryState {
  const held = new Map(state.turns.map((turn) => [turn.id, turn]))
  const paged = new Set(page.turns.map((turn) => turn.id))
  const merged = page.turns.map((turn) => {
    const live = held.get(turn.id)
    return live ? mergeTurn(turn, live) : turn
  })
  return {
    ...state,
    turns: [...merged, ...state.turns.filter((turn) => !paged.has(turn.id))].sort(byId),
    cursor: page.turns[0]?.id ?? state.cursor,
    hasOlder: page.has_older,
  }
}

/** The turns a reader sees: the history read whole, from the cursor on. */
export function shownTurns(state: TrajectoryState): TurnDto[] {
  const { cursor } = state
  return cursor === undefined ? state.turns : state.turns.filter((turn) => turn.id >= cursor)
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

/** Keep the notices of an older page of a turn's steps, in front of the ones held. */
export function addNotices(
  state: TrajectoryState,
  turnId: string,
  steps: readonly StepDto[],
): TrajectoryState {
  if (steps.length === 0) return state
  const held = state.notices[turnId] ?? []
  const fresh = steps.filter((step) => !held.some((one) => one.id === step.id))
  return { ...state, notices: { ...state.notices, [turnId]: [...fresh, ...held] } }
}

export function queuePrompt(state: TrajectoryState, text: string, id: string): TrajectoryState {
  const after = state.turns.at(-1)?.id ?? ""
  return { ...state, queued: [...state.queued, { id, text, after }] }
}

/**
 * Take back the prompt of the send `id`, which the daemon refused. Where an
 * echo of the same text took that entry already, the echo was another
 * send's, so the oldest entry of that text left is the one that is stale.
 */
export function dequeuePrompt(state: TrajectoryState, id: string, text: string): TrajectoryState {
  const found = state.queued.findIndex((one) => one.id === id)
  const index = found === -1 ? state.queued.findIndex((one) => one.text === text) : found
  if (index === -1) return state
  return { ...state, queued: state.queued.filter((_, at) => at !== index) }
}

/**
 * The steps of a turn's open fold, in the turn's order, and how many come
 * before them. A live step the page does not hold goes before the next live
 * step that the page holds, else after the page, and a step in both shows
 * its live version, the newer one. A live step before the page's first step
 * is one of the page's earlier steps, so it is not counted twice.
 */
export function foldSteps(
  turn: TurnDto,
  page: StepPage | undefined,
): { steps: StepDto[]; earlier: number } {
  const live = turn.steps.filter((step) => step.lane === turn.lane)
  if (!page) return { steps: live, earlier: 0 }
  const paged = new Set(page.steps.map((step) => step.id))
  const ahead = new Map<string, StepDto[]>()
  let waiting: StepDto[] = []
  for (const step of live) {
    if (!paged.has(step.id)) waiting.push(step)
    else if (waiting.length > 0) {
      ahead.set(step.id, waiting)
      waiting = []
    }
  }
  const byId = new Map(live.map((step) => [step.id, step]))
  const steps = page.steps.flatMap((step) => [
    ...(ahead.get(step.id) ?? []),
    byId.get(step.id) ?? step,
  ])
  const first = page.steps[0]
  const leading = first ? (ahead.get(first.id)?.length ?? 0) : 0
  return { steps: [...steps, ...waiting], earlier: Math.max(0, page.earlier - leading) }
}

/** The live steps of one subagent in one turn, oldest first. */
export function childSteps(
  children: Readonly<Record<string, StepDto>>,
  turnId: string,
  subagentId: string,
): StepDto[] {
  return Object.values(children)
    .filter((step) => step.turn_id === turnId && step.lane === subagentId)
    .sort(byId)
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

/**
 * A queued prompt leaves the queue once a newer console turn holds its text:
 * the text as it was submitted, else the prompt's content where a daemon
 * sends no submitted text. Each turn takes one prompt, once: the turns that
 * took one are kept, so a later revision does not take a second prompt of
 * the same text with them.
 */
function settle(state: TrajectoryState): TrajectoryState {
  if (state.queued.length === 0) return state
  const taken = new Set(state.echoed)
  const echoed = [...state.echoed]
  const queued = state.queued.filter((one) => {
    const turn = state.turns.find(
      (turn) =>
        turn.id > one.after &&
        !taken.has(turn.id) &&
        turn.prompt?.source === "console" &&
        (turn.prompt.text ?? contentText(turn.prompt.content)) === one.text,
    )
    if (!turn) return true
    taken.add(turn.id)
    echoed.push(turn.id)
    return false
  })
  return queued.length === state.queued.length ? state : { ...state, queued, echoed }
}

function byId(a: { id: string }, b: { id: string }): number {
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0
}

function upsertTurn(state: TrajectoryState, turn: TurnDto): TrajectoryState {
  const at = state.turns.findIndex((one) => one.id === turn.id)
  if (at === -1) {
    const turns = [...state.turns, { ...turn }].sort(byId)
    return { ...state, turns }
  }
  const turns = state.turns.slice()
  turns[at] = mergeTurn(state.turns[at] as TurnDto, turn)
  return { ...state, turns }
}

/**
 * A newer version of a turn over the one held. The newer one says the state,
 * the activity and the attention. An upsert carries no steps, so the ones
 * seen live stay, after the turn ends too, and an open fold keeps every row
 * it showed. A header of an old turn that a read loaded for its attention
 * alone has no prompt and no answer: the ones held stay. A prompt never
 * changes, and an answer only grows, so neither goes back to empty.
 */
function mergeTurn(held: TurnDto, newer: TurnDto): TurnDto {
  const prompt = newer.prompt && newer.prompt.content.length > 0 ? newer.prompt : held.prompt
  return {
    ...newer,
    prompt: prompt ?? newer.prompt,
    answer: newer.answer.length > 0 ? newer.answer : held.answer,
    steps: held.steps.length > 0 ? held.steps : newer.steps,
  }
}

/**
 * Change the steps of one turn wherever they are held: its live steps, its
 * attention and its loaded page. With `added`, a step new to the live steps
 * joins them where it is of the turn's own lane: a subagent's steps are its
 * own lane's. The attention and a page only change the steps they hold,
 * since the turn's own upsert says what its attention is.
 */
function changeSteps(
  state: TrajectoryState,
  turnId: string,
  change: (steps: StepDto[]) => StepDto[],
  added?: StepDto,
): TrajectoryState {
  const at = state.turns.findIndex((turn) => turn.id === turnId)
  if (at === -1) return state
  const turn = state.turns[at] as TurnDto
  const turns = state.turns.slice()
  turns[at] = {
    ...turn,
    steps: added && added.lane !== turn.lane ? changeHeld(turn.steps, change) : change(turn.steps),
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

/** Keep a step of a lane that is not its turn's own: a subagent's. */
function keepChild(state: TrajectoryState, step: StepDto): TrajectoryState {
  const turn = state.turns.find((one) => one.id === step.turn_id)
  if (step.lane === (turn?.lane ?? MAIN_LANE)) return state
  return { ...state, children: { ...state.children, [step.id]: step } }
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

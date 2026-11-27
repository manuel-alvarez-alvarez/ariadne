/**
 * The turn console's state: the daemon's snapshot and ops, applied by id.
 */

import { expect, it } from "vitest"

import type { TurnDto } from "@/api"

import {
  aMessageStep,
  anErrorStep,
  anUpdate,
  aPermissionStep,
  aSnapshot,
  aToolStep,
  aTurn,
  text,
} from "@/test/trajectory"

import {
  addStepPage,
  applySnapshot,
  applyUpdate,
  childSteps,
  contentText,
  dequeuePrompt,
  emptyTrajectory,
  foldSteps,
  prependTurns,
  queuePrompt,
  shownTurns,
} from "./store"

it("replaces a turn and a step by id, so an upsert never shows an item two times", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const tool = aToolStep(turn.id, { status: "running" })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([{ ...turn, steps: [tool] }]))

  state = applyUpdate(
    state,
    anUpdate(2, [
      { op: "item_upsert", step: { ...tool, status: "completed" } },
      { op: "turn_upsert", turn: { ...turn, answer: text("Done") } },
    ]),
  )

  expect(state.turns).toHaveLength(1)
  expect(state.turns[0]?.steps).toHaveLength(1)
  expect(state.turns[0]?.steps[0]).toMatchObject({ id: tool.id, status: "completed" })
  expect(contentText(state.turns[0]?.answer ?? [])).toBe("Done")
})

it("ignores a revision it has applied already", () => {
  const turn = aTurn()
  const state = applySnapshot(emptyTrajectory(), aSnapshot([turn], { rev: 4 }))

  const again = applyUpdate(state, anUpdate(4, [{ op: "turn_upsert", turn: aTurn() }]))

  expect(again).toBe(state)
})

it("appends streamed text to the step it names", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const message = aMessageStep(turn.id, { content: text("Work") })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([{ ...turn, steps: [message] }]))

  state = applyUpdate(
    state,
    anUpdate(2, [{ op: "text_append", turn_id: turn.id, item_id: message.id, text: "ing" }]),
  )

  const step = state.turns[0]?.steps[0]
  expect(step?.type === "message" && contentText(step.content)).toBe("Working")
})

it("changes a step of the attention in place and removes a step everywhere", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const permission = aPermissionStep(turn.id)
  let state = applySnapshot(
    emptyTrajectory(),
    aSnapshot([{ ...turn, steps: [permission], attention: [permission] }]),
  )

  state = applyUpdate(
    state,
    anUpdate(2, [{ op: "item_upsert", step: { ...permission, state: "answered" } }]),
  )
  expect(state.turns[0]?.attention[0]).toMatchObject({ state: "answered" })

  state = applyUpdate(
    state,
    anUpdate(3, [{ op: "item_remove", turn_id: turn.id, item_id: permission.id }]),
  )
  expect(state.turns[0]?.steps).toEqual([])
  expect(state.turns[0]?.attention).toEqual([])
})

it("replaces the state with a snapshot and drops the pages loaded before it", () => {
  const old = aTurn()
  let state = applySnapshot(emptyTrajectory(), aSnapshot([old]))
  state = addStepPage(state, old.id, { steps: [aToolStep(old.id)], earlier_steps: 0 })

  const fresh = aTurn()
  state = applySnapshot(state, aSnapshot([fresh]))

  expect(state.turns.map((turn) => turn.id)).toEqual([fresh.id])
  expect(state.pages).toEqual({})
})

it("puts older turns in front, once each", () => {
  const older = aTurn()
  const newer = aTurn()
  const state = applySnapshot(emptyTrajectory(), aSnapshot([newer], { has_older: true }))

  const next = prependTurns(state, aSnapshot([older, newer], { has_older: false }))

  expect(next.turns.map((turn) => turn.id)).toEqual([older.id, newer.id])
  expect(next.hasOlder).toBe(false)
})

it("keeps a queued prompt until a newer turn holds its text", () => {
  const before = aTurn({ prompt: { id: "p", content: text("again"), source: "console" } })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([before]))
  state = queuePrompt(state, "again", "queued:1")

  // A snapshot of the same turns holds no newer turn: the prompt stays queued.
  state = applySnapshot(state, aSnapshot([before]))
  expect(state.queued).toHaveLength(1)

  const echo = aTurn({ prompt: { id: "q", content: text("again"), source: "console" } })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: echo }]))
  expect(state.queued).toEqual([])
})

it("shows a loaded page of steps, then the live steps after it", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const first = aToolStep(turn.id)
  const second = aToolStep(turn.id)
  const live = { ...turn, steps: [second] }
  const state = addStepPage(applySnapshot(emptyTrajectory(), aSnapshot([live])), turn.id, {
    steps: [first, second],
    earlier_steps: 0,
  })

  expect(foldSteps(live, state.pages[turn.id]).steps.map((step) => step.id)).toEqual([
    first.id,
    second.id,
  ])
})

it("keeps a row seen live in an open fold after its turn ends", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const first = aToolStep(turn.id)
  let state = applySnapshot(emptyTrajectory(), aSnapshot([{ ...turn, steps: [first] }]))
  state = addStepPage(state, turn.id, { steps: [first], earlier_steps: 0 })

  const later = aToolStep(turn.id)
  state = applyUpdate(state, anUpdate(2, [{ op: "item_upsert", step: later }]))
  state = applyUpdate(
    state,
    anUpdate(3, [{ op: "turn_upsert", turn: { ...turn, state: "completed" } }]),
  )

  const ended = state.turns[0]
  expect(ended && foldSteps(ended, state.pages[turn.id]).steps.map((step) => step.id)).toEqual([
    first.id,
    later.id,
  ])
})

it("shows the live status of a step over a page read before it", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const tool = aToolStep(turn.id, { status: "running" })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([{ ...turn, steps: [tool] }]))
  state = applyUpdate(
    state,
    anUpdate(2, [{ op: "item_upsert", step: { ...tool, status: "completed" } }]),
  )

  // The page was read while the call still ran, and lands after its end.
  state = addStepPage(state, turn.id, { steps: [tool], earlier_steps: 0 })

  const live = state.turns[0]
  expect(live && foldSteps(live, state.pages[turn.id]).steps[0]).toMatchObject({
    status: "completed",
  })
})

it("keeps the steps of a subagent out of its parent's live steps", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([turn]))

  state = applyUpdate(
    state,
    anUpdate(2, [{ op: "item_upsert", step: aToolStep(turn.id, { lane: "01CHILD" }) }]),
  )

  const parent = state.turns[0]
  expect(parent?.steps).toEqual([])
  expect(parent && foldSteps(parent, undefined).steps).toEqual([])
})

it("keeps the live version of each step of a subagent's lane, and a snapshot drops them", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  let state = applySnapshot(emptyTrajectory(), aSnapshot([turn]))
  const child = aToolStep(turn.id, { lane: "01CHILD", status: "running", detail_rev: 1 })
  const other = aToolStep(turn.id, { lane: "01OTHER" })

  state = applyUpdate(
    state,
    anUpdate(2, [
      { op: "item_upsert", step: child },
      { op: "item_upsert", step: other },
      { op: "item_upsert", step: { ...child, detail_rev: 2 } },
    ]),
  )
  expect(childSteps(state.children, turn.id, "01CHILD")).toEqual([{ ...child, detail_rev: 2 }])

  state = applyUpdate(
    state,
    anUpdate(3, [{ op: "item_remove", turn_id: turn.id, item_id: child.id }]),
  )
  expect(childSteps(state.children, turn.id, "01CHILD")).toEqual([])
  expect(childSteps(state.children, turn.id, "01OTHER")).toEqual([other])

  state = applySnapshot(state, aSnapshot([turn], { rev: 4 }))
  expect(state.children).toEqual({})
})

it("takes one queued prompt per echo, whatever revision follows", () => {
  let state = applySnapshot(emptyTrajectory(), aSnapshot([aTurn()]))
  state = queuePrompt(state, "same", "queued:1")
  state = queuePrompt(state, "same", "queued:2")

  const echo = aTurn({ prompt: { id: "e", content: text("same"), source: "console" } })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: echo }]))
  expect(state.queued.map((one) => one.id)).toEqual(["queued:2"])

  state = applyUpdate(state, anUpdate(3, [{ op: "session_patch", session: aSnapshot([]).session }]))
  expect(state.queued.map((one) => one.id)).toEqual(["queued:2"])
})

it("takes a queued prompt away when its echo carries the seat brief ahead of it", () => {
  let state = applySnapshot(emptyTrajectory(), aSnapshot([]))
  state = queuePrompt(state, "Start the task", "queued:1")

  const echo = aTurn({
    prompt: {
      id: "e",
      content: text("You are the author.\n\nStart the task"),
      text: "Start the task",
      source: "console",
    },
  })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: echo }]))

  expect(state.queued).toEqual([])
})

it("keeps a queued prompt when another prompt only ends with its text", () => {
  let state = applySnapshot(emptyTrajectory(), aSnapshot([]))
  state = queuePrompt(state, "Start the task", "queued:1")

  // Another client sent a longer prompt whose last paragraph is the same text.
  const other = aTurn({
    prompt: {
      id: "o",
      content: text("Plan first.\n\nStart the task"),
      text: "Plan first.\n\nStart the task",
      source: "console",
    },
  })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: other }]))

  expect(state.queued.map((one) => one.id)).toEqual(["queued:1"])
})

it("clears the entry an identical send's echo left behind when a send is refused", () => {
  let state = applySnapshot(emptyTrajectory(), aSnapshot([]))
  state = queuePrompt(state, "same", "queued:1")
  state = queuePrompt(state, "same", "queued:2")
  // The echo of the second send comes first, and takes the oldest entry.
  const echo = aTurn({ prompt: { id: "e", content: text("same"), source: "console" } })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: echo }]))

  state = dequeuePrompt(state, "queued:1", "same")

  expect(state.queued).toEqual([])
})

it("shows the steps of an open fold in the turn's order, each counted once", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const watched = Array.from({ length: 51 }, () => aToolStep(turn.id))
  let state = applySnapshot(emptyTrajectory(), aSnapshot([turn]))
  state = applyUpdate(
    state,
    anUpdate(
      2,
      watched.map((step) => ({ op: "item_upsert" as const, step })),
    ),
  )

  // The route gives the last 50 steps, and counts the first one as earlier.
  state = addStepPage(state, turn.id, { steps: watched.slice(1), earlier_steps: 1 })

  const held = state.turns[0]
  const fold = held && foldSteps(held, state.pages[turn.id])
  expect(fold?.steps.map((step) => step.id)).toEqual(watched.map((step) => step.id))
  expect(fold?.earlier).toBe(0)
})

it("keeps a turn the stream brings from before the history out of view and off the cursor", () => {
  // Made oldest first: each id sorts after the ones made before it.
  const first = aTurn()
  const second = aTurn()
  const third = aTurn()
  let state = applySnapshot(emptyTrajectory(), aSnapshot([third], { has_older: true }))

  // A diagnosis names an error of the oldest turn, before the history read.
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: first }]))
  expect(shownTurns(state).map((turn) => turn.id)).toEqual([third.id])
  expect(state.cursor).toBe(third.id)

  state = prependTurns(state, aSnapshot([second], { has_older: true }))
  expect(shownTurns(state).map((turn) => turn.id)).toEqual([second.id, third.id])
  expect(state.cursor).toBe(second.id)

  state = prependTurns(state, aSnapshot([first], { has_older: false }))
  expect(shownTurns(state).map((turn) => turn.id)).toEqual([first.id, second.id, third.id])
})

/**
 * An old turn outside the daemon's window, as a diagnosis brings it: its
 * attention, with no prompt and no answer, since the read loaded no more.
 */
function partialHeader(whole: TurnDto): TurnDto {
  return {
    ...whole,
    prompt: null,
    answer: [],
    attention: [anErrorStep(whole.id, { diagnosis: "The disk is full" })],
  }
}

it("keeps the page's prompt and answer, and the stream's newer attention, when the stream came first", () => {
  const old = aTurn({ answer: text("Old answer") })
  const newest = aTurn()
  let state = applySnapshot(emptyTrajectory(), aSnapshot([newest], { has_older: true }))
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: partialHeader(old) }]))

  state = prependTurns(state, aSnapshot([{ ...old, attention: [] }], { has_older: false }))

  const shown = shownTurns(state)[0]
  expect(shown?.id).toBe(old.id)
  expect(contentText(shown?.prompt?.content ?? [])).toBe("Fix the bug")
  expect(contentText(shown?.answer ?? [])).toBe("Old answer")
  expect(shown?.attention).toMatchObject([{ type: "error", diagnosis: "The disk is full" }])
})

it("keeps the prompt and answer the reader loaded when a partial header comes after the page", () => {
  const old = aTurn({ answer: text("Old answer") })
  const newest = aTurn()
  let state = applySnapshot(emptyTrajectory(), aSnapshot([newest], { has_older: true }))
  state = prependTurns(state, aSnapshot([old], { has_older: false }))

  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: partialHeader(old) }]))

  const shown = shownTurns(state)[0]
  expect(contentText(shown?.prompt?.content ?? [])).toBe("Fix the bug")
  expect(contentText(shown?.answer ?? [])).toBe("Old answer")
  expect(shown?.attention).toMatchObject([{ type: "error", diagnosis: "The disk is full" }])
})

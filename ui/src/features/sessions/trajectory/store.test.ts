/**
 * The turn console's state: the daemon's snapshot and ops, applied by id.
 */

import { expect, it } from "vitest"

import {
  aMessageStep,
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
  contentText,
  dequeuePrompt,
  emptyTrajectory,
  prependTurns,
  queuePrompt,
  turnSteps,
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

  expect(turnSteps(live, state.pages[turn.id]).map((step) => step.id)).toEqual([
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
  expect(ended && turnSteps(ended, state.pages[turn.id]).map((step) => step.id)).toEqual([
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
  expect(live && turnSteps(live, state.pages[turn.id])[0]).toMatchObject({ status: "completed" })
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
  expect(parent && turnSteps(parent, undefined)).toEqual([])
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
    prompt: { id: "e", content: text("You are the author.\n\nStart the task"), source: "console" },
  })
  state = applyUpdate(state, anUpdate(2, [{ op: "turn_upsert", turn: echo }]))

  expect(state.queued).toEqual([])
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

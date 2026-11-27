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

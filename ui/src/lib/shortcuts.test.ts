import { describe, expect, it } from "vitest"

import {
  isBareKey,
  isTypingTarget,
  keySequenceLabel,
  matchesHelpKey,
  matchesKeySequence,
  matchesShortcut,
  type ShortcutEvent,
  sequenceLead,
} from "./shortcuts"

function event(overrides: Partial<ShortcutEvent> & { key: string }): ShortcutEvent {
  return { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...overrides }
}

describe("matchesShortcut", () => {
  it("takes either command modifier, so the same chord works on every platform", () => {
    expect(matchesShortcut(event({ key: "k", metaKey: true }), { key: "k" })).toBe(true)
    expect(matchesShortcut(event({ key: "k", ctrlKey: true }), { key: "k" })).toBe(true)
  })

  it("needs the modifier", () => {
    expect(matchesShortcut(event({ key: "k" }), { key: "k" })).toBe(false)
  })

  it("ignores the case the browser reports the key in", () => {
    expect(matchesShortcut(event({ key: "K", metaKey: true }), { key: "k" })).toBe(true)
  })

  it("does not answer to another application's chord over the same key", () => {
    expect(matchesShortcut(event({ key: "k", metaKey: true, altKey: true }), { key: "k" })).toBe(
      false,
    )
    expect(matchesShortcut(event({ key: "k", metaKey: true, shiftKey: true }), { key: "k" })).toBe(
      false,
    )
  })

  it("matches punctuation keys, which is what settings is bound to", () => {
    expect(matchesShortcut(event({ key: ",", metaKey: true }), { key: "," })).toBe(true)
    expect(matchesShortcut(event({ key: ".", metaKey: true }), { key: "," })).toBe(false)
  })
})

describe("matchesKeySequence", () => {
  const NEW_GOAL = { key: "n" }
  const SESSIONS = { lead: "g", key: "s" }

  it("takes a lone key when nothing is pending", () => {
    expect(matchesKeySequence(event({ key: "n" }), NEW_GOAL, null)).toBe(true)
    expect(matchesKeySequence(event({ key: "N" }), NEW_GOAL, null)).toBe(true)
  })

  it("does not take a key that is held with a modifier — that is another chord", () => {
    expect(matchesKeySequence(event({ key: "n", metaKey: true }), NEW_GOAL, null)).toBe(false)
    expect(matchesKeySequence(event({ key: "n", ctrlKey: true }), NEW_GOAL, null)).toBe(false)
    expect(matchesKeySequence(event({ key: "n", shiftKey: true }), NEW_GOAL, null)).toBe(false)
    expect(matchesKeySequence(event({ key: "s", altKey: true }), SESSIONS, "g")).toBe(false)
  })

  it("completes a sequence only after its lead", () => {
    expect(matchesKeySequence(event({ key: "s" }), SESSIONS, "g")).toBe(true)
    expect(matchesKeySequence(event({ key: "s" }), SESSIONS, null)).toBe(false)
    expect(matchesKeySequence(event({ key: "s" }), SESSIONS, "x")).toBe(false)
  })

  it("leaves a lone key alone while a lead is pending, so `g n` creates nothing", () => {
    expect(matchesKeySequence(event({ key: "n" }), NEW_GOAL, "g")).toBe(false)
  })

  it("knows which keys open a sequence", () => {
    expect(sequenceLead(event({ key: "g" }), [NEW_GOAL, SESSIONS])).toBe("g")
    expect(sequenceLead(event({ key: "n" }), [NEW_GOAL, SESSIONS])).toBe(null)
    expect(sequenceLead(event({ key: "g", metaKey: true }), [NEW_GOAL, SESSIONS])).toBe(null)
  })

  it("spells a typed chord as it is typed", () => {
    expect(keySequenceLabel(NEW_GOAL)).toBe("N")
    expect(keySequenceLabel(SESSIONS)).toBe("G S")
  })
})

describe("matchesHelpKey", () => {
  it("takes the character, not the keys pressed to type it", () => {
    // Shift is how `?` is typed on most layouts, so it cannot disqualify it.
    expect(matchesHelpKey(event({ key: "?", shiftKey: true }))).toBe(true)
    expect(matchesHelpKey(event({ key: "?" }))).toBe(true)
  })

  it("is not a chord anything is held for", () => {
    expect(matchesHelpKey(event({ key: "?", metaKey: true }))).toBe(false)
    expect(matchesHelpKey(event({ key: "?", ctrlKey: true }))).toBe(false)
    expect(matchesHelpKey(event({ key: "?", altKey: true }))).toBe(false)
    expect(matchesHelpKey(event({ key: "/" }))).toBe(false)
  })
})

describe("isBareKey", () => {
  it("is true only with nothing held", () => {
    expect(isBareKey(event({ key: "g" }))).toBe(true)
    expect(isBareKey(event({ key: "g", metaKey: true }))).toBe(false)
    expect(isBareKey(event({ key: "g", shiftKey: true }))).toBe(false)
  })
})

describe("isTypingTarget", () => {
  it("is false for the page itself", () => {
    expect(isTypingTarget({ tagName: "BODY" })).toBe(false)
    expect(isTypingTarget(null)).toBe(false)
    expect(isTypingTarget(undefined)).toBe(false)
  })

  it("is true for form fields, xterm's hidden textarea included", () => {
    expect(isTypingTarget({ tagName: "INPUT" })).toBe(true)
    expect(isTypingTarget({ tagName: "TEXTAREA" })).toBe(true)
    expect(isTypingTarget({ tagName: "SELECT" })).toBe(true)
  })

  it("is true inside a contenteditable, where CodeMirror puts focus", () => {
    expect(isTypingTarget({ tagName: "SPAN", isContentEditable: true })).toBe(true)
  })

  it("is false for a button, which has keys of its own but no text", () => {
    expect(isTypingTarget({ tagName: "BUTTON" })).toBe(false)
  })
})

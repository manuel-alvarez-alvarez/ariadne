import { describe, expect, it } from "vitest"

import { isTypingTarget, matchesShortcut, type ShortcutEvent } from "./shortcuts"

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

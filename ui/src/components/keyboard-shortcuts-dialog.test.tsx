// @vitest-environment jsdom

/**
 * The cheat sheet has one job: to be complete. A chord that is bound and not on
 * it is a chord nobody finds, which is the state the app was in before it
 * existed — so what is asserted is the list, chord by chord, against
 * `ui/README.md`'s keyboard table.
 */

import { render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { shortcutLabel } from "@/lib/shortcuts"

import { KeyboardShortcutsDialog } from "./keyboard-shortcuts-dialog"

/** Every chord the README documents, as the sheet spells it. */
const CHORDS = ["N", "[", "G G", "G S", "G K", "G A", "G R", "?", "Esc"]

it("lists every chord, screen by screen", () => {
  render(<KeyboardShortcutsDialog open onOpenChange={() => {}} />)

  for (const chord of CHORDS) {
    expect(screen.getByText(chord)).toBeTruthy()
  }
  // The ⌘ chords are spelled for the platform the sheet is read on, so
  // the held ones are matched by what they do rather than by a hardcoded
  // glyph — except ⌘Escape, whose key cell is asserted below, computed the
  // same way the row itself is.
  expect(screen.getByText("Open the command palette")).toBeTruthy()
  expect(screen.getByText("Open settings")).toBeTruthy()
  expect(screen.getByText(shortcutLabel({ key: "Esc" }))).toBeTruthy()
  expect(
    screen.getByText("Leave a focused console for the pane or the modal around it"),
  ).toBeTruthy()
  // The screens are named, not just keyed.
  expect(screen.getByText("Go to Sessions")).toBeTruthy()
})

it("says what the two vocabularies are, since neither is guessable", () => {
  render(<KeyboardShortcutsDialog open onOpenChange={() => {}} />)

  const description = screen.getByText(/answer to Ctrl as well/)
  expect(description.textContent).toContain("session's console")
})

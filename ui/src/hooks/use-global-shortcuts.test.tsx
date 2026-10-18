// @vitest-environment jsdom

/**
 * Two things worth a test of their own, beyond what `lib/shortcuts.test.ts`
 * already pins: the one typed chord that carries the Shift it takes to type
 * it (`?`, which `isBareKey` cannot guard), and the one place the ⌘ chords
 * and the typed ones part ways — a held chord fires from a text field or a
 * session's console, where a typed one never would, but neither fires once
 * a dialog is up.
 */

import { fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { useGlobalShortcuts } from "./use-global-shortcuts"

const handlers = {
  onOpenPalette: vi.fn(),
  onOpenSettings: vi.fn(),
  onNewGoal: vi.fn(),
  onOpenShortcuts: vi.fn(),
  onNavigate: vi.fn(),
  onToggleSidebar: vi.fn(),
}

/**
 * A screen with the chords bound, a text field, a session's console — a
 * bare `<textarea>` stands in for it here, since `isTypingTarget` cannot
 * tell the two apart — and something layered over it.
 */
function Screen() {
  useGlobalShortcuts(handlers)
  return (
    <>
      <input aria-label="Title" />
      <textarea aria-label="Console" />
      <div role="dialog">
        <button type="button">In a dialog</button>
      </div>
    </>
  )
}

function mount() {
  for (const handler of Object.values(handlers)) handler.mockClear()
  render(<Screen />)
  return userEvent.setup()
}

it("opens the cheat sheet on a bare ?", async () => {
  const user = mount()

  await user.keyboard("?")

  expect(handlers.onOpenShortcuts).toHaveBeenCalledTimes(1)
})

it("leaves the character alone where it is being typed", async () => {
  const user = mount()

  await user.click(screen.getByLabelText("Title"))
  await user.keyboard("?")

  expect(handlers.onOpenShortcuts).not.toHaveBeenCalled()
  expect(screen.getByLabelText("Title")).toHaveProperty("value", "?")
})

it("leaves it to whatever is layered over the screen", async () => {
  const user = mount()

  await user.click(screen.getByRole("button", { name: "In a dialog" }))
  await user.keyboard("?")

  expect(handlers.onOpenShortcuts).not.toHaveBeenCalled()
})

it("opens the palette on ⌘K from a text field", () => {
  mount()

  fireEvent.keyDown(screen.getByLabelText("Title"), { key: "k", metaKey: true })

  expect(handlers.onOpenPalette).toHaveBeenCalledTimes(1)
})

it("opens the palette on ⌘K from a session's console", () => {
  mount()

  fireEvent.keyDown(screen.getByLabelText("Console"), { key: "k", metaKey: true })

  expect(handlers.onOpenPalette).toHaveBeenCalledTimes(1)
})

it("does nothing on ⌘K while a dialog is up", () => {
  mount()

  fireEvent.keyDown(screen.getByRole("button", { name: "In a dialog" }), {
    key: "k",
    metaKey: true,
  })

  expect(handlers.onOpenPalette).not.toHaveBeenCalled()
})

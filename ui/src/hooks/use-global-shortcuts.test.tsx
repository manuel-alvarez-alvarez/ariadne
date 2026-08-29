// @vitest-environment jsdom

/**
 * The one chord whose *binding* is worth a test of its own: `?`.
 *
 * `lib/shortcuts.test.ts` pins the match — `?` carries the Shift it takes to
 * type it, so `isBareKey` cannot guard it — and what is left is whether the
 * shell's listener asks the same question at the same point as it does for the
 * bare letters: after the guards, and never where the keystroke is text or
 * belongs to something layered over the screen.
 */

import { render, screen } from "@testing-library/react"
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

/** A screen with the chords bound, a text field, and something layered over it. */
function Screen() {
  useGlobalShortcuts(handlers)
  return (
    <>
      <input aria-label="Title" />
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

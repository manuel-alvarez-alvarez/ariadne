// @vitest-environment jsdom

/**
 * The sidebar rail: the one piece of the shell that changes how much room the
 * screen inside it gets.
 *
 * 14rem of navigation is 14rem the goals board's five pipeline columns do not
 * get, and on a 1280px laptop that is the difference between a board that fits
 * and a board that scrolls sideways with its last column off the edge. So the
 * shell folds it down to icons — from the header's button and from `[` — and
 * the rail keeps every entry's name in the accessibility tree while taking it
 * off the screen.
 *
 * Mounted against a real data router, because the shell reads the route's own
 * `handle` for the screen's name (`useMatches`), and with no daemon answering:
 * nothing about the rail depends on one.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import { AppShell, type PageHandle } from "@/components/app-shell"
import { useSettingsStore } from "@/stores/settings"
import { renderScreen } from "@/test/harness"

// The store is a module singleton, and the rail is persisted: without this,
// the first test to fold it away folds it for every test after it.
beforeEach(() => useSettingsStore.setState({ sidebarCollapsed: false }))

function mountShell() {
  const router = createMemoryRouter(
    [
      {
        path: "/",
        element: <AppShell />,
        children: [
          { index: true, element: <div />, handle: { title: "Goals" } satisfies PageHandle },
        ],
      },
    ],
    { initialEntries: ["/"] },
  )
  // The tree brings its own router, so the harness only wraps it in the query
  // client and the tooltip provider the shell's own pieces need.
  renderScreen(<RouterProvider router={router} />, { route: null })
  return { user: userEvent.setup(), aside: () => document.querySelector("aside") }
}

it("shows the navigation in full until it is folded away", () => {
  const { aside } = mountShell()

  expect(aside()?.className).toContain("w-56")
  expect(screen.getByText("Ariadne Desktop")).not.toBeNull()
  // Each entry reads as its label, which is also its accessible name.
  expect(screen.getByRole("link", { name: "Repositories" }).textContent).toBe("Repositories")
})

it("folds down to an icon rail from the header, and back", async () => {
  const { user, aside } = mountShell()

  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  expect(aside()?.className).toContain("w-14")

  // The labels come off the screen but not out of the accessibility tree: the
  // links are still named, and a pointer gets the name back as a tooltip.
  const repositories = screen.getByRole("link", { name: "Repositories" })
  expect(repositories.textContent).toBe("")
  expect(screen.queryByText("Ariadne Desktop")).toBeNull()

  await user.click(screen.getByRole("button", { name: "Expand sidebar" }))
  expect(aside()?.className).toContain("w-56")
  expect(screen.getByRole("link", { name: "Repositories" }).textContent).toBe("Repositories")
})

it("says which way it is, so the button is a toggle and not a command", async () => {
  const { user } = mountShell()

  const collapse = screen.getByRole("button", { name: "Collapse sidebar" })
  expect(collapse.getAttribute("aria-pressed")).toBe("false")

  await user.click(collapse)
  expect(screen.getByRole("button", { name: "Expand sidebar" }).getAttribute("aria-pressed")).toBe(
    "true",
  )
})

it("answers to the bracket chord from anywhere on the screen", async () => {
  const { user, aside } = mountShell()

  // `[[` is how user-event spells a literal bracket: a bare `[` opens its own
  // key-descriptor syntax.
  await user.keyboard("[[")
  expect(aside()?.className).toContain("w-14")

  await user.keyboard("[[")
  expect(aside()?.className).toContain("w-56")
})

it("leaves the bracket alone where it is being typed", async () => {
  const { user, aside } = mountShell()

  // The shell's chords are bound on `window`, so the guard against a keystroke
  // that belongs to a text field is the only thing between `[` and a sidebar
  // that folds itself away mid-word.
  const field = document.createElement("input")
  document.body.append(field)
  field.focus()
  await user.keyboard("[[")

  expect(field.value).toBe("[")
  expect(aside()?.className).toContain("w-56")
  field.remove()
})

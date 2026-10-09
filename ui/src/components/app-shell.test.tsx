// @vitest-environment jsdom

/**
 * The shell: one header bar naming the screen and carrying its actions, a
 * sidebar that folds to an icon rail, and the daemon connection status at the
 * sidebar's own foot rather than in a footer of its own.
 *
 * 12rem of navigation is 12rem the goals board's five pipeline columns do not
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

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { ReactNode } from "react"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import { AppShell, type PageHandle } from "@/components/app-shell"
import { PageHeader } from "@/components/page-header"
import { useSettingsStore } from "@/stores/settings"
import { renderScreen, startAt } from "@/test/harness"

// The store is a module singleton, and the rail is persisted: without this,
// the first test to fold it away folds it for every test after it.
beforeEach(() => useSettingsStore.setState({ sidebarCollapsed: false }))

function mountShell(screenElement: ReactNode = <div />) {
  startAt("/")
  const router = createBrowserRouter([
    {
      path: "/",
      element: <AppShell />,
      children: [
        { index: true, element: screenElement, handle: { title: "Goals" } satisfies PageHandle },
      ],
    },
  ])
  // The tree brings its own router, so the harness only wraps it in the query
  // client and the tooltip provider the shell's own pieces need.
  renderScreen(<RouterProvider router={router} />, { route: null })
  return { user: userEvent.setup(), aside: () => document.querySelector("aside") }
}

it("shows the navigation in full until it is folded away", () => {
  const { aside } = mountShell()

  expect(aside()?.className).toContain("w-48")
  expect(screen.getByText("Ariadne")).not.toBeNull()
  // Each entry reads as its label, which is also its accessible name.
  expect(screen.getByRole("link", { name: "Repositories" }).textContent).toBe("Repositories")
})

it("ends the navigation with stats, and lists Forge beside the other screens", () => {
  mountShell()

  // Scoped to the navigation, so a link anywhere else in the shell cannot
  // stand in for an entry of its own.
  const nav = screen.getByRole("navigation", { name: "Main" })
  const links = within(nav)
    .getAllByRole("link")
    .map((link) => link.getAttribute("aria-label"))
  expect(links).toEqual([
    "Goals",
    "Sessions",
    "Forge",
    "Skills",
    "Agents",
    "Permissions",
    "Repositories",
    "Stats",
  ])
  expect(screen.getByRole("link", { name: "Forge" }).getAttribute("href")).toBe("/forge")
  expect(screen.getByRole("link", { name: "Stats" }).getAttribute("href")).toBe("/stats")
  expect(screen.getByRole("link", { name: "Repositories" }).getAttribute("href")).toBe(
    "/repositories",
  )
  expect(screen.getByRole("link", { name: "Permissions" }).getAttribute("href")).toBe(
    "/permissions",
  )
})

it("folds down to an icon rail from the header, and back", async () => {
  const { user, aside } = mountShell()

  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))
  expect(aside()?.className).toContain("w-14")

  // The labels come off the screen but not out of the accessibility tree: the
  // links are still named, and a pointer gets the name back as a tooltip.
  const repositories = screen.getByRole("link", { name: "Repositories" })
  expect(repositories.textContent).toBe("")
  expect(screen.queryByText("Ariadne")).toBeNull()

  await user.click(screen.getByRole("button", { name: "Expand sidebar" }))
  expect(aside()?.className).toContain("w-48")
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
  expect(aside()?.className).toContain("w-48")
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
  expect(aside()?.className).toContain("w-48")
  field.remove()
})

it("shows the screen's name once, as the header's only h1, with its actions at the header's end", () => {
  mountShell(<PageHeader title="Goals" actions={<button type="button">New goal</button>} />)

  const headings = screen.getAllByRole("heading", { level: 1 })
  expect(headings).toHaveLength(1)
  expect(headings[0]?.textContent).toBe("Goals")

  const header = document.querySelector("header")
  if (!header) throw new Error("no header in the shell")
  const newGoal = within(header).getByRole("button", { name: "New goal" })
  const search = within(header).getByRole("button", { name: /Search/ })
  // "At the header's end" means directly before search, theme and settings —
  // not merely somewhere in the header, which beside the title would satisfy
  // too loose a check.
  const headerChildren = Array.from(header.children)
  const actionsContainer = headerChildren.find((el) => el.contains(newGoal))
  const controlsContainer = headerChildren.find((el) => el.contains(search))
  expect(headerChildren.indexOf(controlsContainer as Element)).toBe(
    headerChildren.indexOf(actionsContainer as Element) + 1,
  )
  // The screen's own content never gets its own competing heading.
  const main = document.querySelector("main")
  if (!main) throw new Error("no main in the shell")
  expect(within(main).queryByRole("heading")).toBeNull()
})

it("renders no footer, and ends the sidebar with the connection status, whose click opens the logs drawer", async () => {
  const { user, aside } = mountShell()

  expect(document.querySelector("footer")).toBeNull()
  expect(screen.queryByRole("contentinfo")).toBeNull()

  const status = screen.getByRole("button", { name: /Daemon status/ })
  // The status button itself, not a wrapper around it: no DOM sits between
  // the sidebar and its foot.
  expect(aside()?.lastElementChild).toBe(status)

  await user.click(status)
  expect(screen.getByText("Daemon logs")).not.toBeNull()
})

it("replaces the wordmark with the mark alone in the rail, next to the status dot", async () => {
  const { user, aside } = mountShell()

  const markBefore = aside()?.firstElementChild?.querySelector("svg")
  expect(markBefore).not.toBeNull()

  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }))

  // The mark survives the fold, the word does not — but the mark now carries
  // the name the word used to, as a tooltip of its own.
  const mark = aside()?.firstElementChild?.querySelector("svg")
  expect(mark).not.toBeNull()
  expect(screen.queryByText("Ariadne")).toBeNull()
  const markTrigger = mark?.closest('[data-slot="tooltip-trigger"]')
  if (!markTrigger) throw new Error("the mark has no tooltip trigger around it")
  await user.hover(markTrigger)
  expect(await screen.findByText("Ariadne")).not.toBeNull()

  // The status dot keeps a name of its own, as a tooltip rather than a label.
  const status = screen.getByRole("button", { name: /Daemon status/ })
  const statusName = status.getAttribute("aria-label")?.replace("Daemon status: ", "")
  await user.hover(status)
  expect(await screen.findByText(statusName ?? "")).not.toBeNull()
})

it("keeps the screen at full width behind an open goal", async () => {
  startAt("/goals?goal=g1")
  const router = createBrowserRouter([
    {
      path: "/",
      element: <AppShell />,
      children: [
        { path: "goals", element: <div />, handle: { title: "Goals" } satisfies PageHandle },
      ],
    },
  ])
  renderScreen(<RouterProvider router={router} />, { route: null })

  const pane = await screen.findByRole("region")
  const main = screen.getByRole("main")
  // jsdom lays nothing out, so the claim is held where layout comes from:
  // `<main>` is alone in its row, and the pane floats outside it.
  expect(main.parentElement?.children).toHaveLength(1)
  expect(main.parentElement?.contains(pane)).toBe(false)
})

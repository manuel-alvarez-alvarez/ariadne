// @vitest-environment jsdom

/**
 * What a tab shows (spec 015, rule 110): its icon before its word, one icon per
 * concept across the app, the sidebar's own icon where the sidebar has the
 * concept, and the word as the accessible name and the tooltip. jsdom has no
 * layout, so the width at which the word leaves the layout is proved in a
 * browser probe (spec 015, rule 110).
 */

import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import { AppSidebar } from "@/components/app-sidebar"
import { MarkdownField } from "@/components/markdown-field"
import { TabCount } from "@/components/tab-count"
import { ForgePage } from "@/features/forge/forge-page"
import { PermissionsPage } from "@/features/permissions/permissions-page"
import { TAB_ICONS } from "@/lib/tab-icons"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { Tabs, TabsList, TabsTrigger } from "./tabs"

beforeEach(() => {
  daemonFetch.mockImplementation(async () => jsonResponse([]))
})

/** The `lucide-<name>` class of the icon inside an element: the icon's identity. */
function iconOf(element: HTMLElement): string {
  const svg = element.querySelector("svg")
  const name = [...(svg?.classList ?? [])].find(
    (each) => each.startsWith("lucide-") && each !== "lucide",
  )
  if (!name) throw new Error("no icon")
  return name
}

it("draws the icon before the word, keeps the word as the name, and keeps the count", () => {
  render(
    <Tabs value="sessions">
      <TabsList>
        <TabsTrigger value="sessions" icon={<TAB_ICONS.sessions />} label="Sessions">
          <TabCount count={3} noun="session" />
        </TabsTrigger>
      </TabsList>
    </Tabs>,
  )

  const tab = screen.getByRole("tab", { name: "Sessions" })
  expect(tab.firstElementChild?.tagName.toLowerCase()).toBe("svg")
  expect(tab.firstElementChild?.getAttribute("aria-hidden")).toBe("true")
  expect(within(tab).getByText("Sessions")).toBeTruthy()
  expect(within(tab).getByLabelText("3 sessions")).toBeTruthy()
})

it("hides the word in a narrow pane and keeps it otherwise", () => {
  render(
    <Tabs value="a">
      <TabsList>
        <TabsTrigger value="a" icon={<TAB_ICONS.diff />} label="Diff" />
      </TabsList>
    </Tabs>,
  )

  // A 1px, clipped word (`sr-only`) below the pane container's 26rem, and the
  // plain word above it: the class is the whole rule, the browser applies it.
  const word = screen.getByText("Diff", { selector: "span" })
  expect(word.className).toBe("@max-[26rem]/pane:sr-only")
})

it("names the word in a tooltip when the word is hidden", async () => {
  // jsdom lays nothing out, so the word measures no width: the narrow case.
  render(
    <Tabs value="a">
      <TabsList>
        <TabsTrigger value="a" icon={<TAB_ICONS.diff />} label="Diff" />
      </TabsList>
    </Tabs>,
  )

  await userEvent.hover(screen.getByRole("tab", { name: "Diff" }))
  await waitFor(() =>
    expect(document.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe("Diff"),
  )
})

it("gives each tab concept one icon, on every screen that has the tab", () => {
  const icons = Object.values(TAB_ICONS).map((Icon) => {
    const { container, unmount } = render(<Icon />)
    const name = iconOf(container)
    unmount()
    return name
  })
  expect(new Set(icons).size).toBe(icons.length)
})

it("uses the sidebar's icon for a tab the sidebar names", async () => {
  const router = createMemoryRouter(
    [
      {
        path: "/",
        element: (
          <>
            <AppSidebar />
            <ForgePage />
          </>
        ),
      },
    ],
    { initialEntries: ["/"] },
  )
  renderScreen(<RouterProvider router={router} />, { route: null })

  const forgeTab = screen.getByRole("tab", { name: "Pull requests" })
  expect(iconOf(forgeTab)).toBe(iconOf(screen.getByRole("link", { name: "Forge" })))
  const sessions = document.createElement("div")
  render(<TAB_ICONS.sessions />, { container: sessions })
  expect(iconOf(screen.getByRole("link", { name: "Sessions" }))).toBe(iconOf(sessions))
})

it("shows an icon on the Forge, Permissions and markdown tabs", () => {
  const router = createMemoryRouter(
    [
      {
        path: "/",
        element: (
          <>
            <ForgePage />
            <PermissionsPage />
            <MarkdownField id="notes" label="Notes" name="notes" value="" onChange={() => {}} />
          </>
        ),
      },
    ],
    { initialEntries: ["/"] },
  )
  renderScreen(<RouterProvider router={router} />, { route: null })

  for (const name of ["Pull requests", "Issues", "Learned", "AI", "Write", "Preview"]) {
    expect(screen.getByRole("tab", { name }).firstElementChild?.tagName.toLowerCase()).toBe("svg")
  }
})

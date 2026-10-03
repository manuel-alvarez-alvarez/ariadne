// @vitest-environment jsdom

/**
 * `PageHeader` carries no heading of its own any more: the shell's window
 * header already names the screen, so this only has to prove its two
 * remaining jobs — handing `description` up as that header's tooltip, and
 * putting `actions` in the header's own slot — and that it still renders
 * `actions` in place when there is no shell around it to reach for, which is
 * how every feature page's own test mounts its screen.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import { AppShell, type PageHandle } from "@/components/app-shell"
import { useSettingsStore } from "@/stores/settings"
import { renderScreen, startAt } from "@/test/harness"

import { PageHeader } from "./page-header"

beforeEach(() => useSettingsStore.setState({ sidebarCollapsed: false }))

it("renders its actions in place, with no heading, when there is no shell to hand them to", () => {
  renderScreen(
    <PageHeader
      title="Goals"
      description="What Ariadne is working on."
      actions={<button type="button">New goal</button>}
    />,
  )

  expect(screen.getByRole("button", { name: "New goal" })).not.toBeNull()
  expect(screen.queryByRole("heading")).toBeNull()
  expect(screen.queryByText("What Ariadne is working on.")).toBeNull()
})

it("shows its description as the tooltip of the shell's header title", async () => {
  startAt("/")
  const router = createBrowserRouter([
    {
      path: "/",
      element: <AppShell />,
      children: [
        {
          index: true,
          element: <PageHeader title="Goals" description="What Ariadne is working on." />,
          handle: { title: "Goals" } satisfies PageHandle,
        },
      ],
    },
  ])
  renderScreen(<RouterProvider router={router} />, { route: null })

  const title = screen.getByRole("heading", { level: 1, name: "Goals" })
  const user = userEvent.setup()
  await user.hover(title)

  expect(await screen.findByText("What Ariadne is working on.")).not.toBeNull()
})

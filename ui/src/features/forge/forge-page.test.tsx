// @vitest-environment jsdom

/**
 * The Forge screen's two tabs, each a route of its own: the screen opens on
 * pull requests, and a tab click moves the URL to the other.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ForgePage } from "./forge-page"

beforeEach(() => {
  daemonFetch.mockImplementation(async () => jsonResponse([]))
})

function mount(at: string) {
  const router = createMemoryRouter(
    [
      {
        path: "/forge",
        element: <ForgePage />,
        children: [
          { path: "pull-requests", element: <p>pull requests tab</p> },
          { path: "issues", element: <p>issues tab</p> },
        ],
      },
    ],
    { initialEntries: [at] },
  )
  renderScreen(<RouterProvider router={router} />, { route: null })
  return router
}

it("shows the tab its URL names, and a tab click moves to the other", async () => {
  const user = userEvent.setup()
  const router = mount("/forge/pull-requests")

  expect(screen.getByText("pull requests tab")).toBeDefined()
  expect(screen.getByRole("tab", { name: "Pull requests" }).getAttribute("aria-selected")).toBe(
    "true",
  )

  await user.click(screen.getByRole("tab", { name: "Issues" }))
  expect(await screen.findByText("issues tab")).toBeDefined()
  expect(router.state.location.pathname).toBe("/forge/issues")
})

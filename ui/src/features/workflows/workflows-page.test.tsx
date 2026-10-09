// @vitest-environment jsdom

import { screen, within } from "@testing-library/react"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import { describe, expect, it } from "vitest"
import { paths } from "@/routes/paths"
import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen, startAt } from "@/test/harness"
import { WorkflowsPage } from "./workflows-page"

describe("the workflow list", () => {
  it("groups shipped workflows apart from user workflows and shows their columns", async () => {
    const shipped = aWorkflow()
    const mine = aWorkflow({
      name: "release",
      builtin: false,
      steps: [
        {
          id: "release",
          title: "Release",
          description: "Release the work.",
          skills: [],
          rank: null,
          gate: null,
        },
      ],
    })
    daemonFetch.mockImplementation(async () => jsonResponse([shipped, mine]))
    startAt(paths.workflows())
    const router = createBrowserRouter([{ path: paths.workflows(), element: <WorkflowsPage /> }])
    renderScreen(<RouterProvider router={router} />, { route: null })

    const shippedGroup = await screen.findByRole("region", { name: "Shipped with Ariadne" })
    expect(
      within(shippedGroup).getByRole("link", { name: /develop-review-merge/ }).textContent,
    ).toContain("Develop")
    expect(
      within(screen.getByRole("region", { name: "Yours" })).getByRole("link", { name: /release/ }),
    ).toBeDefined()
  })
})

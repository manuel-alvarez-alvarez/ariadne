// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

import { aGoal } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CreateTaskDialog } from "./task-form-dialog"

it("renders one staffing row for each workflow column", async () => {
  daemonFetch.mockImplementation(async (input) => {
    const request = input instanceof Request ? input : new Request(String(input))
    const path = new URL(request.url).pathname
    if (path === "/v1/skills" || path === "/v1/models" || path === "/v1/tasks")
      return jsonResponse([])
    return new Response("not found", { status: 404 })
  })
  const goal = aGoal({
    steps: [
      { id: "build", title: "Build", description: "", skills: ["coding"] },
      { id: "test", title: "Test", description: "", skills: ["testing"] },
    ],
  })
  renderScreen(<CreateTaskDialog goal={goal} open onOpenChange={vi.fn()} />)
  expect(await screen.findByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.queryByText("Author skills")).toBeNull()
  expect(screen.queryByText("Reviewers")).toBeNull()
})

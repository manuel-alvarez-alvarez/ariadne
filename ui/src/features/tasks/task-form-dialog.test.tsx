// @vitest-environment jsdom
import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { aGoal, aModel, anEffort } from "@/test/fixtures"
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
      { id: "build", title: "Build", description: "", skills: ["coding"], ranks: [] },
      { id: "test", title: "Test", description: "", skills: ["testing"], ranks: [] },
    ],
  })
  renderScreen(<CreateTaskDialog goal={goal} open onOpenChange={vi.fn()} />)
  expect(await screen.findByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.queryByText("Author skills")).toBeNull()
  expect(screen.queryByText("Reviewers")).toBeNull()
})

it("pins a column's model and effort through the three-column picker", async () => {
  const model = aModel({
    id: "codex-acp:gpt-5.5",
    agent_id: "codex-acp",
    efforts: [anEffort({ id: "high", default: true })],
  })
  daemonFetch.mockImplementation(async (input) => {
    const request = input instanceof Request ? input : new Request(String(input))
    const path = new URL(request.url).pathname
    if (path === "/v1/models") return jsonResponse([model])
    if (path === "/v1/skills" || path === "/v1/tasks" || path === "/v1/acp-agents")
      return jsonResponse([])
    return new Response("not found", { status: 404 })
  })
  const goal = aGoal({
    steps: [{ id: "build", title: "Build", description: "", skills: ["coding"], ranks: [] }],
  })
  const user = userEvent.setup()
  renderScreen(<CreateTaskDialog goal={goal} open onOpenChange={vi.fn()} />)

  const trigger = await screen.findByRole("button", { name: "Build runs on" })
  await user.click(trigger)
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(model.id),
  )
  await user.click(within(screen.getByRole("listbox", { name: "Efforts" })).getByText("high"))

  await waitFor(() => expect(screen.queryByRole("listbox", { name: "Efforts" })).toBeNull())
  expect(trigger.textContent).toContain("gpt-5.5 · high")
  expect(screen.getByRole("dialog")).toBeDefined()
})

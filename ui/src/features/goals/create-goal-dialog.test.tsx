// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

import { aRepository, aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CreateGoalDialog } from "./create-goal-dialog"

it("offers a workflow and no landing", async () => {
  daemonFetch.mockImplementation(async (input) => {
    const path = new URL(String(input)).pathname
    if (path === "/v1/repositories")
      return jsonResponse([aRepository({ default_workflow: "build" })])
    if (path === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build" })])
    if (path === "/v1/models") return jsonResponse([])
    return new Response("not found", { status: 404 })
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  expect(await screen.findByRole("combobox", { name: "Workflow" })).toBeTruthy()
  expect(screen.queryByRole("combobox", { name: /landing/i })).toBeNull()
})

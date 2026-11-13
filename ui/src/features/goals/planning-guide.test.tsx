// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import type { SessionDto } from "@/api"
import { aGoal, aSession, aSessionPage } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { PlanningGuide } from "./planning-guide"

function mount(sessions: SessionDto[] | "error") {
  daemonFetch.mockResolvedValue(
    sessions === "error"
      ? errorResponse(500, "internal", "boom")
      : jsonResponse(aSessionPage(sessions)),
  )
  const open = vi.fn()
  const goal = aGoal({ status: "planning" })
  renderScreen(<PlanningGuide goal={goal} onOpenConversation={open} onNewTask={vi.fn()} />)
  return open
}

const orchestrator = (overrides: Partial<SessionDto>) =>
  aSession({ id: "orch", seat: "orchestrator", task_id: null, ...overrides })

it.each<[Partial<SessionDto>, string]>([
  [{ status: "starting" }, "The orchestrator is starting"],
  [{ status: "running" }, "The orchestrator is working on the plan"],
  [{ status: "idle", attention_reason: "waiting_input" }, "The orchestrator is waiting for you"],
  [
    { status: "running", attention_reason: "waiting_permission" },
    "The orchestrator is waiting for you",
  ],
  [{ status: "idle" }, "The orchestrator is idle"],
  [{ status: "exited" }, "The planning conversation has ended"],
])("words a recorded %j session as %s", async (overrides, headline) => {
  const open = mount([orchestrator(overrides)])
  expect(await screen.findByText(headline)).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "Open orchestrator conversation" }))
  expect(open).toHaveBeenCalledWith("orch")
})

it("claims no progress when no session is recorded, and keeps manual task creation", async () => {
  const open = mount([])
  expect(await screen.findByText("No planning conversation recorded yet")).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Open orchestrator conversation" })).toBeNull()
  expect(screen.getByRole("button", { name: "Create task manually" })).toBeTruthy()
  expect(open).not.toHaveBeenCalled()
})

it("says the state is unavailable when the sessions cannot be read", async () => {
  mount("error")
  expect(await screen.findByText("Planning state is unavailable")).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Open orchestrator conversation" })).toBeNull()
})

it("says the state is unavailable when a refetch fails over cached sessions", async () => {
  daemonFetch.mockResolvedValueOnce(
    jsonResponse(aSessionPage([orchestrator({ status: "running" })])),
  )
  const { queryClient } = renderScreen(
    <PlanningGuide
      goal={aGoal({ status: "planning" })}
      onOpenConversation={vi.fn()}
      onNewTask={vi.fn()}
    />,
  )
  expect(await screen.findByText("The orchestrator is working on the plan")).toBeTruthy()
  daemonFetch.mockResolvedValue(errorResponse(500, "internal", "boom"))
  await queryClient.invalidateQueries()
  expect(await screen.findByText("Planning state is unavailable")).toBeTruthy()
})

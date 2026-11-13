// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"
import type { AttentionItemDto } from "@/api"
import { aGoal, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionPopover } from "./attention-strip"

const goal = aGoal()
const task = aTask({ goal_id: goal.id })
let recovery: AttentionItemDto[]
let complete = true
let failSessions = false
beforeEach(() => {
  recovery = []
  complete = true
  failSessions = false
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname === "/v1/goals") return Promise.resolve(jsonResponse([goal]))
    if (url.pathname === "/v1/tasks") return Promise.resolve(jsonResponse([task]))
    if (url.pathname === "/v1/attention")
      return Promise.resolve(jsonResponse({ items: recovery, complete }))
    if (failSessions) return Promise.resolve(jsonResponse({ message: "offline" }, 503))
    return Promise.resolve(jsonResponse(aSessionPage([])))
  })
})
function item(): AttentionItemDto {
  return {
    id: "recovery:resource:test",
    producer: "recovery",
    reason: "resource",
    summary: "Resource limit reached",
    required_action: "Free resources",
    since: "2026-01-01T00:00:00Z",
    affected: [],
    target: { kind: "task", task_id: task.id },
  }
}
it("hides after a complete successful empty read", async () => {
  recovery = []
  const { queryClient } = renderScreen(<AttentionPopover />, { route: "/profiles" })
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  expect(screen.queryByRole("button", { name: /Needs attention/ })).toBeNull()
})
it("shows unavailable retry and keeps partial items visible", async () => {
  failSessions = true
  const { queryClient } = renderScreen(<AttentionPopover />, { route: "/profiles" })
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  expect(screen.getByRole("button", { name: /Needs attention/ })).not.toBeNull()
  fireEvent.click(screen.getByRole("button", { name: /Needs attention/ }))
  expect(await screen.findByText(/Could not load attention/)).not.toBeNull()
  expect(screen.getByRole("button", { name: "Retry" })).not.toBeNull()
})
it("opens the item target, closes on selection, and closes on Escape", async () => {
  recovery = [item()]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  const trigger = await screen.findByRole("button", { name: /Needs attention/ })
  fireEvent.click(trigger)
  expect(await screen.findByText("Resource limit reached")).not.toBeNull()
  fireEvent.keyDown(document, { key: "Escape" })
  await waitFor(() => expect(screen.queryByText("Resource limit reached")).toBeNull())
  fireEvent.click(trigger)
  fireEvent.click(await screen.findByText("Resource limit reached"))
  await waitFor(() => expect(screen.queryByText("Resource limit reached")).toBeNull())
})

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
function item(
  target: AttentionItemDto["target"] = { kind: "task", task_id: task.id },
): AttentionItemDto {
  return {
    id: "recovery:resource:test",
    producer: "recovery",
    reason: "resource",
    summary: "Resource limit reached",
    required_action: "Free resources",
    since: "2026-01-01T00:00:00Z",
    affected: [],
    target,
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
it("opens a drawer from View all and filters a goal-less item by goal", async () => {
  recovery = [item()]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  fireEvent.click(await screen.findByRole("button", { name: /Needs attention/ }))
  fireEvent.click(await screen.findByRole("button", { name: "View all" }))
  expect(await screen.findByText("Showing 1 of 1")).not.toBeNull()
  expect(screen.getAllByText("Free resources").length).toBeGreaterThan(0)
  fireEvent.change(screen.getByLabelText("Goal"), { target: { value: "__no_goal__" } })
  expect(await screen.findByText("Showing 1 of 1")).not.toBeNull()
  fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "recovery:resource" } })
  expect(screen.getByText("Resource limit reached")).not.toBeNull()
})

async function openDrawer() {
  const trigger = await screen.findByRole("button", { name: /Needs attention/ })
  fireEvent.click(trigger)
  fireEvent.click(await screen.findByRole("button", { name: "View all" }))
  return trigger
}
it("labels a console row and a task row by where each one navigates", async () => {
  recovery = [
    item({ kind: "console", session_id: "01JSESS0000000000000000009" }),
    { ...item(), id: "recovery:resource:other", summary: "Other limit" },
  ]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  expect(await screen.findByText("Open console")).not.toBeNull()
  expect(screen.getByText("Open task")).not.toBeNull()
})
it("navigates to the row target from the drawer and closes it", async () => {
  recovery = [item()]
  const { location } = renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  fireEvent.click(await screen.findByText("Open task"))
  await waitFor(() => expect(screen.queryByText("Showing 1 of 1")).toBeNull())
  expect(location.url).toContain(`task=${task.id}`)
})
it("returns focus to the trigger when the drawer closes", async () => {
  recovery = [item()]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  const trigger = await openDrawer()
  await screen.findByText("Showing 1 of 1")
  fireEvent.keyDown(document, { key: "Escape" })
  await waitFor(() => expect(screen.queryByText("Showing 1 of 1")).toBeNull())
  await waitFor(() => expect(document.activeElement).toBe(trigger))
})
it("narrows the drawer list by reason and counts what is shown", async () => {
  recovery = [
    item(),
    { ...item(), id: "recovery:access:other", reason: "access", summary: "Sign in to continue" },
  ]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  expect(await screen.findByText("Showing 2 of 2")).not.toBeNull()
  fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "recovery:access" } })
  expect(screen.getByText("Showing 1 of 2")).not.toBeNull()
  expect(screen.queryByText("Resource limit reached")).toBeNull()
})
it("keeps items and reports a partial read in the drawer", async () => {
  recovery = [item()]
  failSessions = true
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  expect(await screen.findByText(/Some items could not load/)).not.toBeNull()
  expect(screen.getByText("Showing 1 of 1")).not.toBeNull()
})
it("reports a failed read in the drawer when nothing loaded", async () => {
  failSessions = true
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  expect(await screen.findByText(/Could not load attention/)).not.toBeNull()
})
it("does not return focus to the trigger after a row is picked", async () => {
  recovery = [item({ kind: "console", session_id: "01JSESS0000000000000000009" })]
  renderScreen(<AttentionPopover />, { route: "/profiles" })
  const trigger = await openDrawer()
  fireEvent.click(await screen.findByText("Open console"))
  await waitFor(() => expect(screen.queryByText("Showing 1 of 1")).toBeNull())
  await new Promise((resolve) => requestAnimationFrame(resolve))
  expect(document.activeElement).not.toBe(trigger)
})
it("drops a filter whose last item resolved instead of hiding every row", async () => {
  recovery = [
    item(),
    { ...item(), id: "recovery:access:other", reason: "access", summary: "Sign in to continue" },
  ]
  const { queryClient } = renderScreen(<AttentionPopover />, { route: "/profiles" })
  await openDrawer()
  await screen.findByText("Showing 2 of 2")
  fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "recovery:access" } })
  expect(screen.getByText("Showing 1 of 2")).not.toBeNull()
  recovery = [item()]
  await queryClient.invalidateQueries()
  expect(await screen.findByText("Showing 1 of 1")).not.toBeNull()
  expect((screen.getByLabelText("Reason") as HTMLSelectElement).value).toBe("")
})

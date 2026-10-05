// @vitest-environment jsdom
import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"
import { qk } from "@/api"
import { GoalSwimlanes } from "@/features/goals/goal-swimlanes"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { stubWebSocket } from "@/test/web-socket"
import { DetailPanels } from "./detail-panels"

const goal = aGoal({ id: "g1", title: "Focus goal", status: "active" })
const task = aTask({ id: "t1", goal_id: goal.id, title: "Focus task" })
const session = aSession({ id: "s1", goal_id: goal.id })

beforeEach(() => {
  stubWebSocket()
  daemonFetch.mockImplementation((input) => {
    const request = input instanceof Request ? input : new Request(String(input))
    const path = new URL(request.url).pathname
    if (request.method === "POST") {
      const row = path.includes("/goals/") ? goal : task
      return Promise.resolve(
        jsonResponse({ ...row, status: path.endsWith("/retry") ? "ready" : "cancelled" }),
      )
    }
    if (path === "/v1/goals/g1") return Promise.resolve(jsonResponse(goal))
    if (path === "/v1/tasks/t1") return Promise.resolve(jsonResponse(task))
    if (path === "/v1/tasks") return Promise.resolve(jsonResponse([task]))
    return new Promise(() => {})
  })
})

function mount(route: string, status = task.status) {
  return renderScreen(
    <>
      <GoalSwimlanes goals={[goal]} />
      <DetailPanels />
    </>,
    {
      route,
      seed: (client) => {
        client.setQueryData(qk.goals.detail(goal.id), goal)
        client.setQueryData(qk.tasks.detail(task.id), { ...task, status })
        client.setQueryData(qk.tasks.list({}), [task])
        client.setQueryData(qk.tasks.list({ goal: goal.id }), [task])
        client.setQueryData(qk.sessions.detail(session.id), session)
        client.setQueryData(
          qk.tasks.diff(task.id),
          "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n",
        )
      },
    },
  )
}

async function expectPaneFocus() {
  await waitFor(() =>
    expect(
      document.querySelector('[data-slot="docked-pane"]')?.contains(document.activeElement),
    ).toBe(true),
  )
}

it("keeps focus in the task pane after opening a goal from the board", async () => {
  mount("/goals")
  const user = userEvent.setup()
  await user.click(screen.getByRole("link", { name: goal.title }))
  const pane = await screen.findByRole("region", { name: goal.title })
  await user.click(within(pane).getByRole("link", { name: new RegExp(task.title) }))
  await screen.findByRole("heading", { name: task.title })
  await expectPaneFocus()
})

it.each([
  ["Cancel goal", "/goals?goal=g1"],
  ["Cancel task", "/goals?task=t1"],
] as const)("keeps focus in the pane after confirming %s", async (label, route) => {
  mount(route, "in_progress")
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: label }))
  const dialog = await screen.findByRole("dialog")
  await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true))
  await user.click(within(dialog).getByRole("button", { name: label }))
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  await expectPaneFocus()
})

it("keeps focus in the pane after Retry task removes its button", async () => {
  mount("/goals?task=t1", "failed")
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Retry task" }))
  await waitFor(() => expect(screen.queryByRole("button", { name: "Retry task" })).toBeNull())
  await expectPaneFocus()
})

it.each([
  ["console", "/goals?session=s1"],
  ["diff", "/goals?task=t1&tab=diff"],
])("keeps focus in the pane after collapsing the %s", async (name, route) => {
  mount(route)
  const user = userEvent.setup()
  await user.click(await screen.findByRole("button", { name: `Expand the ${name}` }))
  await user.click(
    await screen.findByRole("button", { name: `Collapse the ${name} back into the panel` }),
  )
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  await expectPaneFocus()
})

it.each(["task", "goal breadcrumb"])(
  "returns focus to the board opener after closing the replacement %s",
  async (replacement) => {
    mount("/goals")
    const user = userEvent.setup()
    const opener = screen.getByRole("link", { name: goal.title })
    await user.click(opener)
    const pane = await screen.findByRole("region", { name: goal.title })
    await user.click(within(pane).getByRole("link", { name: new RegExp(task.title) }))
    await screen.findByRole("heading", { name: task.title })
    if (replacement === "goal breadcrumb") {
      await user.click(await screen.findByRole("button", { name: goal.title }))
      await screen.findByRole("region", { name: goal.title })
    }
    await user.keyboard("{Escape}")
    await waitFor(() => expect(document.querySelector('[data-slot="docked-pane"]')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
  },
)

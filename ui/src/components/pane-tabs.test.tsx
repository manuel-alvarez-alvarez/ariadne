// @vitest-environment jsdom

/**
 * The tabs of each pane (spec 015, rule 110): each tab is named by its word
 * alone, even with a count beside it, and draws the icon of its concept. The
 * panes are mounted whole, so a tab that lost its icon or took another
 * concept's fails here and not only in the unit of `TabsTrigger`.
 */

import { render, screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { GoalPanel } from "@/features/goals/goal-panel"
import { PullRequestPanel } from "@/features/pull-requests/pull-request-panel"
import { SessionPanel } from "@/features/sessions/session-panel"
import { TaskPanel } from "@/features/tasks/task-panel"
import { TAB_ICONS } from "@/lib/tab-icons"
import { stubEventSource } from "@/test/event-source"
import { aGoal, aSession, aSessionPage, aTask, GOAL_ID } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"

type Concept = keyof typeof TAB_ICONS

const session = aSession({
  id: "01JSESS0000000000000000001",
  status: "idle",
  goal_id: GOAL_ID,
  task_id: null,
  seat: "orchestrator",
})
const task = aTask()

beforeEach(() => {
  stubEventSource()
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const path = new URL(input instanceof Request ? input.url : String(input)).pathname
    if (path === `/v1/sessions/${session.id}`) return jsonResponse(session)
    if (path === "/v1/sessions") return jsonResponse(aSessionPage([session]))
    if (path === "/v1/tasks") return jsonResponse([task])
    if (path === "/v1/attention") return jsonResponse({ items: [] })
    if (path.startsWith("/v1/pull-requests/") || path.includes("/pull-requests/")) {
      return jsonResponse(pull)
    }
    return jsonResponse([])
  })
})

/** The `lucide-<name>` class that draws a concept's icon. */
function iconClass(concept: Concept): string {
  const Icon = TAB_ICONS[concept]
  const { container, unmount } = render(<Icon />)
  const name = [...(container.querySelector("svg")?.classList ?? [])].find(
    (each) => each.startsWith("lucide-") && each !== "lucide",
  )
  unmount()
  if (!name) throw new Error(`no icon for ${concept}`)
  return name
}

/** The tab named exactly `name`, drawn with the icon of `concept`. */
async function expectTab(name: string, concept: Concept) {
  const tab = await screen.findByRole("tab", { name })
  const svg = tab.querySelector("svg")
  expect(svg?.classList.contains(iconClass(concept)), `${name} icon`).toBe(true)
  return tab
}

it("names each task pane tab by its word and draws its icon, count included", async () => {
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.goals.detail("local", GOAL_ID), aGoal({ id: GOAL_ID }))
    },
  })

  await expectTab("Description", "description")
  await expectTab("History", "history")
  await expectTab("Diff", "diff")
  const sessions = await expectTab("Sessions", "sessions")
  expect(await within(sessions).findByLabelText("1 session")).toBeTruthy()
})

it("names each goal pane tab by its word and draws its icon, count included", async () => {
  renderScreen(<GoalPanel host="local" goalId={GOAL_ID} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.goals.detail("local", GOAL_ID), aGoal()),
  })

  await expectTab("Description", "description")
  const tasks = await expectTab("Tasks", "tasks")
  expect(await within(tasks).findByLabelText("1 task")).toBeTruthy()
  const sessions = await expectTab("Sessions", "sessions")
  expect(await within(sessions).findByLabelText("1 session")).toBeTruthy()
})

it("names each pull request pane tab by its word and draws its icon", async () => {
  renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />)

  await expectTab("Description", "description")
  await expectTab("Sessions", "sessions")
})

it("names each session pane tab by its word and draws its icon", async () => {
  renderScreen(<SessionPanel host="local" sessionId={session.id} onClose={() => {}} />)

  await expectTab("Console", "console")
  await expectTab("Agent activity", "activity")
})

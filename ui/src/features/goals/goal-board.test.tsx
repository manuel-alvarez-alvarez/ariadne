// @vitest-environment jsdom
import { act, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { AttentionListDto, GoalDto, SessionDto, TaskDto } from "@/api"
import { dispatchDomainEvent } from "@/events/dispatch"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { GoalBoard } from "./goal-board"

const GOAL_TITLE = "Ship the board"

afterEach(() => forgetHosts())
beforeEach(() => localStorage.clear())

/** The daemon of one host: these tasks, sessions and attention items, nothing else. */
function stubDaemon(
  tasks: TaskDto[],
  sessions: SessionDto[] = [],
  attention: AttentionListDto = { items: [], complete: true },
) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname === "/v1/tasks") return Promise.resolve(jsonResponse(tasks))
    if (url.pathname === "/v1/attention") return Promise.resolve(jsonResponse(attention))
    if (url.pathname === "/v1/sessions")
      return Promise.resolve(jsonResponse(aSessionPage(sessions)))
    return Promise.resolve(jsonResponse([]))
  })
}

function board(goals: GoalDto[], props: { activeOnly?: boolean } = {}) {
  return renderScreen(
    <GoalBoard
      hosts={["local"]}
      goals={goals.map((goal) => ({ ...goal, host: "local" }))}
      {...props}
    />,
  )
}

/** Each row's own text, top to bottom: a task row, a folded goal or a placeholder. */
function rowTitles(): string[] {
  return [
    ...document.querySelectorAll("[data-task-row], [data-goal-folded], [data-goal-placeholder]"),
  ].map((row) =>
    row.matches("[data-task-row]")
      ? (row.querySelector("a span")?.textContent ?? "")
      : (row.querySelector("a")?.getAttribute("aria-label") ??
        row.querySelector("a")?.textContent ??
        ""),
  )
}

it("draws each task as one row of one list, the rows of a goal in its group, and counts them", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([
    aTask({ id: "one", goal_id: goal.id, title: "First", status: "pending" }),
    aTask({ id: "two", goal_id: goal.id, title: "Second", status: "in_progress" }),
  ])
  board([goal])

  expect(await screen.findByRole("heading", { name: "Tasks (2)" })).toBeTruthy()
  const group = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  const rows = group.querySelectorAll("[data-task-row]")
  expect([...rows].map((row) => row.querySelector("a span")?.textContent)).toEqual([
    "First",
    "Second",
  ])
  expect(screen.queryByText("No goal")).toBeNull()
})

it("makes a row one link to the task panel with one tab stop, its pull request outside the link", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([
    aTask({
      id: "t1",
      goal_id: goal.id,
      title: "With a request",
      status: "failed",
      reason: "boom",
      pr_url: "https://github.com/acme/app/pull/412",
    }),
  ])
  board([goal])

  const title = await screen.findByText("With a request")
  const row = title.closest("[data-task-row]") as HTMLElement
  const links = within(row).getAllByRole("link")
  const own = links.find((link) => link.contains(title))
  expect(own?.getAttribute("href")).toContain("task=t1")
  expect(within(own as HTMLElement).getByText("Failed")).toBeTruthy()
  const pr = within(row).getByRole("link", { name: /Pull request #412/ })
  expect(own?.contains(pr)).toBe(false)
  const stops = [...row.querySelectorAll<HTMLElement>("a, button, [tabindex]")].filter(
    (node) => node.tabIndex >= 0,
  )
  expect(stops).toEqual([own])
  expect(row.className).toContain("border-status-danger/40")
})

it("hides terminal tasks under Active while preserving progress", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([
    aTask({ id: "one", goal_id: goal.id, title: "Waiting", status: "pending" }),
    aTask({ id: "two", goal_id: goal.id, title: "Complete", status: "finished" }),
    aTask({ id: "three", goal_id: goal.id, title: "Cancelled task", status: "cancelled" }),
  ])
  board([goal], { activeOnly: true })

  expect(await screen.findByRole("heading", { name: "Tasks (1)" })).toBeTruthy()
  expect(screen.getByText("Waiting")).toBeTruthy()
  expect(screen.queryByText("Complete")).toBeNull()
  expect(screen.queryByText("Cancelled task")).toBeNull()
  expect(screen.getByText("1/2")).toBeTruthy()
})

it("puts a goal bar over the rows of its goal: toggle, title link, status, progress and tokens", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  stubDaemon([
    aTask({ id: "t1", goal_id: goal.id, status: "finished" }),
    aTask({ id: "t2", goal_id: goal.id, status: "in_progress" }),
  ])
  board([goal])

  await screen.findByRole("heading", { name: "Tasks (2)" })
  const link = screen.getByRole("link", { name: GOAL_TITLE })
  expect(link.getAttribute("href")).toContain(`goal=${goal.id}`)
  const bar = link.closest("[data-goal-bar]") as HTMLElement
  expect(bar.getAttribute("data-goal-bar")).toBe("hover")
  expect(within(bar).getByRole("button", { name: `Collapse ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(bar).getByText("Active")).toBeTruthy()
  expect(await within(bar).findByText("1/2")).toBeTruthy()
  expect(within(bar).getByText(/out/)).toBeTruthy()
  // The bar and its frame float over the gap: neither is a row of the list.
  const group = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(group.querySelector("[data-goal-frame]")).toBeTruthy()
  expect(bar.querySelector("[data-task-row]")).toBeNull()
})

it("keeps the goal bar shown at rest while its orchestrator wants a person", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  const orchestrator = aSession({
    goal_id: goal.id,
    task_id: null,
    status: "running",
    attention_reason: "waiting_permission",
  })
  stubDaemon([aTask({ goal_id: goal.id, status: "in_progress" })], [orchestrator], {
    items: [
      {
        id: `agent_request:waiting_permission:${orchestrator.id}`,
        occurrence: "o-1",
        producer: "agent_request",
        reason: "unknown",
        summary: "Permission approval needed",
        required_action: "Answer in the console.",
        since: "2026-01-01T00:00:00Z",
        affected: [{ kind: "session", id: orchestrator.id, label: GOAL_TITLE }],
        target: { kind: "console", session_id: orchestrator.id },
      },
    ],
    complete: true,
  })
  board([goal])

  const badge = await screen.findByText("Waiting for permission")
  const bar = badge.closest("[data-goal-bar]") as HTMLElement
  expect(bar.getAttribute("data-goal-bar")).toBe("pinned")
})

it("folds a finished goal to one summary row", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "completed" })
  stubDaemon([aTask({ goal_id: goal.id, title: "Done work", status: "finished" })])
  board([goal])

  const summary = await screen.findByText("1 task finished")
  const row = summary.closest("[data-goal-folded]") as HTMLElement
  expect(within(row).getByRole("button", { name: `Expand ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(row).getByRole("link", { name: GOAL_TITLE })).toBeTruthy()
  expect(within(row).getByText("Completed")).toBeTruthy()
  expect(screen.queryByText("Done work")).toBeNull()
})

it("draws a goal with no shown task as one placeholder row that opens the goal", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "planning" })
  stubDaemon([])
  board([goal])

  const state = await screen.findByText("No tasks yet — the orchestrator is still working")
  const row = screen.getByRole("link", { name: GOAL_TITLE })
  expect(row.hasAttribute("data-goal-placeholder")).toBe(true)
  expect(row.contains(state)).toBe(true)
  expect(row.getAttribute("href")).toContain(`goal=${goal.id}`)
  expect(within(row).getByText("Planning")).toBeTruthy()
  expect(screen.getByRole("heading", { name: "Tasks (0)" })).toBeTruthy()
})

it("shows the execution host of a projected task in its row", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([aTask({ id: "remote", goal_id: goal.id, title: "Remote work", host: "worker" })])
  board([goal])
  expect(await screen.findByText("Remote work")).toBeTruthy()
  expect(screen.getByText("worker")).toBeTruthy()
})

it("draws each goal-less task as a group of one row, in no goal's group", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([
    aTask({ id: "g1", goal_id: goal.id, title: "In a goal" }),
    aTask({ id: "n1", goal_id: null, title: "Loose one", steps: [] }),
    aTask({ id: "n2", goal_id: null, title: "Loose two", steps: [] }),
  ])
  board([goal])

  const loose = await screen.findByText("Loose one")
  expect(loose.closest("[role=group]")).toBeNull()
  expect(screen.getByText("Loose two").closest("[role=group]")).toBeNull()
  const group = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(within(group).queryByText("Loose one")).toBeNull()
  expect(screen.getByRole("heading", { name: "Tasks (3)" })).toBeTruthy()
})

it("sorts the groups: what needs the user, then live, then finished, newest change first", async () => {
  const live = aGoal({ id: "g-live", title: "Live goal", updated_at: "2026-01-02T00:00:00Z" })
  const done = aGoal({
    id: "g-done",
    title: "Done goal",
    status: "completed",
    updated_at: "2026-01-05T00:00:00Z",
  })
  stubDaemon([
    aTask({ id: "l1", goal_id: live.id, title: "Live task", status: "in_progress" }),
    aTask({ id: "d1", goal_id: done.id, title: "Done task", status: "finished" }),
    aTask({
      id: "n-new",
      goal_id: null,
      title: "Newer loose task",
      status: "in_progress",
      updated_at: "2026-01-03T00:00:00Z",
      steps: [],
    }),
    aTask({
      id: "n-failed",
      goal_id: null,
      title: "Failed loose task",
      status: "failed",
      updated_at: "2026-01-01T00:00:00Z",
      steps: [],
    }),
    aTask({
      id: "n-finished",
      goal_id: null,
      title: "Finished loose task",
      status: "finished",
      updated_at: "2026-01-04T00:00:00Z",
      steps: [],
    }),
  ])
  board([live, done])

  await screen.findByText("Live task")
  expect(rowTitles()).toEqual([
    "Failed loose task",
    "Newer loose task",
    "Live task",
    "Done goal",
    "Finished loose task",
  ])
})

it("shows a far goal-less task once, though its source and its worker both list it", async () => {
  const task = aTask({
    id: "far-task",
    goal_id: null,
    title: "Far loose task",
    execution_context: {
      source_goal: null,
      source_daemon_id: "source",
      execution_owner: "worker",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  const EMPTY = { "/v1/attention": { items: [], complete: true }, "/v1/sessions": aSessionPage([]) }
  stubHosts({
    local: {
      ...EMPTY,
      "/v1/tasks": [task],
      "/v1/remote-tasks/protocol": { protocol: "remote-tasks-v1", daemon_id: "source" },
    },
    box: {
      ...EMPTY,
      "/v1/tasks": [task],
      "/v1/remote-tasks/protocol": { protocol: "remote-tasks-v1", daemon_id: "worker" },
    },
  })
  renderScreen(<GoalBoard hosts={["local", "box"]} goals={[]} />)
  expect(await screen.findByText("Far loose task")).toBeTruthy()
  expect(await screen.findByRole("heading", { name: "Tasks (1)" })).toBeTruthy()
  expect(screen.getAllByText("Far loose task")).toHaveLength(1)
})

it("adds a goal-less task's row when its task_created event arrives", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  const loose = aTask({ id: "n1", goal_id: null, title: "Fresh and loose", steps: [] })
  stubDaemon([aTask({ goal_id: goal.id })])
  const { queryClient } = board([goal])
  await screen.findByRole("heading", { name: "Tasks (1)" })
  expect(screen.queryByText("Fresh and loose")).toBeNull()

  stubDaemon([aTask({ goal_id: goal.id }), loose])
  act(() => dispatchDomainEvent(queryClient, "local", { event: "task_created", data: loose }))

  expect(await screen.findByText("Fresh and loose")).toBeTruthy()
  expect(screen.getByRole("heading", { name: "Tasks (2)" })).toBeTruthy()
})

it("moves a task's row into its goal when its task_updated event arrives", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  const loose = aTask({ id: "n1", goal_id: null, title: "Moving task", steps: [] })
  stubDaemon([loose])
  const { queryClient } = board([goal])
  expect((await screen.findByText("Moving task")).closest("[role=group]")).toBeNull()

  const moved = { ...loose, goal_id: goal.id }
  stubDaemon([moved])
  act(() =>
    dispatchDomainEvent(queryClient, "local", { event: "task_updated", data: { task: moved } }),
  )

  const group = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(await within(group).findByText("Moving task")).toBeTruthy()
})

it("shows only the empty state when the board has no goal and no task", async () => {
  stubDaemon([])
  renderScreen(<GoalBoard hosts={["local"]} goals={[]} empty={<p>Nothing here</p>} />)

  expect(await screen.findByText("Nothing here")).toBeTruthy()
  expect(screen.queryByRole("heading", { name: /Tasks/ })).toBeNull()
})

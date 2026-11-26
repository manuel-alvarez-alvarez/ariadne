// @vitest-environment jsdom
import { act, fireEvent, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { AttentionListDto, GoalDto, SessionDto, TaskDto } from "@/api"
import { dispatchDomainEvent } from "@/events/dispatch"
import { aGoal, aSession, aSessionPage, aTask, aWorkflow } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
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

function board(goals: GoalDto[], props: { activeOnly?: boolean; showHost?: boolean } = {}) {
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
  return [...document.querySelectorAll("[data-task-row], [data-goal-folded]")].map((row) =>
    row.matches("[data-task-row]")
      ? (row.querySelector("a span")?.textContent ?? "")
      : (row.querySelector("a")?.textContent ?? ""),
  )
}

/** The classes that would draw a box around an element. */
const BOX = /(^|\s)(border|border-dashed|bg-[\w/-]+|ring|shadow[\w-]*)(\s|$)/

/** Whether something on the way from `node` up to the board hides it until hover or focus. */
function hiddenAtRest(node: HTMLElement): boolean {
  for (let at: HTMLElement | null = node; at; at = at.parentElement) {
    if (/(^|\s)(opacity-0|hidden|invisible)(\s|$)/.test(at.className)) return true
  }
  return false
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
  // The rows speak for themselves: no line of column names over them.
  expect(screen.queryAllByRole("columnheader")).toEqual([])
  expect(document.querySelectorAll("[data-goal-line]")).toHaveLength(1)
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
})

it("draws a row with no border and no fill, its rail segment in the status colour", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([
    aTask({ id: "t1", goal_id: goal.id, title: "Broke", status: "failed", reason: "boom" }),
    aTask({ id: "t2", goal_id: goal.id, title: "Done", status: "finished" }),
    aTask({ id: "t3", goal_id: goal.id, title: "Waiting", status: "pending" }),
    aTask({ id: "n1", goal_id: null, title: "Loose", status: "cancelled", steps: [] }),
  ])
  board([goal])

  const rail = async (title: string) => {
    const row = (await screen.findByText(title)).closest("[data-task-row]") as HTMLElement
    expect(row.className).not.toMatch(BOX)
    // Hover and keyboard focus tint the row; nothing else draws it.
    expect(row.className).toContain("hover:bg-muted/60")
    expect(row.className).toContain("has-[a:focus-visible]:bg-muted/60")
    return (row.querySelector("[data-rail]") as HTMLElement).className
  }
  expect(await rail("Broke")).toContain("border-status-danger/40")
  expect(await rail("Done")).toContain("border-status-done/40")
  expect(await rail("Waiting")).toContain("border-border")
  // A task with no goal has its segment too.
  expect(await rail("Loose")).toContain("border-muted-foreground/40")
})

it("puts the token figures in one column, the goal's total over its rows', never cut", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  stubDaemon([aTask({ id: "t1", goal_id: goal.id, title: "Spend", status: "in_progress" })])
  board([goal])

  const row = (await screen.findByText("Spend")).closest("[data-task-row]") as HTMLElement
  const line = document.querySelector("[data-goal-line]") as HTMLElement
  const rowTokens = row.querySelector("[data-column=tokens]") as HTMLElement
  const goalTokens = line.querySelector("[data-column=tokens]") as HTMLElement
  expect(within(goalTokens).getByText(/out/)).toBeTruthy()
  expect(within(rowTokens).getByText(/out/)).toBeTruthy()
  // One grid track holds both, sized to the widest figure, and neither cell clips.
  const track = (cell: HTMLElement) => cell.className.match(/col-start-\d/)?.[0]
  expect(track(goalTokens)).toBe("col-start-5")
  expect(track(rowTokens)).toBe("col-start-5")
  for (const cell of [goalTokens, rowTokens])
    expect(cell.className).not.toMatch(/truncate|overflow-hidden|max-w-/)
  const list = line.closest("[role=group]")?.parentElement as HTMLElement
  expect(list.className).toContain("max-content")
})

it("fits its tracks to any board width: only the token column has a fixed floor", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([aTask({ goal_id: goal.id, title: "Fit" })])
  board([goal])

  const row = (await screen.findByText("Fit")).closest("[data-task-row]") as HTMLElement
  const list = row.closest("[role=group]")?.parentElement as HTMLElement
  const templates = [...list.className.matchAll(/grid-cols-\[([^\]]+)\]/g)].map((m) => m[1] ?? "")
  // One template for a wide board and one for a narrow board, seven tracks each.
  expect(templates).toHaveLength(2)
  for (const template of templates) {
    const tracks = template.split("_")
    expect(tracks).toHaveLength(7)
    // The fifth is the token column: as wide as its widest figure.
    expect(tracks[4]).toBe("max-content")
    // The title keeps a share of the board, so it shrinks with the board.
    expect(tracks[0]).toBe("minmax(min(6rem,25%),1fr)")
    // The others can shrink to nothing; `auto` holds only a column that hides.
    for (const track of tracks.slice(1, 4))
      expect(track === "auto" || track.startsWith("minmax(0,")).toBe(true)
    expect(tracks.slice(5)).toEqual(["auto", "auto"])
  }
})

it("hides a goal's host on its goal line with the host column", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  stubDaemon([aTask({ goal_id: goal.id, title: "Hosted", host: "worker" })])
  board([goal], { showHost: true })

  const row = (await screen.findByText("Hosted")).closest("[data-task-row]") as HTMLElement
  const line = document.querySelector("[data-goal-line]") as HTMLElement
  const hides = (node: Element) =>
    node.className.match(/@max-\[[\d.]+rem\]\/board:hidden/)?.[0] ?? null
  const rowHost = row.querySelector("[data-column=host]") as HTMLElement
  const goalHost = within(line).getByText("local")
  expect(hides(rowHost)).not.toBeNull()
  expect(hides(goalHost)).toBe(hides(rowHost))
})

it("hides whole columns by the board's own width: step names, then activity, host and PR link", async () => {
  const goal = aGoal({ title: GOAL_TITLE, steps: aWorkflow().steps })
  stubDaemon([
    aTask({
      id: "t1",
      goal_id: goal.id,
      title: "Narrow",
      status: "in_progress",
      step: "develop",
      host: "worker",
      pr_url: "https://github.com/acme/app/pull/7",
    }),
  ])
  board([goal])

  const row = (await screen.findByText("Narrow")).closest("[data-task-row]") as HTMLElement
  const section = screen.getByRole("region", { name: "Goals board" })
  // The board is the container the widths are read from, and it never scrolls sideways.
  expect(section.className).toContain("@container/board")
  expect(section.className).toContain("overflow-x-hidden")
  const below = (column: string) => {
    const cell = row.querySelector(`[data-column=${column}]`) as HTMLElement
    const at = cell.className.match(
      /@max-\[(\d+(?:\.\d+)?)rem\]\/board:(?:\[&_\[data-step-name\]\]:)?hidden/,
    )
    return at ? Number(at[1]) : null
  }
  const steps = below("steps") as number
  const activity = below("activity") as number
  const host = below("host") as number
  const pr = below("pr") as number
  expect(steps).toBeGreaterThan(activity)
  expect(activity).toBeGreaterThan(host)
  expect(host).toBeGreaterThan(pr)
  // A pinned inspector leaves the board 36rem to 42rem wide: it hides all four.
  expect(pr).toBeLessThanOrEqual(42)
  for (const column of ["title", "state", "tokens"]) expect(below(column)).toBeNull()
  // The step cell hides the names alone; its dots stay.
  const track = within(row).getByRole("list", { name: "Workflow steps" })
  expect(track.querySelectorAll("[data-step-name]").length).toBeGreaterThan(0)
  expect((row.querySelector("[data-column=title]") as HTMLElement).className).toContain("truncate")
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

it("puts one goal line over the rows of its goal, shown at rest: toggle, title link, status, progress and tokens", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  stubDaemon([
    aTask({ id: "t1", goal_id: goal.id, status: "finished" }),
    aTask({ id: "t2", goal_id: goal.id, status: "in_progress" }),
  ])
  board([goal])

  await screen.findByRole("heading", { name: "Tasks (2)" })
  const link = screen.getByRole("link", { name: GOAL_TITLE })
  expect(link.getAttribute("href")).toContain(`goal=${goal.id}`)
  const line = link.closest("[data-goal-line]") as HTMLElement
  expect(within(line).getByRole("button", { name: `Collapse ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(line).getByText("Active")).toBeTruthy()
  expect(await within(line).findByText("1/2")).toBeTruthy()
  expect(within(line).getByText(/out/)).toBeTruthy()
  expect(line.querySelector("[data-task-row]")).toBeNull()
  // Visible with no hover and no focus, and drawn as plain text: no box, no frame.
  expect(hiddenAtRest(line)).toBe(false)
  expect(line.className).not.toMatch(BOX)
  const group = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(group.className).not.toMatch(BOX)
  // Nothing is laid over the rows: the only drawn pieces past the text are the rails.
  expect(
    [...group.querySelectorAll(".absolute")].every((node) => node.matches("[data-rail]")),
  ).toBe(true)
})

it("shows the orchestrator's attention badge on the goal line", async () => {
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
  const line = badge.closest("[data-goal-line]") as HTMLElement
  expect(within(line).getByRole("link", { name: GOAL_TITLE })).toBeTruthy()
  expect(hiddenAtRest(line)).toBe(false)
})

it("folds a finished goal to its goal line with the summary, and no border", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "completed" })
  stubDaemon([aTask({ goal_id: goal.id, title: "Done work", status: "finished" })])
  board([goal])

  const summary = await screen.findByText("1 task finished")
  const row = summary.closest("[data-goal-folded]") as HTMLElement
  expect(row.hasAttribute("data-goal-line")).toBe(true)
  expect(row.className).not.toMatch(BOX)
  expect(within(row).getByRole("button", { name: `Expand ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(row).getByRole("link", { name: GOAL_TITLE })).toBeTruthy()
  expect(within(row).getByText("Completed")).toBeTruthy()
  expect(within(row).getByText(/out/)).toBeTruthy()
  expect(screen.queryByText("Done work")).toBeNull()
})

it("draws a goal with no shown task as its goal line, then one muted line with the state", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "planning" })
  stubDaemon([])
  board([goal])

  const state = await screen.findByText("No tasks yet — the orchestrator is still working")
  const placeholder = state.closest("[data-goal-placeholder]") as HTMLElement
  expect(placeholder.className).toContain("text-muted-foreground")
  expect(placeholder.className).not.toMatch(BOX)
  const line = screen.getByRole("link", { name: GOAL_TITLE }).closest("[data-goal-line]")
  expect(line?.nextElementSibling).toBe(placeholder)
  expect(screen.getByRole("link", { name: GOAL_TITLE }).getAttribute("href")).toContain(
    `goal=${goal.id}`,
  )
  expect(within(line as HTMLElement).getByText("Planning")).toBeTruthy()
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

it("keeps the task warning and its Retry instead of the empty state when the tasks fail", async () => {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname === "/v1/tasks")
      return Promise.resolve(errorResponse(503, "unavailable", "down"))
    return Promise.resolve(jsonResponse([]))
  })
  renderScreen(<GoalBoard hosts={["local"]} goals={[]} empty={<p>Nothing here</p>} />)

  expect(await screen.findByRole("button", { name: /retry/i })).toBeTruthy()
  expect(screen.queryByText("Nothing here")).toBeNull()
})

it("hands the keyboard to the fold button that replaces the one that was activated", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  stubDaemon([aTask({ id: "t1", goal_id: goal.id, status: "in_progress" })])
  board([goal])

  const collapse = await screen.findByRole("button", { name: `Collapse ${GOAL_TITLE}` })
  collapse.focus()
  fireEvent.click(collapse)
  const expand = await screen.findByRole("button", { name: `Expand ${GOAL_TITLE}` })
  expect(document.activeElement).toBe(expand)
})

it("hands the keyboard to the fold button when unfolding leaves a goal with no shown task", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "cancelled" })
  stubDaemon([aTask({ id: "t1", goal_id: goal.id, status: "cancelled" })])
  board([goal], { activeOnly: true })

  const expand = await screen.findByRole("button", { name: `Expand ${GOAL_TITLE}` })
  expand.focus()
  fireEvent.click(expand)
  const collapse = await screen.findByRole("button", { name: `Collapse ${GOAL_TITLE}` })
  expect(screen.getByText("No active tasks").closest("[data-goal-placeholder]")).toBeTruthy()
  expect(document.activeElement).toBe(collapse)
})

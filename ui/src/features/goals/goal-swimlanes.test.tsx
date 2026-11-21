// @vitest-environment jsdom
import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { GoalSwimlanes } from "./goal-swimlanes"

const GOAL_TITLE = "Ship the board"

it("shows all tasks in one area and counts them", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "one", goal_id: goal.id, title: "First", status: "pending" }),
      aTask({ id: "two", goal_id: goal.id, title: "Second", status: "finished" }),
    ]),
  )
  renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )
  expect(await screen.findByRole("heading", { name: "Tasks (2)" })).toBeTruthy()
  const area = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(within(area).getByText("First")).toBeTruthy()
  expect(within(area).getByText("Second")).toBeTruthy()
})

it("hides terminal tasks under Active while preserving progress", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "one", goal_id: goal.id, title: "Waiting", status: "pending" }),
      aTask({ id: "two", goal_id: goal.id, title: "Complete", status: "finished" }),
      aTask({ id: "three", goal_id: goal.id, title: "Cancelled task", status: "cancelled" }),
    ]),
  )
  renderScreen(
    <GoalSwimlanes
      hosts={["local"]}
      goals={[goal].map((goal) => ({ ...goal, host: "local" }))}
      activeOnly
    />,
  )
  expect(await screen.findByRole("heading", { name: "Tasks (1)" })).toBeTruthy()
  expect(screen.getByText("Waiting")).toBeTruthy()
  expect(screen.queryByText("Complete")).toBeNull()
  expect(screen.queryByText("Cancelled task")).toBeNull()
  expect(screen.getByText("1/2")).toBeTruthy()
})

it("draws a goal as one line: toggle, title link, status, progress and tokens", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "t1", goal_id: goal.id, status: "finished" }),
      aTask({ id: "t2", goal_id: goal.id, status: "in_progress" }),
    ]),
  )
  renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )

  const link = await screen.findByRole("link", { name: GOAL_TITLE })
  expect(link.getAttribute("href")).toContain(`goal=${goal.id}`)
  const line = link.closest("header") as HTMLElement
  expect(within(line).getByRole("button", { name: `Collapse ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(line).getByText("Active")).toBeTruthy()
  expect(await within(line).findByText("1/2")).toBeTruthy()
  expect(within(line).getByText(/out/)).toBeTruthy()
  expect(screen.queryByRole("progressbar")).toBeNull()
})

it("shows the lane summary in place of the cells once folded", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "completed" })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, status: "finished" })]))
  renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )

  expect(await screen.findByText("1 task finished")).toBeTruthy()
  expect(screen.queryByRole("group", { name: `${GOAL_TITLE}: Tasks` })).toBeNull()
})

it("aligns compact goals and tasks on fixed shared columns in one focusable scrollport", async () => {
  const goals = [aGoal({ id: "g1", title: "Short" }), aGoal({ id: "g2", title: "Long goal name" })]
  daemonFetch.mockResolvedValue(
    jsonResponse(
      goals.map((goal, index) =>
        aTask({
          id: `t${index}`,
          goal_id: goal.id,
          title: index ? "A much longer task title" : "Tiny",
        }),
      ),
    ),
  )
  renderScreen(
    <GoalSwimlanes
      compact
      hosts={["local"]}
      goals={goals.map((goal) => ({ ...goal, host: "local" }))}
    />,
  )
  const tables = await screen.findAllByRole("table")
  expect(tables).toHaveLength(2)
  for (const table of tables) {
    expect(table.className).toContain("table-fixed")
    expect(within(table).getByRole("columnheader", { name: "Goal" })).toBeTruthy()
    expect(within(table).getByRole("columnheader", { name: "Task" })).toBeTruthy()
  }
  expect(screen.getByRole("cell", { name: "Short" })).toBeTruthy()
  expect(screen.getByRole("cell", { name: "Long goal name" })).toBeTruthy()
  expect(screen.getByRole("region", { name: "Compact goals" }).tabIndex).toBe(0)
})

it("omits compact tables while tasks load", () => {
  daemonFetch.mockImplementation(() => new Promise(() => {}))
  renderScreen(<GoalSwimlanes compact hosts={["local"]} goals={[{ ...aGoal(), host: "local" }]} />)
  expect(screen.getByText("Tasks are not loaded")).toBeTruthy()
  expect(screen.queryByRole("table")).toBeNull()
})

it("keeps unavailable task hosts distinct from empty compact lanes", async () => {
  daemonFetch.mockResolvedValue(errorResponse(503, "unavailable", "Host unavailable"))
  renderScreen(<GoalSwimlanes compact hosts={["local"]} goals={[{ ...aGoal(), host: "local" }]} />)
  expect(await screen.findByText(/Host unavailable/)).toBeTruthy()
  expect(screen.getByText("Tasks are not loaded")).toBeTruthy()
  expect(screen.queryByRole("table")).toBeNull()
  expect(screen.queryByText("No tasks")).toBeNull()
})

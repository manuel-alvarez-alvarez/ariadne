// @vitest-environment jsdom
import { act, screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import { dispatchDomainEvent } from "@/events/dispatch"
import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
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

it("shows the execution host of a projected task in its source lane", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([aTask({ id: "remote", goal_id: goal.id, title: "Remote work", host: "worker" })]),
  )
  renderScreen(<GoalSwimlanes hosts={["local"]} goals={[{ ...goal, host: "local" }]} />)
  expect(await screen.findByText("Remote work")).toBeTruthy()
  expect(screen.getByText("worker")).toBeTruthy()
})

it("holds every goal-less task in one No goal lane, apart from the goal lanes", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "g1", goal_id: goal.id, title: "In a goal" }),
      aTask({ id: "n1", goal_id: null, title: "Loose one", steps: [] }),
      aTask({ id: "n2", goal_id: null, title: "Loose two", steps: [] }),
    ]),
  )
  renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )
  const lane = await screen.findByRole("group", { name: "No goal: Tasks" })
  expect(await within(lane).findByText("Loose one")).toBeTruthy()
  expect(within(lane).getByText("Loose two")).toBeTruthy()
  expect(within(lane).queryByText("In a goal")).toBeNull()
  expect(screen.getAllByRole("group", { name: "No goal: Tasks" })).toHaveLength(1)
  expect(
    within(screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })).queryByText("Loose one"),
  ).toBeNull()
})

it("keeps the No goal lane, empty, while every task has a goal", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id })]))
  renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )
  await screen.findByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  const lane = screen.getByRole("group", { name: "No goal: Tasks" })
  expect(within(lane).getByText("No tasks")).toBeTruthy()
  expect(screen.getAllByRole("group", { name: "No goal: Tasks" })).toHaveLength(1)
})

it("adds a goal-less task to the No goal lane when its task_created event arrives", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  const loose = aTask({ id: "n1", goal_id: null, title: "Fresh and loose", steps: [] })
  daemonFetch.mockResolvedValueOnce(jsonResponse([aTask({ goal_id: goal.id })]))
  const { queryClient } = renderScreen(
    <GoalSwimlanes hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )
  await screen.findByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(screen.queryByText("Fresh and loose")).toBeNull()

  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id }), loose]))
  act(() => dispatchDomainEvent(queryClient, "local", { event: "task_created", data: loose }))

  const lane = await screen.findByRole("group", { name: "No goal: Tasks" })
  expect(within(lane).getByText("Fresh and loose")).toBeTruthy()
})

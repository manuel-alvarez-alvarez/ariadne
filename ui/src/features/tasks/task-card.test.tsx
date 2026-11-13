// @vitest-environment jsdom

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { TaskDto } from "@/api"
import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskCard } from "./task-card"

const STEPS = [
  { id: "build", title: "Build", description: "Build the change.", skills: ["coding"], ranks: [] },
  { id: "test", title: "Test", description: "Test the change.", skills: ["testing"], ranks: [] },
  { id: "ship", title: "Ship", description: "Ship it.", skills: [], ranks: [] },
]

const SESSION = "01JSESSION000000000000001"

function states() {
  return screen.getAllByRole("listitem").map((dot) => dot.getAttribute("data-step-state"))
}

function card(overrides: Partial<TaskDto>) {
  renderScreen(<TaskCard task={aTask(overrides)} steps={STEPS} />)
}

it.each([
  [{ status: "pending", depends_on: ["a", "b"] }, "Waits for 2 tasks"],
  [{ status: "ready", depends_on: ["a"] }, "Waits for 1 task"],
  [{ status: "pending", depends_on: [] }, "Waits"],
  [{ status: "failed", reason: "boom" }, "Failed"],
  [{ status: "cancelled" }, "Cancelled"],
  [{ status: "in_progress", stalled: true }, "Stalled"],
] as [Partial<TaskDto>, string][])("shows the state badge %#", (overrides, label) => {
  card(overrides)
  expect(screen.getByText(label)).toBeTruthy()
})

it("shows no state badge for a task under way", () => {
  card({ status: "in_progress", step: "test" })
  for (const label of ["Waits", "Failed", "Stalled", "Cancelled", "In progress"]) {
    expect(screen.queryByText(label)).toBeNull()
  }
  expect(screen.getByText("Test", { selector: "span.truncate" })).toBeTruthy()
})

it("puts a failed task's reason in its badge tooltip", async () => {
  card({ status: "failed", reason: "the build broke" })
  await userEvent.hover(screen.getByText("Failed"))
  expect((await screen.findAllByText("the build broke")).length).toBeGreaterThan(0)
})

it("draws a check before the current step, a mark on it and rings after", () => {
  card({ status: "in_progress", step: "test" })
  expect(states()).toEqual(["done", "current", "ahead"])
  const [first, second] = screen.getAllByRole("listitem")
  expect(first?.querySelector("svg")).toBeTruthy()
  expect(second?.getAttribute("aria-current")).toBe("step")
  expect(second?.querySelector("svg")).toBeNull()
})

it("draws every dot as a check for a finished task and as a ring for one with no step", () => {
  card({ status: "finished", step: null })
  expect(states()).toEqual(["done", "done", "done"])
})

it("draws every dot as a ring for a task with no step", () => {
  card({ status: "pending", step: null })
  expect(states()).toEqual(["ahead", "ahead", "ahead"])
})

it("tells the step, its agent and its session in a dot's tooltip", async () => {
  card({
    status: "in_progress",
    step: "build",
    agents: [
      {
        id: "a1",
        step: "build",
        skills: ["coding", "testing"],
        model: "codex-acp:o3",
        effort: "high",
        session_id: SESSION,
      },
    ],
  })
  await userEvent.hover(
    within(screen.getAllByRole("listitem")[0] as HTMLElement).getByLabelText("Build"),
  )
  expect(await screen.findByText("Build the change.")).toBeTruthy()
  expect(screen.getByText("codex-acp:o3 @ high")).toBeTruthy()
  expect(screen.getByText("coding, testing")).toBeTruthy()
  expect(screen.getByRole("link", { name: /^session …/ })).toBeTruthy()
})

it("says none staffed for a step with no agent", async () => {
  card({ status: "in_progress", step: "build", agents: [] })
  await userEvent.hover(
    within(screen.getAllByRole("listitem")[2] as HTMLElement).getByLabelText("Ship"),
  )
  expect(await screen.findByText("none staffed")).toBeTruthy()
})

it("links the pull request and the updated time, and takes one tab stop", () => {
  card({ pr_url: "https://github.com/o/r/pull/7" })
  const pr = screen.getByRole("link", { name: "Pull request" })
  expect(pr.getAttribute("href")).toBe("https://github.com/o/r/pull/7")
  expect(pr.getAttribute("tabindex")).toBe("-1")
  expect(document.querySelector("time")).toBeTruthy()
  // Line 2's right end, beside the time: laid over the card's corner rather
  // than a row of its own, with line 2 leaving room for it.
  expect(pr.className).toContain("absolute")
  expect(pr.className).toContain("bottom-1.5")
  expect(document.querySelector("time")?.parentElement?.className).toContain("pr-5")
  expect(pr.parentElement?.children).toHaveLength(3) // card link, hints, PR: no third row
  const links = screen.getAllByRole("link")
  expect(links.filter((link) => link.getAttribute("tabindex") !== "-1")).toHaveLength(1)
})

it("shows no pull request link without a pr_url, and no branch", () => {
  card({ pr_url: null })
  expect(screen.queryByRole("link", { name: "Pull request" })).toBeNull()
  expect(screen.queryByText(/wire-the-sessions/)).toBeNull()
})

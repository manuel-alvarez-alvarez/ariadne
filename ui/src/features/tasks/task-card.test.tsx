// @vitest-environment jsdom
import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import type { TaskDto } from "@/api"
import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskCard } from "./task-card"

it("offers the task usage breakdown on keyboard focus", async () => {
  renderScreen(<TaskCard task={aTask()} steps={[]} />)
  const user = userEvent.setup()
  await user.tab()
  await user.tab()
  expect((await screen.findByRole("tooltip")).textContent).toContain("Input")
})

it.each(["finished", "cancelled"] as const)(
  "keeps %s cards compact with accessible usage details",
  (status) => {
    renderScreen(
      <TaskCard
        task={aTask({ status, step: "build", reason: "Stopped by request" })}
        steps={[{ id: "build", title: "Build", description: "", skills: [], ranks: [] }]}
      />,
    )
    expect(screen.queryByRole("list", { name: "Workflow steps" })).toBeNull()
    expect(screen.queryByText(/Current model/)).toBeNull()
    expect(screen.getByText(status === "finished" ? "Finished" : "Cancelled")).not.toBeNull()
    expect(screen.getByText("Input 0")).not.toBeNull()
    expect(screen.getByText("Output 0")).not.toBeNull()
    expect(screen.getByText("Usage details").closest("details")?.open).toBe(false)
  },
)

const steps = ["Plan", "Build", "Review"].map((title) => ({
  id: title.toLowerCase(),
  title,
  description: "",
  skills: [],
  ranks: [],
}))

it("labels completed, current and upcoming work and names the current staffing", () => {
  renderScreen(
    <TaskCard
      task={aTask({
        status: "in_progress",
        step: "build",
        agents: [
          { id: "builder", step: "build", model: "codex:model", effort: "high", skills: [] },
        ],
      })}
      steps={steps}
    />,
  )
  const items = screen.getAllByRole("listitem")
  expect(items.map((item) => item.getAttribute("data-step-state"))).toEqual([
    "done",
    "current",
    "ahead",
  ])
  expect(items[1]?.getAttribute("aria-current")).toBe("step")
  expect(screen.getByText("Current model: codex:model · high")).not.toBeNull()
})

it("names missing staffing without inventing a model and omits it without a step", () => {
  const view = renderScreen(<TaskCard task={aTask({ step: "build", agents: [] })} steps={steps} />)
  expect(screen.getByText("Current model: None staffed")).not.toBeNull()
  view.rerender(<TaskCard task={aTask({ step: null })} steps={steps} />)
  expect(screen.queryByText(/Current model/)).toBeNull()
})

it("shows dependency counts and failure previews", () => {
  const view = renderScreen(
    <TaskCard task={aTask({ status: "pending", depends_on: ["a", "b"] })} steps={steps} />,
  )
  expect(screen.getByText("Waits for 2 tasks")).not.toBeNull()
  view.rerender(
    <TaskCard task={aTask({ status: "failed", reason: "Build failed" })} steps={steps} />,
  )
  expect(screen.getByText("Build failed")).not.toBeNull()
})

it("keeps cancellation reasons, usage and Updated metadata in compact cards", async () => {
  const usage = { input_tokens: 1000, cached_input_tokens: 500, output_tokens: 200 }
  renderScreen(
    <TaskCard
      task={aTask({
        status: "cancelled",
        reason: "No longer needed",
        pr_url: "https://example.com/pr",
        usage: {
          total: usage,
          agents: [{ agent_id: "builder", skills: [], step: "build", usage }],
        },
      })}
      steps={steps}
    />,
  )
  expect(screen.getByText("No longer needed")).not.toBeNull()
  expect(screen.getByRole("link", { name: "Pull request" }).getAttribute("href")).toBe(
    "https://example.com/pr",
  )
  const user = userEvent.setup()
  await user.click(screen.getByText("Usage details"))
  const details = screen.getByText("Usage details").closest("details")
  expect(details?.open).toBe(true)
  expect(details?.textContent).toContain("Build: Input 1k · Output 200 · 50.0% cached")
  expect(details?.textContent).toContain("Updated")
})

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

it("shows no pull request link without a pr_url, and no branch", () => {
  card({ pr_url: null })
  expect(screen.queryByRole("link", { name: "Pull request" })).toBeNull()
  expect(screen.queryByText(/wire-the-sessions/)).toBeNull()
})

it("shows task totals and per-step usage on hover without adding cached tokens to input", async () => {
  const usage = { input_tokens: 1000, cached_input_tokens: 500, output_tokens: 200 }
  renderScreen(
    <TaskCard
      task={aTask({
        usage: {
          total: usage,
          agents: [{ agent_id: "builder", skills: [], step: "build", usage }],
        },
      })}
      steps={steps}
    />,
  )
  expect(screen.getByText("Input 1k · 50.0% cached")).not.toBeNull()
  await userEvent.hover(screen.getByRole("button", { name: "Task token usage" }))
  expect((await screen.findByRole("tooltip")).textContent).toContain(
    "Build: Input 1k · Output 200 · 50.0% cached",
  )
})

it("keeps session attention visible and updates the current model when task data changes", () => {
  const view = renderScreen(
    <TaskCard task={aTask({ step: "build" })} steps={steps} attention="waiting_input" />,
  )
  expect(screen.getByText("Waiting for input")).not.toBeNull()
  expect(screen.getByText("Current model: claude-agent-acp:claude-sonnet-5")).not.toBeNull()
  view.rerender(<TaskCard task={aTask({ step: null })} steps={steps} />)
  expect(screen.queryByText(/Current model/)).toBeNull()
})

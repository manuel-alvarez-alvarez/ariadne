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
  expect(screen.getByRole("link").textContent).not.toContain("claude-agent-acp:claude-sonnet-5")
  expect(
    document.getElementById(screen.getByRole("link").getAttribute("aria-describedby") ?? "")
      ?.textContent,
  ).toContain("Current step Test")
})

it("shows the current staffed model and compact task token totals", () => {
  card({
    status: "in_progress",
    step: "build",
    agents: [
      {
        id: "a1",
        step: "build",
        skills: ["coding"],
        model: "codex-acp:a-very-long-model-name",
        effort: "high",
      },
    ],
    usage: {
      total: { input_tokens: 1234, cached_input_tokens: 300, output_tokens: 56 },
      agents: [],
    },
  })
  expect(screen.queryByText("codex-acp:a-very-long-model-name")).toBeNull()
  expect(
    document.getElementById(screen.getByRole("link").getAttribute("aria-describedby") ?? "")
      ?.textContent,
  ).toContain("Current model codex-acp:a-very-long-model-name")
  expect(screen.getByText("Build", { selector: "span.truncate" })).toBeTruthy()
  expect(screen.getByText("1.2k")).toBeTruthy()
  expect(screen.getByText("56")).toBeTruthy()
})

it.each(["finished", "cancelled"] as const)(
  "shows token totals and the workflow without a model for %s tasks",
  (status) => {
    card({
      status,
      usage: { total: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 }, agents: [] },
    })
    expect(screen.getAllByText("0").length).toBeGreaterThan(0)
    expect(screen.queryByText("claude-agent-acp:claude-sonnet-5")).toBeNull()
    expect(screen.getByRole("list", { name: "Workflow steps" })).toBeTruthy()
  },
)

it("puts a failed task's reason in its badge tooltip", async () => {
  card({ status: "failed", reason: "the build broke" })
  await userEvent.hover(screen.getByText("Failed"))
  expect((await screen.findAllByText("the build broke")).length).toBeGreaterThan(0)
})

it("draws a check before the current step, a mark on it and rings after", () => {
  card({ status: "in_progress", step: "test" })
  expect(states()).toEqual(["done", "current", "ahead"])
  const [first, second] = screen.getAllByRole("listitem")
  expect(first?.querySelector("svg.lucide-check")).toBeTruthy()
  expect(second?.getAttribute("aria-current")).toBe("step")
  expect(second?.querySelector("svg.lucide-check")).toBeNull()
})

it("keeps the workflow track for a finished task, every step done", () => {
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

it("links the pull request between the token figure and the time, and takes one tab stop", () => {
  card({ pr_url: "https://github.com/o/r/pull/7" })
  const pr = screen.getByRole("link", { name: /^Pull request #7\b/ })
  expect(pr.getAttribute("href")).toBe("https://github.com/o/r/pull/7")
  expect(pr.getAttribute("tabindex")).toBe("-1")
  expect(document.querySelector("time")).toBeTruthy()
  // The link's row holds an invisible slot between the figure and the time;
  // the figure is the flexible part, the slot and the time never shrink.
  const row = document.querySelector("[data-pr-slot]")?.parentElement as HTMLElement
  expect(row.children).toHaveLength(3)
  expect(row.children[0]?.className).toContain("flex-1")
  expect(row.children[1]?.className).toContain("shrink-0")
  expect(row.children[2]?.className).toContain("shrink-0")
  expect(pr.parentElement?.className).toContain("bottom-1.5")
  const links = screen.getAllByRole("link")
  expect(links.filter((link) => link.getAttribute("tabindex") !== "-1")).toHaveLength(1)
})

it("shows no pull request link without a pr_url, and no branch", () => {
  card({ pr_url: null })
  expect(screen.queryByRole("link", { name: "Pull request" })).toBeNull()
  expect(screen.queryByText(/wire-the-sessions/)).toBeNull()
})

it("keeps a long workflow on one line and folds its far steps", () => {
  const many = Array.from({ length: 12 }, (_, i) => ({
    id: `s${i}`,
    title: `S${i}`,
    description: "",
    skills: [],
    ranks: [],
  }))
  renderScreen(<TaskCard task={aTask({ step: "s11" })} steps={many} />)
  const track = screen.getByRole("list", { name: "Workflow steps" })
  expect(within(track).getAllByRole("listitem")).toHaveLength(12)
  expect(track.className).toContain("flex-nowrap")
  expect(track.className).toContain("overflow-hidden")
  expect(within(track).getAllByRole("listitem")[0]?.className).toContain("hidden")
})

it("shows the latest of the task's update and its sessions' activity", () => {
  renderScreen(
    <TaskCard
      task={aTask({ updated_at: "2026-01-01T00:00:00Z" })}
      steps={STEPS}
      lastActivityAt="2026-02-01T00:00:00Z"
    />,
  )
  expect(document.querySelector("time")?.getAttribute("datetime")).toBe("2026-02-01T00:00:00Z")
})

it("falls back to the task's update without session activity", () => {
  renderScreen(<TaskCard task={aTask({ updated_at: "2026-01-01T00:00:00Z" })} steps={STEPS} />)
  expect(document.querySelector("time")?.getAttribute("datetime")).toBe("2026-01-01T00:00:00Z")
})

it("lists the folded steps in the fold marker's tooltip", async () => {
  const many = Array.from({ length: 8 }, (_, i) => ({
    id: `s${i}`,
    title: `S${i}`,
    description: `Does ${i}.`,
    skills: [],
    ranks: [],
  }))
  renderScreen(<TaskCard task={aTask({ status: "in_progress", step: "s7" })} steps={many} />)
  await userEvent.hover(screen.getByLabelText("6 more steps"))
  expect(await screen.findByText("Does 0.")).toBeTruthy()
})

it("hovers the token figure into Input, Output and a row per step, with no tab stop inside the link", async () => {
  card({
    status: "in_progress",
    step: "build",
    agents: [],
    usage: {
      total: { input_tokens: 1234, cached_input_tokens: 300, output_tokens: 56 },
      agents: [
        {
          agent_id: "a1",
          skills: [],
          step: "build",
          usage: { input_tokens: 1234, cached_input_tokens: 300, output_tokens: 56 },
        },
      ],
    },
  })
  const link = screen.getByRole("link")
  expect(link.querySelectorAll("[tabindex]:not([tabindex='-1'])").length).toBe(0)
  await userEvent.hover(screen.getByText("1.2k"))
  expect((await screen.findAllByText("Input")).length).toBeGreaterThan(0)
  expect(screen.getAllByText("Output").length).toBeGreaterThan(0)
  expect(screen.getAllByText("Build").length).toBeGreaterThan(0)
})

it.each([
  ["finished", "border-status-done/40"],
  ["cancelled", "border-muted-foreground/40"],
  ["failed", "border-status-danger/40"],
] as const)("outlines a %s task with its status color", (status, border) => {
  card({ status })
  expect(screen.getByRole("link").parentElement?.classList.contains(border)).toBe(true)
})

it("anchors task usage and activity at the bottom of the card", () => {
  card({ title: "A task with a long title" })
  const link = screen.getByRole("link")
  expect(link.classList.contains("h-full")).toBe(true)
  expect(link.querySelector(".mt-auto")?.textContent).toContain("0")
})

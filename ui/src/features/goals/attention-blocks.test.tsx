// @vitest-environment jsdom

/**
 * The attention blocks of the Goals board: each item of `GET /v1/attention`
 * drawn below the task row or the goal line it names (spec 015 rule 39a).
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { AttentionItemDto, GoalDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { GoalBoard } from "./goal-board"

afterEach(() => forgetHosts())
beforeEach(() => localStorage.clear())

const SINCE = "2026-11-25T10:00:00Z"

function item(
  id: string,
  affected: AttentionItemDto["affected"],
  over: Partial<AttentionItemDto> = {},
): AttentionItemDto {
  return {
    id,
    occurrence: `o-${id}`,
    producer: "recovery",
    reason: "quota",
    summary: `Summary of ${id}`,
    required_action: `Action of ${id}`,
    since: SINCE,
    affected,
    target: { kind: "task", task_id: affected[0]?.id ?? "" },
    ...over,
  }
}

function open(
  goals: GoalDto[],
  tasks: TaskDto[],
  items: AttentionItemDto[],
  extra: {
    sessions?: SessionDto[]
    complete?: boolean
    answers?: Record<string, unknown>
  } = {},
) {
  const seen = stubHosts({
    local: {
      "/v1/tasks": tasks,
      "/v1/goals": goals,
      "/v1/sessions": aSessionPage(extra.sessions ?? []),
      "/v1/attention": { items, complete: extra.complete ?? true },
      ...extra.answers,
    },
  })
  renderScreen(
    <GoalBoard hosts={["local"]} goals={goals.map((goal) => ({ ...goal, host: "local" }))} />,
  )
  return seen
}

/** What follows `node` among its siblings' blocks: the block sits right after the row. */
function blockAfter(node: Element): HTMLElement | null {
  const next = node.nextElementSibling
  return next instanceof HTMLElement && next.matches("[data-attention-blocks]") ? next : null
}

const goal = aGoal({ id: "G1", title: "Ship the board", status: "active" })
const one = aTask({ id: "T1", goal_id: "G1", title: "First task" })
const two = aTask({ id: "T2", goal_id: "G1", title: "Second task" })

function taskRow(title: string): HTMLElement {
  const row = screen.getByText(title).closest("[data-task-row]")
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${title}`)
  return row
}

it("shows a block below the row of the task an item names, and none below the others", async () => {
  open([goal], [one, two], [item("a", [{ kind: "task", id: "T1", label: "First task" }])])

  const block = await waitFor(() => {
    const found = blockAfter(taskRow("First task"))
    if (!found) throw new Error("no block yet")
    return found
  })
  expect(within(block).getByText("Summary of a")).toBeTruthy()
  expect(blockAfter(taskRow("Second task"))).toBeNull()
})

it("shows a block below a task whose session an item names", async () => {
  const session = aSession({ id: "S1", goal_id: "G1", task_id: "T2" })
  open([goal], [one, two], [item("s", [{ kind: "session", id: "S1", label: "agent" }])], {
    sessions: [session],
  })

  await waitFor(() => expect(blockAfter(taskRow("Second task"))).not.toBeNull())
  expect(blockAfter(taskRow("First task"))).toBeNull()
})

it("shows one block below each task an item names, and one if it names a task and its session", async () => {
  const session = aSession({ id: "S1", goal_id: "G1", task_id: "T1" })
  open(
    [goal],
    [one, two],
    [
      item("many", [
        { kind: "task", id: "T1", label: "First task" },
        { kind: "session", id: "S1", label: "agent" },
        { kind: "task", id: "T2", label: "Second task" },
      ]),
    ],
    { sessions: [session] },
  )

  await waitFor(() => expect(blockAfter(taskRow("Second task"))).not.toBeNull())
  for (const title of ["First task", "Second task"]) {
    const block = blockAfter(taskRow(title)) as HTMLElement
    expect(within(block).getAllByText("Summary of many")).toHaveLength(1)
  }
})

it("shows a block for the goal, or for a session of the goal with no task, directly below the goal line", async () => {
  const orchestrator = aSession({ id: "S9", goal_id: "G1", task_id: null, seat: "orchestrator" })
  open(
    [goal],
    [one],
    [
      item("g", [{ kind: "goal", id: "G1", label: "Ship the board" }]),
      item("o", [{ kind: "session", id: "S9", label: "orchestrator" }]),
    ],
    { sessions: [orchestrator] },
  )

  const line = await waitFor(() => {
    const found = document.querySelector("[data-goal-line]")
    if (!found || !blockAfter(found)) throw new Error("no block yet")
    return found
  })
  const block = blockAfter(line) as HTMLElement
  expect(within(block).getByText("Summary of g")).toBeTruthy()
  expect(within(block).getByText("Summary of o")).toBeTruthy()
  // The goal's own blocks come before its first task row.
  expect(block.nextElementSibling).toBe(taskRow("First task"))
})

it("shows a task with no goal its blocks below its row", async () => {
  const loose = aTask({ id: "T7", goal_id: null, title: "Loose task" })
  open([], [loose], [item("l", [{ kind: "task", id: "T7", label: "Loose task" }])])

  await waitFor(() => expect(blockAfter(taskRow("Loose task"))).not.toBeNull())
})

it("shows no block under a folded goal, and its goal line keeps the attention badge", async () => {
  const orchestrator = aSession({
    id: "S9",
    goal_id: "G1",
    task_id: null,
    status: "running",
    attention_reason: "waiting_permission",
  })
  localStorage.setItem(
    "ariadne.goals.collapsed-lanes",
    JSON.stringify({ collapsed: ["G1"], expanded: [] }),
  )
  open(
    [goal],
    [one],
    [
      item("g", [{ kind: "goal", id: "G1", label: "Ship the board" }]),
      item("t", [{ kind: "task", id: "T1", label: "First task" }]),
      item("o", [{ kind: "session", id: "S9", label: "orchestrator" }]),
    ],
    { sessions: [orchestrator] },
  )

  await screen.findByText("Waiting for permission")
  expect(document.querySelector("[data-attention-blocks]")).toBeNull()
})

it("draws the content of a drawer row: summary, action, age, links, target and Dismiss", async () => {
  const sessions = [
    aSession({ id: "S1", goal_id: "G1", task_id: "T1" }),
    aSession({ id: "S2", goal_id: "G1", task_id: "T1" }),
  ]
  open(
    [goal],
    [one],
    [
      item(
        "q",
        [
          { kind: "session", id: "S1", label: "first agent" },
          { kind: "session", id: "S2", label: "second agent" },
        ],
        { producer: "agent_request", target: { kind: "console", session_id: "S1" } },
      ),
    ],
    { sessions },
  )

  const block = await waitFor(() => {
    const found = blockAfter(taskRow("First task"))
    if (!found) throw new Error("no block yet")
    return found
  })
  expect(within(block).getByText("Summary of q")).toBeTruthy()
  expect(within(block).getByText("Action of q")).toBeTruthy()
  expect(within(block).getByText(/Affects: first agent, second agent/)).toBeTruthy()
  expect(block.querySelector(`time[datetime="${SINCE}"]`)).not.toBeNull()
  expect(within(block).getByRole("link", { name: "first agent" })).toBeTruthy()
  expect(within(block).getByRole("link", { name: "second agent" })).toBeTruthy()
  expect(within(block).getByRole("button", { name: "Open console" })).toBeTruthy()
  expect(within(block).getByRole("button", { name: "Dismiss" }).textContent).toBe("")
  expect(within(block).getByRole("button", { name: "Open console" }).textContent).toBe("")
})

it("keeps every link and button of a block in the tab order", async () => {
  open([goal], [one], [item("k", [{ kind: "task", id: "T1", label: "First task" }])])

  const block = await waitFor(() => {
    const found = blockAfter(taskRow("First task"))
    if (!found) throw new Error("no block yet")
    return found
  })
  const stops = block.querySelectorAll("a, button")
  expect(stops.length).toBeGreaterThanOrEqual(2)
  for (const stop of stops) expect(stop.getAttribute("tabindex")).not.toBe("-1")
})

it("shows blocks from the shared list while the read is incomplete", async () => {
  open([goal], [one], [item("i", [{ kind: "task", id: "T1", label: "First task" }])], {
    complete: false,
  })

  await waitFor(() => expect(blockAfter(taskRow("First task"))).not.toBeNull())
})

const QUESTION = (schema: unknown) => ({
  "/v1/sessions/S1/questions": [
    {
      id: "Q1",
      session_id: "S1",
      message: "Which database should I use?",
      schema,
      created_at: SINCE,
    },
  ],
  "/v1/sessions/S1/questions/Q1/answer": () => new Response(null, { status: 204 }),
})
const ASK = item("question:Q1", [{ kind: "session", id: "S1", label: "agent" }], {
  producer: "agent_request",
  reason: "unknown",
  target: { kind: "console", session_id: "S1" },
})
const choice = (value: string, title: string) => ({ const: value, title })

it("answers a form of one single-select question from its block with one button per choice", async () => {
  const seen = open([goal], [one], [ASK], {
    sessions: [aSession({ id: "S1", goal_id: "G1", task_id: "T1" })],
    answers: QUESTION({
      type: "object",
      properties: {
        db: {
          type: "string",
          title: "Database",
          oneOf: [choice("pg", "Postgres"), choice("my", "MySQL")],
        },
      },
    }),
  })

  const form = await screen.findByRole("region", { name: "Agent question" })
  expect(within(form).getByText("Which database should I use?")).toBeTruthy()
  expect(
    within(form)
      .getAllByRole("button")
      .map((button) => button.textContent),
  ).toEqual(["Postgres", "MySQL"])
  fireEvent.click(within(form).getByRole("button", { name: "Postgres" }))
  await waitFor(() => expect(seen.some((request) => request.method === "POST")).toBe(true))
  const answer = seen.find((request) => request.method === "POST")
  expect(answer?.url.pathname).toBe("/v1/sessions/S1/questions/Q1/answer")
  expect(JSON.parse(answer?.body ?? "{}")).toEqual({ action: "accept", content: { db: "pg" } })
})

it("links a form of another shape to the console and draws no buttons", async () => {
  open([goal], [one], [ASK], {
    sessions: [aSession({ id: "S1", goal_id: "G1", task_id: "T1" })],
    answers: QUESTION({
      type: "object",
      properties: {
        db: { type: "string", title: "Database", oneOf: [choice("pg", "Postgres")] },
        note: { type: "string", title: "Note" },
      },
    }),
  })

  const block = await waitFor(() => {
    const found = blockAfter(taskRow("First task"))
    if (!found) throw new Error("no block yet")
    return found
  })
  expect(
    within(block)
      .getByRole("link", { name: /Summary of question:Q1/ })
      .getAttribute("href"),
  ).toContain("session=S1")
  expect(screen.queryByRole("region", { name: "Agent question" })).toBeNull()
})

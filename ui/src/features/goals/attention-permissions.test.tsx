// @vitest-environment jsdom

/**
 * A pending permission on the Needs attention list: the block shows the oldest
 * one its session waits for and answers it in place (spec 015 rule 59, spec
 * 031 rule 11).
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { AttentionItemDto } from "@/api"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { aPermissionStep, aSnapshot, aTurn } from "@/test/trajectory"
import { AttentionPopover } from "./attention-strip"
import { GoalBoard } from "./goal-board"

afterEach(() => forgetHosts())
beforeEach(() => localStorage.clear())

const goal = aGoal({ id: "G1", title: "Ship the board", status: "active" })
const task = aTask({ id: "T1", goal_id: "G1", title: "First task" })
const session = aSession({
  id: "S1",
  goal_id: "G1",
  task_id: "T1",
  status: "running",
  attention_reason: "waiting_permission",
})

const ITEM: AttentionItemDto = {
  id: "agent_request:waiting_permission:S1",
  occurrence: "o-1",
  producer: "agent_request",
  reason: "unknown",
  summary: "Permission approval needed",
  required_action: "Answer in the console.",
  since: "2026-11-25T10:00:00Z",
  affected: [{ kind: "session", id: "S1", label: "author" }],
  target: { kind: "console", session_id: "S1" },
}

function turnWith(...steps: ReturnType<typeof aPermissionStep>[]) {
  return aTurn({ state: "running", attention: steps })
}

function stub(trajectory: unknown, input?: (url: URL, request: Request) => Response | undefined) {
  return stubHosts({
    local: {
      "/v1/tasks": [task],
      "/v1/goals": [goal],
      "/v1/sessions": aSessionPage([session]),
      "/v1/attention": { items: [ITEM], complete: true },
      "/v1/sessions/S1/trajectory": trajectory,
      "/v1/sessions/S1/console/input": input ?? (() => new Response(null, { status: 204 })),
    },
  })
}

function board() {
  return renderScreen(<GoalBoard hosts={["local"]} goals={[{ ...goal, host: "local" }]} />)
}

async function card() {
  return screen.findByRole("region", { name: "Pending permission" })
}

it("shows the oldest pending permission of the session with one button per option", async () => {
  const turn = aTurn({ state: "running" })
  const newer = aPermissionStep(turn.id, {
    tool: { title: "Edit", subject: "a.txt", kind: "edit" },
  })
  const older = aPermissionStep(turn.id, {
    question: "Run the build?",
    tool: { title: "Bash", subject: "make build", kind: "execute" },
  })
  // The turn lists its attention newest first.
  stub(aSnapshot([{ ...turn, attention: [newer, older] }]))
  board()

  const block = await card()
  expect(within(block).getByText(/Bash/)).toBeTruthy()
  expect(within(block).getByText("make build")).toBeTruthy()
  expect(within(block).getByText("Run the build?")).toBeTruthy()
  expect(within(block).queryByText("a.txt")).toBeNull()
  for (const name of ["Allow once", "Allow always", "Reject"])
    expect(within(block).getByRole("button", { name })).toBeTruthy()
})

it("posts the option id as text to the console input of the session", async () => {
  const turn = aTurn({ state: "running" })
  const seen = stub(aSnapshot([turnWith(aPermissionStep(turn.id))]))
  board()

  fireEvent.click(within(await card()).getByRole("button", { name: "Allow always" }))

  await waitFor(() =>
    expect(seen.find((request) => request.method === "POST")?.url.pathname).toBe(
      "/v1/sessions/S1/console/input",
    ),
  )
  expect(JSON.parse(seen.find((request) => request.method === "POST")?.body ?? "{}")).toEqual({
    text: "always",
  })
})

it("disables every option while the post runs", async () => {
  stub(aSnapshot([turnWith(aPermissionStep("T"))]), () => new Promise(() => {}) as never)
  board()

  const block = await card()
  fireEvent.click(within(block).getByRole("button", { name: "Allow once" }))

  await waitFor(() => {
    for (const button of within(block).getAllByRole("button"))
      expect((button as HTMLButtonElement).disabled).toBe(true)
  })
})

it("shows a failed post and permits a new attempt", async () => {
  let calls = 0
  stub(aSnapshot([turnWith(aPermissionStep("T"))]), () => {
    calls += 1
    return calls === 1
      ? new Response(JSON.stringify({ error: { code: "busy", message: "Nobody is listening" } }), {
          status: 409,
          headers: { "content-type": "application/json" },
        })
      : new Response(null, { status: 204 })
  })
  board()

  const block = await card()
  fireEvent.click(within(block).getByRole("button", { name: "Reject" }))

  expect((await within(block).findByRole("alert")).textContent).toMatch(/Nobody is listening/)
  const again = within(block).getByRole("button", { name: "Reject" }) as HTMLButtonElement
  expect(again.disabled).toBe(false)
  fireEvent.click(again)
  await waitFor(() => expect(calls).toBe(2))
})

it("names the subagent that asked", async () => {
  const step = aPermissionStep("T", {
    subagent: { id: "SA1", title: "Explore the tests" },
  })
  stub(aSnapshot([turnWith(step)]))
  board()

  expect((await card()).textContent).toMatch(/asked by subagent Explore the tests/)
})

it("shows no option button and keeps the console link when nothing is pending", async () => {
  stub(aSnapshot([aTurn({ state: "running" })]))
  const { queryClient } = board()

  await screen.findByText("Permission approval needed")
  // The trajectory response has arrived and holds no pending permission.
  await waitFor(() => {
    const reads = queryClient
      .getQueryCache()
      .findAll({ queryKey: ["local", "sessions", "detail", "S1", "pending-permission"] })
    expect(reads.length).toBeGreaterThan(0)
    expect(reads.every((read) => read.state.status === "success")).toBe(true)
  })
  expect(screen.queryByRole("region", { name: "Pending permission" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull()
  expect(screen.getByText("Open console")).toBeTruthy()
})

it("disables the buttons of every view of the permission while one view posts", async () => {
  stub(aSnapshot([turnWith(aPermissionStep("T"))]), () => new Promise(() => {}) as never)
  renderScreen(
    <>
      <GoalBoard hosts={["local"]} goals={[{ ...goal, host: "local" }]} />
      <AttentionPopover />
    </>,
    { route: "/goals" },
  )

  await card()
  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  await waitFor(() =>
    expect(screen.getAllByRole("region", { name: "Pending permission" })).toHaveLength(2),
  )
  const [first, second] = screen.getAllByRole("region", { name: "Pending permission" })
  fireEvent.click(within(first as HTMLElement).getByRole("button", { name: "Allow once" }))

  await waitFor(() => {
    for (const view of [first, second])
      for (const button of within(view as HTMLElement).getAllByRole("button"))
        expect((button as HTMLButtonElement).disabled).toBe(true)
  })
})

it("keeps an answered permission disabled when the attention list refresh fails", async () => {
  let answered = false
  stubHosts({
    local: {
      "/v1/tasks": [task],
      "/v1/goals": [goal],
      "/v1/sessions": aSessionPage([session]),
      "/v1/attention": () =>
        answered
          ? new Response(JSON.stringify({ error: { code: "down", message: "Unavailable" } }), {
              status: 503,
              headers: { "content-type": "application/json" },
            })
          : new Response(JSON.stringify({ items: [ITEM], complete: true }), {
              headers: { "content-type": "application/json" },
            }),
      "/v1/sessions/S1/trajectory": aSnapshot([turnWith(aPermissionStep("T"))]),
      "/v1/sessions/S1/console/input": () => {
        answered = true
        return new Response(null, { status: 204 })
      },
    },
  })
  const { queryClient } = board()

  const block = await card()
  fireEvent.click(within(block).getByRole("button", { name: "Allow once" }))

  await waitFor(() => {
    const lists = queryClient.getQueryCache().findAll({ queryKey: ["local", "attention", "list"] })
    expect(lists.some((list) => list.state.fetchStatus === "idle" && list.state.error)).toBe(true)
  })
  for (const button of within(block).getAllByRole("button"))
    expect((button as HTMLButtonElement).disabled).toBe(true)
})

it("shows the permission now pending after the attention list changes", async () => {
  const first = aPermissionStep("T", { tool: { title: "Bash", subject: "first", kind: "execute" } })
  const second = aPermissionStep("T", {
    tool: { title: "Bash", subject: "second", kind: "execute" },
  })
  let current = first
  stubHosts({
    local: {
      "/v1/tasks": [task],
      "/v1/goals": [goal],
      "/v1/sessions": aSessionPage([session]),
      "/v1/attention": { items: [ITEM], complete: true },
      "/v1/sessions/S1/trajectory": () =>
        new Response(JSON.stringify(aSnapshot([turnWith(current)])), {
          headers: { "content-type": "application/json" },
        }),
    },
  })
  const { queryClient } = board()

  await screen.findByText("first")
  current = second
  await queryClient.invalidateQueries({ queryKey: ["local", "attention"] })

  await screen.findByText("second")
})

it("draws the same buttons in the popover and in the drawer", async () => {
  stub(aSnapshot([turnWith(aPermissionStep("T"))]))
  renderScreen(<AttentionPopover />, { route: "/goals" })

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const inPopover = await card()
  expect(within(inPopover).getAllByRole("button")).toHaveLength(3)

  fireEvent.click(screen.getByRole("button", { name: "View all" }))
  const drawer = await screen.findByRole("dialog")
  const inDrawer = await within(drawer).findByRole("region", { name: "Pending permission" })
  expect(
    within(inDrawer)
      .getAllByRole("button")
      .map((button) => button.textContent),
  ).toEqual(["Allow once", "Allow always", "Reject"])
})

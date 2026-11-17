// @vitest-environment jsdom

/**
 * The task panel on a named host: it reads that host, its actions post there,
 * and the host's own event stream refreshes it.
 */

import { act, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { MessageDto } from "@/api"
import { DetailPanels } from "@/components/detail-panels"
import { EventStreamProvider } from "@/events/provider"
import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { aGoal, aSessionPage, aTask } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, type SeenRequest, stubHosts } from "@/test/hosts"

const BOX = "http://box.test:7676"
const GOAL = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })
const TASK = aTask({ id: "01JBOXTASK000000000000000T", goal_id: GOAL.id, status: "failed" })

function aMessage(body: string): MessageDto {
  return {
    id: `01JMESSAGE${body.length.toString().padStart(15, "0")}`,
    goal_id: GOAL.id,
    task_id: TASK.id,
    kind: "message",
    from_actor: "user",
    from_agent_id: null,
    from_session: null,
    to_actor: "agent",
    to_agent_id: TASK.agents[0]?.id ?? null,
    body,
    delivered_at: null,
    created_at: "2026-11-17T10:00:00Z",
  }
}

beforeEach(() => {
  stubEventSource()
  useSettingsStore.setState({ hostFilter: "" })
})
afterEach(() => forgetHosts())

function onBox(seen: SeenRequest[], method: string, path: string) {
  return seen.filter(
    (request) =>
      request.origin === BOX && request.method === method && request.url.pathname === path,
  )
}

it("reads the task of a named host and refreshes its messages on that host's stream", async () => {
  const user = userEvent.setup()
  const box = { messages: [aMessage("first")] }
  const seen = stubHosts({
    box: {
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      "/v1/sessions": aSessionPage([]),
      [`/v1/tasks/${TASK.id}/transitions`]: [],
      [`/v1/tasks/${TASK.id}/messages`]: (_url: URL, request: Request) => {
        if (request.method === "GET") return jsonResponse(box.messages)
        box.messages = [...box.messages, aMessage("posted to box")]
        return jsonResponse(box.messages.at(-1))
      },
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?task=${TASK.id}&host=box&tab=messages` },
  )
  expect(await screen.findByText("first")).toBeTruthy()

  // The panel's own composer posts to the box daemon's route, never to local's.
  await user.type(screen.getByRole("textbox", { name: "Message" }), "posted to box")
  await user.click(screen.getByRole("button", { name: "Send" }))
  await waitFor(() => expect(onBox(seen, "POST", `/v1/tasks/${TASK.id}/messages`)).toHaveLength(1))
  expect(JSON.parse(onBox(seen, "POST", `/v1/tasks/${TASK.id}/messages`)[0]?.body ?? "{}")).toEqual(
    { body: "posted to box", to_actor: "orchestrator" },
  )
  expect(seen.filter((request) => request.method === "POST" && request.origin !== BOX)).toEqual([])

  // The POST's own refresh shows the posted message. A later message the box
  // writes by itself is not read until the box's stream says so.
  expect(await screen.findByText("posted to box")).toBeTruthy()
  box.messages = [...box.messages, aMessage("said later by an agent")]
  await act(async () => {})
  expect(screen.queryByText("said later by an agent")).toBeNull()

  const stream = FakeEventSource.instances.find((one) => one.url.startsWith(BOX))
  if (!stream) throw new Error("no stream on box")
  act(() => {
    stream.succeed()
    stream.emit("message_sent", box.messages.at(-1))
  })
  expect(await screen.findByText("said later by an agent")).toBeTruthy()
})

it("names the host of the task panel under any pick while a named host is listed", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  stubHosts({
    box: {
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      "/v1/sessions": aSessionPage([]),
      [`/v1/tasks/${TASK.id}/transitions`]: [],
      [`/v1/tasks/${TASK.id}/messages`]: [],
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?task=${TASK.id}&host=box` },
  )

  await screen.findByRole("heading", { name: TASK.title })
  await waitFor(() =>
    expect(screen.getAllByTitle("Host").map((label) => label.textContent)).toEqual(["box"]),
  )
})

it("retries a failed task on the host the panel is open on", async () => {
  const user = userEvent.setup()
  const seen = stubHosts({
    box: {
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      "/v1/sessions": aSessionPage([]),
      [`/v1/tasks/${TASK.id}/transitions`]: [],
      [`/v1/tasks/${TASK.id}/messages`]: [],
      [`/v1/tasks/${TASK.id}/retry`]: { ...TASK, status: "ready" },
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?task=${TASK.id}&host=box` },
  )

  await user.click(await screen.findByRole("button", { name: /Retry/ }))

  await waitFor(() => expect(onBox(seen, "POST", `/v1/tasks/${TASK.id}/retry`)).toHaveLength(1))
  expect(seen.some((request) => request.method === "POST" && request.origin !== BOX)).toBe(false)
})

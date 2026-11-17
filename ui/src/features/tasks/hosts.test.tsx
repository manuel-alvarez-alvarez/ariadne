// @vitest-environment jsdom

/**
 * The task panel on a named host: it reads that host, its actions post there,
 * and the host's own event stream refreshes it.
 */

import { act, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"

import { api, type MessageDto, unwrap } from "@/api"
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

  // The message goes to the box daemon's route, never to local's.
  await unwrap(
    api("box").POST("/v1/tasks/{id}/messages", {
      params: { path: { id: TASK.id } },
      body: { body: "posted to box", to_agent_id: TASK.agents[0]?.id },
    } as never),
  )
  expect(onBox(seen, "POST", `/v1/tasks/${TASK.id}/messages`)).toHaveLength(1)
  expect(seen.filter((request) => request.method === "POST" && request.origin !== BOX)).toEqual([])

  const stream = FakeEventSource.instances.find((one) => one.url.startsWith(BOX))
  if (!stream) throw new Error("no stream on box")
  act(() => {
    stream.succeed()
    stream.emit("message_sent", box.messages.at(-1))
  })
  expect(await screen.findByText("posted to box")).toBeTruthy()
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

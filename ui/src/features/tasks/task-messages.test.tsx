// @vitest-environment jsdom

import { act, screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { type DomainEvent, type MessageDto, qk } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { TaskPanel } from "./task-panel"

const TASK = aTask({
  agents: [
    { id: "author", step: "develop", skills: ["coding", "testing"], model: "codex-acp:o3" },
    { id: "reviewer", step: "review", skills: ["code-review"], model: "codex-acp:o3" },
  ],
})

function aMessage(overrides: Partial<MessageDto> = {}): MessageDto {
  return {
    id: "01JMESSAGE0000000000000001",
    goal_id: TASK.goal_id,
    task_id: TASK.id,
    kind: "message",
    from_actor: "agent",
    from_agent_id: "author",
    from_session: null,
    to_actor: "agent",
    to_agent_id: "reviewer",
    body: "First message.",
    delivered_at: null,
    created_at: "2026-01-01T00:00:00Z",
    ...overrides,
  }
}

function stubMessages(messages: MessageDto[] | (() => MessageDto[])): void {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (
      request.method === "GET" &&
      new URL(request.url).pathname === `/v1/tasks/${TASK.id}/messages`
    ) {
      return Promise.resolve(jsonResponse(typeof messages === "function" ? messages() : messages))
    }
    return new Promise(() => {})
  })
}

function renderMessages({ stream = false }: { stream?: boolean } = {}): void {
  const goal = aGoal({ id: TASK.goal_id })
  renderScreen(
    stream ? (
      <EventStreamProvider>
        <TaskPanel taskId={TASK.id} onClose={() => {}} />
      </EventStreamProvider>
    ) : (
      <TaskPanel taskId={TASK.id} onClose={() => {}} />
    ),
    {
      route: "/goals?tab=messages",
      seed: (client) => {
        client.setQueryData(qk.tasks.detail(TASK.id), TASK)
        client.setQueryData(qk.goals.detail(goal.id), goal)
      },
    },
  )
}

beforeEach(() => stubEventSource())

it("lists task messages oldest first and names both agents by their skills", async () => {
  stubMessages([
    aMessage({ body: "First message." }),
    aMessage({
      id: "01JMESSAGE0000000000000002",
      from_agent_id: "reviewer",
      to_agent_id: "author",
      body: "Second message.",
      created_at: "2026-01-02T00:00:00Z",
    }),
  ])

  renderMessages()

  expect(await screen.findByText("First message.")).toBeTruthy()
  expect(screen.getAllByText("coding, testing").length).toBeGreaterThan(0)
  expect(screen.getAllByText("code-review").length).toBeGreaterThan(0)
  expect(
    screen.getByText("First message.").compareDocumentPosition(screen.getByText("Second message.")),
  ).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
  expect(screen.getByRole("tab", { name: /Messages2 messages/ })).toBeTruthy()
})

it("says when agents have said nothing", async () => {
  stubMessages([])
  renderMessages()

  expect(await screen.findByText("The agents have said nothing yet")).toBeTruthy()
})

it("refetches an open messages tab when a message is sent", async () => {
  let messages = [aMessage()]
  stubMessages(() => messages)
  renderMessages({ stream: true })

  expect(await screen.findByText("First message.")).toBeTruthy()
  const stream: FakeEventSource = latestSource()
  act(() => {
    stream.succeed()
    stream.beat()
  })
  messages = [aMessage({ body: "New message." })]

  const event: DomainEvent = {
    event: "message_sent",
    data: aMessage({ id: "01JMESSAGE0000000000000002", body: "New message." }),
  }
  act(() => stream.emit(event.event, event.data))

  expect(await screen.findByText("New message.")).toBeTruthy()
})

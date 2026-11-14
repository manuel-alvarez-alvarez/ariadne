// @vitest-environment jsdom

/**
 * What a streamed chunk draws again in the console: only the block it
 * changes, never a question card or a permission picker beside it, while a
 * change of the card or the picker still draws them again.
 */

import { act, fireEvent, screen, within } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

import type { AgentEventDto } from "@/api"
import { latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, renderScreen } from "@/test/harness"

import { consoleViewKey, updateViewState } from "./console-view-state"
import { SessionConsole } from "./session-console"

const renders = vi.hoisted(() => ({ card: 0, picker: 0 }))

// Count the renders of a question card and a permission picker; each still draws as it does.
vi.mock("./questions", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./questions")>()
  return {
    ...actual,
    QuestionCard: (props: Parameters<typeof actual.QuestionCard>[0]) => {
      renders.card += 1
      return <actual.QuestionCard {...props} />
    },
  }
})
vi.mock("./blocks", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./blocks")>()
  return {
    ...actual,
    PermissionPicker: (props: Parameters<typeof actual.PermissionPicker>[0]) => {
      renders.picker += 1
      return <actual.PermissionPicker {...props} />
    },
  }
})

const SESSION_ID = "01JSESS0000000000000000002"

let next = 0
function event(kind: string, payload: unknown = {}): AgentEventDto {
  next += 1
  return {
    id: `01EVENT${String(next).padStart(19, "0")}`,
    kind,
    payload,
    summary: `${kind} summary`,
    created_at: "2026-11-14T10:00:00Z",
  }
}

beforeEach(() => {
  updateViewState(consoleViewKey("http://127.0.0.1:7676", SESSION_ID), {
    draft: "",
    scrollTop: 0,
    following: true,
    folded: true,
    blockFolds: {},
    shown: undefined,
    failed: [],
  })
  stubEventSource()
  daemonFetch.mockImplementation(async () => Response.json([]))
})

function renderConsole(snapshot: AgentEventDto[]) {
  renderScreen(<SessionConsole sessionId={SESSION_ID} status="running" />)
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", snapshot)
  })
  return source
}

function transcript() {
  return screen.getByRole("log", { name: "Console transcript" })
}

it("draws a question card again for its own change, not for a message streamed after it", () => {
  const source = renderConsole([
    event("agent_request", {
      request_id: "01RQ",
      hint: "Pick a base",
      question: { text: "Which base?", options: [{ id: "main", label: "Main" }] },
    }),
  ])
  renders.card = 0

  act(() => source.emit("event", event("agent_message_chunk", { text: "still " })))
  act(() => source.emit("event", event("agent_message_chunk", { text: "working" })))
  within(transcript()).getByText("still working")
  expect(renders.card).toBe(0)

  act(() =>
    source.emit(
      "event",
      event("agent_request.resolved", {
        request_id: "01RQ",
        resolution: "answered",
        answer: { text: "Main" },
      }),
    ),
  )
  within(transcript()).getByText("Answered · Main")
  expect(renders.card).toBeGreaterThan(0)
})

it("draws a permission picker again for its own change, not for a message streamed after it", () => {
  const source = renderConsole([
    event("permission_request", {
      acp: { toolCallId: "call-1", title: "Bash", rawInput: { command: "rm -rf build" } },
      options: [
        { optionId: "allow", name: "Allow", kind: "allow_once" },
        { optionId: "reject", name: "Reject", kind: "reject_once" },
      ],
    }),
  ])
  const log = transcript()
  fireEvent.click(within(log).getByRole("button", { name: /Permission/ }))
  renders.picker = 0

  act(() => source.emit("event", event("agent_message_chunk", { text: "waiting" })))
  within(log).getByText("waiting")
  expect(renders.picker).toBe(0)

  const picker = screen.getByRole("listbox", { name: "Answer the permission" })
  fireEvent.keyDown(picker, { key: "ArrowDown" })
  expect(renders.picker).toBeGreaterThan(0)
  expect(
    within(picker)
      .getByRole("option", { name: /Reject/ })
      .getAttribute("aria-selected"),
  ).toBe("true")
})

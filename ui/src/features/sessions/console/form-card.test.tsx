// @vitest-environment jsdom

/**
 * The card of a question an agent asks with its own tool, drawn in the
 * session console from the console stream and `GET .../questions`, and
 * answered on the question's own route.
 */

import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import type { AgentEventDto } from "@/api"
import { latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"

import { SessionConsole } from "./session-console"

const SESSION_ID = "01JSESS0000000000000000002"
const ANSWER_PATH = `/v1/sessions/${SESSION_ID}/questions/Q1/answer`

const SCHEMA = {
  type: "object",
  properties: {
    db: {
      type: "string",
      title: "Database",
      oneOf: [
        { const: "pg", title: "Postgres", description: "The server one" },
        { const: "lite", title: "SQLite" },
      ],
    },
    extras: {
      type: "array",
      title: "Extras",
      items: {
        anyOf: [
          { const: "ci", title: "CI" },
          { const: "docs", title: "Docs" },
        ],
      },
    },
    note: { type: "string", title: "Note" },
  },
  required: ["db"],
}

let next = 0
function event(kind: string, payload: unknown): AgentEventDto {
  next += 1
  return {
    id: `01FORM${String(next).padStart(20, "0")}`,
    kind,
    payload,
    summary: kind,
    created_at: "2026-11-25T10:00:00Z",
  }
}

const asked = (id = "Q1") =>
  event("question", {
    question: {
      id,
      session_id: SESSION_ID,
      message: "Which database should I use?",
      schema: SCHEMA,
      created_at: "2026-11-25T10:00:00Z",
    },
  })

const posts: { path: string; body: unknown }[] = []
let listed: unknown[] = []
let reply: () => Response = () => new Response(null, { status: 204 })

beforeEach(() => {
  posts.length = 0
  listed = []
  reply = () => new Response(null, { status: 204 })
  stubEventSource()
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const path = new URL(request.url).pathname
    if (request.method === "GET") return Response.json(path.endsWith("/questions") ? listed : [])
    posts.push({ path, body: JSON.parse((await request.text()) || "null") })
    return reply()
  })
})

afterEach(cleanup)

function renderConsole(snapshot: AgentEventDto[]) {
  renderScreen(<SessionConsole host="local" sessionId={SESSION_ID} status="idle" />)
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", snapshot)
  })
  return source
}

const card = () => screen.getByRole("region", { name: "Agent question" })

it("draws the message, each field and each choice with its description as written", () => {
  renderConsole([asked()])

  const form = within(card())
  form.getByText("Which database should I use?")
  const database = form.getByRole("group", { name: /Database/ })
  within(database).getByRole("radio", { name: /Postgres/ })
  within(database).getByText("The server one")
  within(database).getByRole("radio", { name: /SQLite/ })
  within(form.getByRole("group", { name: /Extras/ })).getAllByRole("checkbox")
  form.getByRole("textbox", { name: /Note/ })
  expect(
    card().compareDocumentPosition(screen.getByRole("textbox", { name: "Message the agent" })) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy()
})

it("sends the answers as accept with the content of the form", async () => {
  renderConsole([asked()])
  const form = within(card())

  expect((form.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(form.getByRole("radio", { name: /SQLite/ }))
  fireEvent.click(form.getByRole("checkbox", { name: "CI" }))
  fireEvent.click(form.getByRole("checkbox", { name: "Docs" }))
  fireEvent.change(form.getByRole("textbox", { name: /Note/ }), { target: { value: "soon" } })
  fireEvent.click(form.getByRole("button", { name: "Send" }))

  await waitFor(() =>
    expect(posts).toEqual([
      {
        path: ANSWER_PATH,
        body: { action: "accept", content: { db: "lite", extras: ["ci", "docs"], note: "soon" } },
      },
    ]),
  )
  await screen.findByText(/Answer sent/)
})

it("sends decline when the user dismisses the question", async () => {
  renderConsole([asked()])

  fireEvent.click(within(card()).getByRole("button", { name: "Dismiss" }))

  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: { action: "decline" } }]))
})

it("shows a failed answer on the card and lets the user try again", async () => {
  reply = () => errorResponse(409, "question_closed", "question Q1 is closed")
  renderConsole([asked()])
  fireEvent.click(within(card()).getByRole("radio", { name: /Postgres/ }))
  fireEvent.click(within(card()).getByRole("button", { name: "Send" }))

  await within(card()).findByRole("alert")
  expect((within(card()).getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(
    false,
  )
})

it("closes the card with its answer when the resolved event lands, live and in a replay", () => {
  const resolved = event("question.resolved", {
    question_id: "Q1",
    action: "accept",
    content: { db: "pg", extras: ["docs"] },
    closed_by: "user",
  })
  const source = renderConsole([asked()])
  act(() => source.emit("event", resolved))
  const live = card().textContent
  cleanup()
  renderConsole([asked(), resolved])

  expect(card().textContent).toBe(live)
  expect(card().textContent).toContain("Database → Postgres")
  expect(card().textContent).toContain("Extras → Docs")
  expect(within(card()).queryByRole("button", { name: "Send" })).toBeNull()
})

it("shows a dismissed question closed", () => {
  renderConsole([asked(), event("question.resolved", { question_id: "Q1", action: "decline" })])

  expect(card().textContent).toContain("Dismissed")
})

it("restores an open question older than the snapshot's page from the list", async () => {
  listed = [
    {
      id: "QOLD",
      session_id: SESSION_ID,
      message: "An older question",
      schema: SCHEMA,
      created_at: "2026-11-25T09:00:00Z",
    },
  ]
  renderConsole([])

  await screen.findByText("An older question")
})

it("says the session waits for the user while a question is open", () => {
  const source = renderConsole([asked()])
  const status = () => screen.getByRole("region", { name: "Session status" })

  expect(status().textContent).toContain("waiting for you")
  act(() =>
    source.emit("event", event("question.resolved", { question_id: "Q1", action: "cancel" })),
  )
  expect(status().textContent).not.toContain("waiting for you")
})

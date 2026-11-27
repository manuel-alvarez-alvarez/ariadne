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
/** Holds the first session's question read open until a test lets it go. */
let held: Promise<void> | undefined
/** The older page of events `GET /v1/events` answers. */
let history: unknown[] = []
let reply: () => Response = () => new Response(null, { status: 204 })

beforeEach(() => {
  posts.length = 0
  // The daemon lists the question the snapshots below ask, while it is open.
  listed = [
    {
      id: "Q1",
      session_id: SESSION_ID,
      message: "Which database should I use?",
      schema: SCHEMA,
      created_at: "2026-11-25T10:00:00Z",
    },
  ]
  held = undefined
  history = []
  reply = () => new Response(null, { status: 204 })
  stubEventSource()
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const path = new URL(request.url).pathname
    if (request.method === "GET" && path.endsWith(`${SESSION_ID}/questions`) && held)
      return held.then(() => Response.json(listed))
    if (request.method === "GET")
      return Response.json(
        path.endsWith("/questions")
          ? path.includes(SESSION_ID)
            ? listed
            : []
          : path.endsWith("/v1/events")
            ? history
            : [],
      )
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

it("draws one question at a time with large choice rows", () => {
  renderConsole([asked()])
  const form = within(card())
  form.getByText("1 of 3")
  form.getByRole("button", { name: /Postgres/ })
  form.getByText("The server one")
  expect(form.queryByRole("textbox", { name: /Note/ })).toBeNull()
})

it("sends the answers as accept with the content of the form", async () => {
  renderConsole([asked()])
  const form = within(card())
  fireEvent.click(form.getByRole("button", { name: /SQLite/ }))
  fireEvent.click(form.getByRole("checkbox", { name: /CI/ }))
  fireEvent.click(form.getByRole("checkbox", { name: /Docs/ }))
  fireEvent.click(form.getByRole("button", { name: "Next" }))
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
})

it("sends decline when the user dismisses the question", async () => {
  renderConsole([asked()])

  fireEvent.click(within(card()).getByRole("button", { name: "Dismiss" }))

  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: { action: "decline" } }]))
})

it("shows a failed answer on the card and lets the user try again", async () => {
  reply = () => errorResponse(409, "question_closed", "question Q1 is closed")
  renderConsole([asked()])
  fireEvent.click(within(card()).getByRole("button", { name: /Postgres/ }))
  fireEvent.click(within(card()).getByRole("button", { name: "Skip" }))
  fireEvent.click(within(card()).getByRole("button", { name: "Skip" }))

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

/** The card's marker, border, fold controls and scroll areas, as the console draws them. */
function cardMarks() {
  const nodes = Array.from(card().querySelectorAll("*"))
  return {
    purpleIcons: card().querySelectorAll(".lucide-circle-help.text-question-fg").length,
    leftBorders: card().querySelectorAll(".border-l").length,
    folds: card().querySelectorAll("[aria-expanded]").length,
    scrollAreas: nodes.filter((node) =>
      /(^|\s)(overflow-(auto|scroll|x-auto|y-auto)|max-h-)/.test(node.getAttribute("class") ?? ""),
    ).length,
  }
}

it("marks an open and a closed question with one purple icon, folds only closed cards, and lets it grow", () => {
  const unmarked = { purpleIcons: 1, leftBorders: 0, folds: 0, scrollAreas: 0 }
  const source = renderConsole([asked()])
  expect(cardMarks()).toEqual(unmarked)

  act(() =>
    source.emit("event", event("question.resolved", { question_id: "Q1", action: "decline" })),
  )
  expect(card().textContent).toContain("Dismissed")
  expect(cardMarks()).toEqual({ ...unmarked, folds: 1 })
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

it("keeps a question restored from the list, with its answer, when it closes and the list is read again", async () => {
  listed = [
    {
      id: "QOLD",
      session_id: SESSION_ID,
      message: "An older question",
      schema: SCHEMA,
      created_at: "2026-11-25T09:00:00Z",
    },
  ]
  const source = renderConsole([])
  await screen.findByText("An older question")

  listed = []
  const resolved = event("question.resolved", {
    question_id: "QOLD",
    action: "accept",
    content: { db: "lite" },
  })
  act(() => source.emit("event", resolved))
  // A reconnect: the snapshot carries the close, and the list no longer has the question.
  act(() => source.emit("snapshot", [resolved]))

  await waitFor(() => expect(card().textContent).toContain("Database → SQLite"))
  expect(card().textContent).toContain("Database → SQLite")
})

it("draws a question closed as closed, not waiting, when a reconnect omits it and its close", async () => {
  listed = [
    {
      id: "QOLD",
      session_id: SESSION_ID,
      message: "An older question",
      schema: SCHEMA,
      created_at: "2026-11-25T09:00:00Z",
    },
  ]
  const source = renderConsole([])
  await screen.findByText("An older question")

  listed = []
  act(() => source.emit("snapshot", []))

  await waitFor(() => expect(card().textContent).toContain("Closed"))
  expect(within(card()).queryByRole("button", { name: "Send" })).toBeNull()
  expect(screen.getByRole("region", { name: "Session status" }).textContent).not.toContain(
    "waiting for you",
  )
})

it("draws a close whose question is in an older page with the question's own labels", async () => {
  listed = []
  history = [asked("QOLD")].map((one) => ({
    ...one,
    payload: {
      question: {
        ...(one.payload as { question: object }).question,
        message: "From the older page",
      },
    },
  }))
  renderConsole([
    event("question.resolved", {
      question_id: "QOLD",
      action: "accept",
      content: { db: "lite" },
    }),
  ])

  await waitFor(() => expect(card().textContent).toContain("Database → SQLite"))
  expect(card().textContent).toContain("Database → SQLite")
})

it("draws no card of the session it left when its question read lands late", async () => {
  let release = () => {}
  held = new Promise<void>((resolve) => {
    release = resolve
  })
  listed = [
    {
      id: "QOLD",
      session_id: SESSION_ID,
      message: "A question of the first session",
      schema: SCHEMA,
      created_at: "2026-11-25T09:00:00Z",
    },
  ]
  const { rerender } = renderScreen(
    <SessionConsole host="local" sessionId={SESSION_ID} status="idle" />,
  )
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [])
  })
  // The other session has sent no snapshot yet, so it has read nothing of its own.
  rerender(<SessionConsole host="local" sessionId="01JOTHER00000000000000000" status="idle" />)
  await act(async () => {
    release()
    await held
  })

  expect(screen.queryByText("A question of the first session")).toBeNull()
})

it("closes a question the snapshot carries when the list no longer holds it", async () => {
  listed = []
  renderConsole([asked(), event("agent_message", { text: "later" })])

  await waitFor(() => expect(card().textContent).toContain("Closed"))
  expect(within(card()).queryByRole("button", { name: "Send" })).toBeNull()
  expect(screen.getByRole("region", { name: "Session status" }).textContent).not.toContain(
    "waiting for you",
  )
})

it("recovers the answer of a question a reconnect omits from the history", async () => {
  listed = [
    {
      id: "Q1",
      session_id: SESSION_ID,
      message: "Which database should I use?",
      schema: SCHEMA,
      created_at: "2026-11-25T09:00:00Z",
    },
  ]
  const source = renderConsole([event("agent_message", { text: "first page" })])
  await screen.findByText("Which database should I use?")

  listed = []
  history = [
    event("question.resolved", { question_id: "Q1", action: "accept", content: { db: "pg" } }),
  ]
  act(() => source.emit("snapshot", [event("agent_message", { text: "newest page" })]))

  await waitFor(() => expect(card().textContent).toContain("Database → Postgres"))
})

it("keeps a question the stream carried when a newer snapshot drops it and its close", async () => {
  listed = []
  const source = renderConsole([])
  const question = asked()
  act(() => source.emit("event", question))
  await screen.findByText("Which database should I use?")

  history = [
    question,
    event("question.resolved", { question_id: "Q1", action: "accept", content: { db: "pg" } }),
  ]
  act(() => source.emit("snapshot", [event("agent_message", { text: "newest page" })]))

  await waitFor(() => expect(card().textContent).toContain("Database → Postgres"))
})

it("says the open questions could not be read, and reads them again on Retry", async () => {
  let failing = true
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const path = new URL(request.url).pathname
    if (path.endsWith(`${SESSION_ID}/questions`))
      return failing ? errorResponse(503, "unavailable", "down") : Response.json(listed)
    return Response.json([])
  })
  renderConsole([event("agent_message", { text: "newest page" })])

  const alert = await screen.findByText(/Could not read the open questions/)
  failing = false
  fireEvent.click(within(alert.parentElement as HTMLElement).getByRole("button", { name: "Retry" }))

  await screen.findByText("Which database should I use?")
  expect(screen.queryByText(/Could not read the open questions/)).toBeNull()
})

it("keeps a closed question a snapshot carried when a newer snapshot omits it", async () => {
  listed = []
  const question = asked()
  const close = event("question.resolved", {
    question_id: "Q1",
    action: "accept",
    content: { db: "pg" },
  })
  const source = renderConsole([question, close])
  await waitFor(() => expect(card().textContent).toContain("Database → Postgres"))

  history = [question, close]
  act(() => source.emit("snapshot", [event("agent_message", { text: "newest page" })]))

  await waitFor(() => expect(card().textContent).toContain("Database → Postgres"))
})

it("keeps composer number and Enter keys outside the question shortcuts", () => {
  renderConsole([asked()])
  const composer = screen.getByRole("textbox", { name: "Message the agent" })
  fireEvent.focus(composer)
  fireEvent.keyDown(composer, { key: "2" })
  expect(within(card()).getByText("1 of 3")).toBeTruthy()
  expect(posts).toEqual([])
})

// @vitest-environment jsdom

/**
 * The card of a question an agent asks with its own tool, drawn in the
 * turn console from `GET .../questions`, and answered on the question's own
 * route.
 */

import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import { latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"
import { aQuestionStep, aSnapshot, aTurn } from "@/test/trajectory"

import { TrajectoryConsole } from "../trajectory/trajectory-console"

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

const posts: { path: string; body: unknown }[] = []
let listed: unknown[] = []
/** Holds the first session's question read open until a test lets it go. */
let held: Promise<void> | undefined
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
  reply = () => new Response(null, { status: 204 })
  stubEventSource()
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const path = new URL(request.url).pathname
    if (request.method === "GET" && path.endsWith(`${SESSION_ID}/questions`) && held)
      return held.then(() => Response.json(listed))
    if (request.method === "GET")
      return Response.json(
        path.endsWith("/questions") ? (path.includes(SESSION_ID) ? listed : []) : [],
      )
    posts.push({ path, body: JSON.parse((await request.text()) || "null") })
    return reply()
  })
})

afterEach(cleanup)

/** The console with a running turn whose open question `Q1` is the one `listed` holds. */
function renderConsole(attention = true) {
  const turn = aTurn({ state: "running", ended_at: null })
  const question = aQuestionStep(turn.id, { request_id: "Q1" })
  const turns = [{ ...turn, attention: attention ? [question] : [] }]
  renderScreen(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="running" />)
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", aSnapshot(turns))
  })
  return source
}

const card = () => screen.getByRole("region", { name: "Agent question" })
const opened = () => screen.findByRole("region", { name: "Agent question" })

it("draws one question at a time with large choice rows", async () => {
  renderConsole()
  await opened()
  const form = within(card())
  form.getByText("1 of 3")
  form.getByRole("button", { name: /Postgres/ })
  form.getByText("The server one")
  expect(form.queryByRole("textbox", { name: /Note/ })).toBeNull()
})

it("sends the answers as accept with the content of the form", async () => {
  renderConsole()
  await opened()
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
  renderConsole()
  await opened()

  fireEvent.click(within(card()).getByRole("button", { name: "Dismiss" }))

  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: { action: "decline" } }]))
})

it("shows a failed answer on the card and lets the user try again", async () => {
  reply = () => errorResponse(409, "question_closed", "question Q1 is closed")
  renderConsole()
  await opened()
  fireEvent.click(within(card()).getByRole("button", { name: /Postgres/ }))
  fireEvent.click(within(card()).getByRole("button", { name: "Skip" }))
  fireEvent.click(within(card()).getByRole("button", { name: "Skip" }))

  await within(card()).findByRole("alert")
  expect((within(card()).getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(
    false,
  )
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
    <TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />,
  )
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([]))
  })
  // The other session has sent no snapshot yet, so it has read nothing of its own.
  rerender(<TrajectoryConsole host="local" sessionId="01JOTHER00000000000000000" status="idle" />)
  await act(async () => {
    release()
    await held
  })

  expect(screen.queryByText("A question of the first session")).toBeNull()
})

it("keeps composer number and Enter keys outside the question shortcuts", async () => {
  renderConsole()
  await opened()
  const composer = screen.getByRole("textbox", { name: "Message the agent" })
  fireEvent.focus(composer)
  fireEvent.keyDown(composer, { key: "2" })
  expect(within(card()).getByText("1 of 3")).toBeTruthy()
  expect(posts).toEqual([])
})

it("says the session waits for the user while a question is open", async () => {
  renderConsole()
  await opened()

  expect(screen.getByRole("region", { name: "Session status" }).textContent).toContain(
    "waiting for you",
  )
})

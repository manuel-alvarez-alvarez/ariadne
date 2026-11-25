// @vitest-environment jsdom
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it } from "vitest"
import { daemonFetch, renderScreen } from "@/test/harness"
import { FormCard } from "./form-card"
import { formFields } from "./form-questions"

// Shape emitted by claude-agent-acp askUserQuestionsToCreateRequest.
const schema = {
  type: "object",
  properties: {
    question_0: {
      type: "string",
      title: "Database",
      description: "Choose **storage**",
      oneOf: [
        {
          const: "Postgres (Recommended)",
          title: "Postgres (Recommended)",
          description: "Shared server",
        },
        { const: "SQLite", title: "SQLite" },
      ],
    },
    question_0_custom: { type: "string", title: "Other" },
    question_1: {
      type: "array",
      title: "Extras",
      description: "Choose extras",
      items: { anyOf: [{ const: "CI", title: "CI" }] },
    },
    question_1_custom: { type: "string", title: "Other" },
  },
}
afterEach(cleanup)
function open(
  source = schema,
  closed?: { action: "accept" | "decline"; content: Record<string, unknown> },
) {
  const posts: unknown[] = []
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const r = input instanceof Request ? input : new Request(input)
    posts.push(JSON.parse(await r.text()))
    return new Response(null, { status: 204 })
  })
  renderScreen(
    <FormCard
      host="local"
      sessionId="S1"
      live
      question={{
        id: "Q1",
        message: "Please answer the following questions.",
        fields: formFields(source),
        createdAt: "2026-11-25T10:00:00Z",
        closed,
      }}
    />,
  )
  return posts
}
it("groups Claude companions and advances a single pick before sending multiple picks", async () => {
  const posts = open()
  const card = screen.getByRole("region", { name: "Agent question" })
  expect(document.activeElement).toBe(card)
  expect(card.querySelector("strong")?.textContent).toBe("Database")
  expect(card.querySelector(".text-xs, .font-mono")).toBeNull()
  expect(card.querySelector("p strong")?.textContent).toBe("storage")
  screen.getByText("1 of 2")
  screen.getByText("Recommended")
  fireEvent.keyDown(card, { key: "1" })
  screen.getByText("2 of 2")
  expect(posts).toEqual([])
  screen.getByRole("button", { name: "Question 1, answered" })
  fireEvent.click(screen.getByRole("checkbox", { name: /CI/ }))
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Docs" } })
  fireEvent.click(screen.getByRole("button", { name: "Send" }))
  await waitFor(() =>
    expect(posts).toEqual([
      {
        action: "accept",
        content: {
          question_0: "Postgres (Recommended)",
          question_1: ["CI"],
          question_1_custom: "Docs",
        },
      },
    ]),
  )
  expect(card.textContent).toContain("Extras → CI, Docs")
  expect(card.textContent).not.toContain("question_1_custom")
})
it("sends only custom text and skips without content", async () => {
  const posts = open()
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Redis" } })
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" })
  fireEvent.click(screen.getByRole("button", { name: "Skip" }))
  await waitFor(() =>
    expect(posts).toEqual([{ action: "accept", content: { question_0_custom: "Redis" } }]),
  )
})
it("opens an answered summary read-only with its picks marked", () => {
  open(schema, { action: "accept", content: { question_0: "SQLite", question_1_custom: "Docs" } })
  fireEvent.click(screen.getByRole("button", { name: /Database → SQLite/ }))
  expect(screen.getByRole("button", { name: /SQLite/ }).getAttribute("aria-pressed")).toBe("true")
  expect((screen.getByRole("button", { name: /SQLite/ }) as HTMLButtonElement).disabled).toBe(true)
  expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull()
})
it("validates a number step and keeps shortcuts inside the card", async () => {
  const posts = open({
    type: "object",
    properties: { count: { type: "number", title: "Count" } },
  } as unknown as typeof schema)
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "bad" } })
  fireEvent.click(screen.getByRole("button", { name: "Send" }))
  screen.getByRole("alert")
  expect(posts).toEqual([])
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "3" } })
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" })
  await waitFor(() => expect(posts).toEqual([{ action: "accept", content: { count: 3 } }]))
})

it("uses the message for one question and sends its choice immediately", async () => {
  const posts = open({
    type: "object",
    properties: {
      question_0: schema.properties.question_0,
      question_0_custom: schema.properties.question_0_custom,
    },
  } as typeof schema)
  screen.getByText("Please answer the following questions.")
  expect(screen.queryByText("1 of 1")).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: /SQLite/ }))
  await waitFor(() =>
    expect(posts).toEqual([{ action: "accept", content: { question_0: "SQLite" } }]),
  )
})
it("moves between rows and returns to the previous question", () => {
  open()
  const card = screen.getByRole("region", { name: "Agent question" })
  fireEvent.keyDown(card, { key: "ArrowDown" })
  expect(document.activeElement?.textContent).toContain("Postgres")
  fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowDown" })
  expect(document.activeElement?.textContent).toContain("SQLite")
  fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowUp" })
  expect(document.activeElement?.textContent).toContain("Postgres")
  fireEvent.click(screen.getByRole("button", { name: /SQLite/ }))
  fireEvent.keyDown(card, { key: "Backspace" })
  screen.getByText("1 of 2")
  fireEvent.keyDown(card, { key: "3" })
  expect(document.activeElement).toBe(screen.getByRole("textbox"))
})

it("keeps a dismissed card to one status and opens its full form read-only", () => {
  open(schema, { action: "decline", content: {} })
  const summary = screen.getByRole("button", { name: "Dismissed" })
  expect(screen.queryByRole("textbox")).toBeNull()
  fireEvent.click(summary)
  expect((screen.getByRole("textbox") as HTMLInputElement).disabled).toBe(true)
  expect(screen.queryByRole("button", { name: "Skip" })).toBeNull()
})

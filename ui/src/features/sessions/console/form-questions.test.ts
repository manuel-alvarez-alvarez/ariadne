import { expect, it } from "vitest"

import type { AgentEventDto, QuestionDto } from "@/api"

import { formContent, formFields, formQuestions } from "./form-questions"

const SCHEMA = {
  type: "object",
  properties: {
    db: {
      type: "string",
      title: "Database",
      description: "Where the rows live",
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
    note: { type: "string" },
    count: { type: "integer", title: "Count" },
    ok: { type: "boolean", title: "Sure?" },
  },
  required: ["db"],
}

let at = 0
function event(kind: string, payload: unknown): AgentEventDto {
  at += 1
  return {
    id: `E${String(at).padStart(3, "0")}`,
    kind,
    payload,
    summary: kind,
    created_at: "2026-11-25T10:00:00Z",
  }
}

const asked = (id: string) =>
  event("question", {
    question: { id, message: `Message ${id}`, schema: SCHEMA, created_at: "2026-11-25T10:00:00Z" },
  })

it("reads each field of the schema with its choices as the agent wrote them", () => {
  const [db, extras, note, count, ok] = formFields(SCHEMA)

  expect(db).toEqual({
    key: "db",
    title: "Database",
    description: "Where the rows live",
    required: true,
    kind: {
      type: "single",
      choices: [
        { value: "pg", title: "Postgres", description: "The server one" },
        { value: "lite", title: "SQLite" },
      ],
    },
  })
  expect(extras?.kind).toEqual({
    type: "multi",
    choices: [
      { value: "ci", title: "CI" },
      { value: "docs", title: "Docs" },
    ],
  })
  expect(note).toMatchObject({ title: "note", required: false, kind: { type: "text" } })
  expect(count?.kind).toEqual({ type: "number", integer: true })
  expect(ok?.kind.type).toBe("single")
})

it("draws a question the same from the events and from the list", () => {
  const dto: QuestionDto = {
    id: "Q1",
    session_id: "S",
    message: "Message Q1",
    schema: SCHEMA as never,
    created_at: "2026-11-25T10:00:00Z",
  }

  expect(formQuestions([asked("Q1")], [])).toEqual(formQuestions([], [dto]))
})

it("closes a question with its resolved event, whatever order a replay brings them in", () => {
  const resolved = event("question.resolved", {
    question_id: "Q1",
    action: "accept",
    content: { db: "pg" },
    closed_by: "user",
  })

  for (const events of [
    [asked("Q1"), resolved],
    [resolved, asked("Q1")],
  ]) {
    const [question] = formQuestions(events, [])
    expect(question?.closed).toEqual({
      action: "accept",
      content: { db: "pg" },
      closedBy: "user",
    })
  }
})

it("keeps a listed question that a resolved event closes, with its answer", () => {
  const dto: QuestionDto = {
    id: "Q1",
    session_id: "S",
    message: "m",
    schema: SCHEMA as never,
    created_at: "2026-11-25T09:00:00Z",
  }
  const resolved = event("question.resolved", {
    question_id: "Q1",
    action: "accept",
    content: { db: "pg" },
  })

  const [question] = formQuestions([resolved], [dto])
  expect(question?.message).toBe("m")
  expect(question?.fields.map((field) => field.key)).toContain("db")
  expect(question?.closed?.content).toEqual({ db: "pg" })
})

it("keeps the answer of a closed question that no event or list carries", () => {
  const resolved = event("question.resolved", {
    question_id: "Q9",
    action: "accept",
    content: { db: "pg" },
  })

  const [question] = formQuestions([resolved], [])
  expect(question?.id).toBe("Q9")
  expect(question?.fields.map((field) => field.title)).toEqual(["db"])
  expect(question?.closed?.action).toBe("accept")
})

it("puts a listed question older than the events before the ones they carry", () => {
  const dto: QuestionDto = {
    id: "QOLD",
    session_id: "S",
    message: "old",
    schema: SCHEMA as never,
    created_at: "2026-11-25T09:00:00Z",
  }

  expect(formQuestions([asked("QNEW")], [dto]).map((question) => question.id)).toEqual([
    "QOLD",
    "QNEW",
  ])
})

it("sends only what the person filled, and stops on an empty required field", () => {
  const fields = formFields(SCHEMA)

  expect(formContent(fields, {})).toEqual({ problem: "Database needs an answer" })
  expect(
    formContent(fields, { db: "lite", extras: ["ci"], note: "  ", count: "3", ok: false }),
  ).toEqual({ content: { db: "lite", extras: ["ci"], count: 3, ok: false } })
  expect(formContent(fields, { db: "lite", count: "2.5" })).toEqual({
    problem: "Count takes a whole number",
  })
})

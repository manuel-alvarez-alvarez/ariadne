/**
 * The questions an agent asks with its own tool, an ACP `elicitation/create`
 * in form mode (021): what the console reads of them, apart from the
 * `wait_for_user` cards of `questions.tsx`.
 *
 * A question reaches the console twice. The `question` and `question.resolved`
 * events of the stream carry it and its close, so a replay draws the same
 * state as the live stream. `GET /v1/sessions/{id}/questions` lists the open
 * ones with every snapshot, so a question older than the snapshot's page, and
 * a reconnect, still draw it. `formQuestions` folds both into one list.
 */

import { type AgentEventDto, api, type Host, type QuestionDto, unwrap } from "@/api"

/** One choice of a field: the value the answer carries, and the text the agent wrote for it. */
export interface FormChoice {
  value: string | number | boolean
  title: string
  description?: string
}

type FieldKind =
  | { type: "single"; choices: FormChoice[] }
  | { type: "multi"; choices: FormChoice[] }
  | { type: "text" }
  | { type: "number"; integer: boolean }

/** One field of a form, read off its `requestedSchema`. */
export interface FormField {
  /** The property name, which the answer's content is keyed by. */
  key: string
  /** The field's `title`, else its key. */
  title: string
  description?: string
  required: boolean
  kind: FieldKind
}

/** How a question closed, as its `question.resolved` event says. */
export interface FormClosed {
  action: "accept" | "decline" | "cancel"
  content: Record<string, unknown>
  /** Who closed it: `user`, `agent` or `session`. */
  closedBy?: string
}

export interface FormQuestion {
  id: string
  message: string
  fields: FormField[]
  createdAt: string
  /** None while it waits for an answer. */
  closed?: FormClosed
  /** Only its close is known: the question itself is in an older page of the history. */
  orphan?: true
  /** It left the list with no close read: the close is in an older page of the history. */
  lost?: true
  /** The id of the event that asked it, where an event did. */
  eventId?: string
}

type Json = Record<string, unknown>

function record(value: unknown): Json | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Json)
    : undefined
}

function text(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined
}

function scalar(value: unknown): FormChoice["value"] | undefined {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean"
    ? value
    : undefined
}

function titleOf(value: FormChoice["value"]): string {
  return String(value)
}

/** `oneOf` or `anyOf` with `const` and `title`, else a plain `enum` with its `enumNames`. */
function choicesOf(schema: unknown): FormChoice[] {
  const source = record(schema)
  if (!source) return []
  const titled = [source.oneOf, source.anyOf].find(Array.isArray)
  if (titled) {
    return (titled as unknown[]).flatMap((entry) => {
      const choice = record(entry)
      const value = scalar(choice?.const)
      if (!choice || value === undefined) return []
      const description = text(choice.description)
      return [
        {
          value,
          title: text(choice.title) ?? titleOf(value),
          ...(description === undefined ? {} : { description }),
        },
      ]
    })
  }
  const names = Array.isArray(source.enumNames) ? source.enumNames : []
  return (Array.isArray(source.enum) ? source.enum : []).flatMap((entry, at) => {
    const value = scalar(entry)
    return value === undefined ? [] : [{ value, title: text(names[at]) ?? titleOf(value) }]
  })
}

function kindOf(property: Json): FieldKind {
  switch (property.type) {
    case "array":
      return { type: "multi", choices: choicesOf(property.items) }
    case "boolean":
      return {
        type: "single",
        choices: [
          { value: true, title: "Yes" },
          { value: false, title: "No" },
        ],
      }
    case "number":
    case "integer":
      return { type: "number", integer: property.type === "integer" }
    default: {
      const choices = choicesOf(property)
      return choices.length === 0 ? { type: "text" } : { type: "single", choices }
    }
  }
}

/** The fields of a `requestedSchema`, in the order it lists them. */
export function formFields(schema: unknown): FormField[] {
  const source = record(schema)
  const required = new Set(
    (Array.isArray(source?.required) ? source.required : []).filter(
      (key): key is string => typeof key === "string",
    ),
  )
  return Object.entries(record(source?.properties) ?? {}).flatMap(([key, value]) => {
    const property = record(value)
    if (!property) return []
    const description = text(property.description)
    return [
      {
        key,
        title: text(property.title) ?? key,
        ...(description === undefined ? {} : { description }),
        required: required.has(key),
        kind: kindOf(property),
      },
    ]
  })
}

function questionOf(dto: QuestionDto): FormQuestion {
  return {
    id: dto.id,
    message: dto.message,
    fields: formFields(dto.schema),
    createdAt: dto.created_at,
  }
}

function closedOf(payload: unknown): FormClosed {
  const action = text(record(payload)?.action)
  return {
    action: action === "accept" || action === "decline" ? action : "cancel",
    content: record(record(payload)?.content) ?? {},
    closedBy: text(record(payload)?.closed_by),
  }
}

/**
 * Every question of the session as the console draws it, oldest first: the
 * ones the events carry, each closed by its `question.resolved` event, and
 * the ones the console read from the list, which the events do not carry.
 *
 * `listed` is the latest read, the questions open now, and `through` the
 * newest event the console held when it asked for them. `seen` is every
 * question any read of this session listed. A question the list no longer
 * holds and no event closes closed while the console was away. Its close is
 * in an older page of the history for a question older than the events (it
 * is `lost` until the console finds it), and for one the events carry up to
 * `through` there is nothing more to find: it draws closed with no answer.
 * A close whose question nothing carries is an `orphan`: its form is in an
 * older page of the history, which the console loads.
 */
export function formQuestions(
  events: readonly AgentEventDto[],
  listed: readonly QuestionDto[],
  seen: readonly QuestionDto[] = listed,
  through = "",
): FormQuestion[] {
  const asked = new Map<string, FormQuestion>()
  const closed = new Map<string, FormClosed>()
  for (const event of events) {
    const payload = record(event.payload)
    if (event.kind === "question") {
      const question = record(payload?.question)
      const id = text(question?.id)
      if (!question || id === undefined) continue
      asked.set(id, {
        id,
        message: text(question.message) ?? "",
        fields: formFields(question.schema),
        createdAt: text(question.created_at) ?? event.created_at,
        eventId: event.id,
      })
    } else if (event.kind === "question.resolved") {
      const id = text(payload?.question_id)
      if (id !== undefined) closed.set(id, closedOf(payload))
    }
  }
  const open = new Set(listed.map((dto) => dto.id))
  const older = new Map<string, FormQuestion>()
  for (const dto of [...seen, ...listed]) if (!asked.has(dto.id)) older.set(dto.id, questionOf(dto))
  for (const [id, close] of closed) {
    if (asked.has(id) || older.has(id)) continue
    older.set(id, {
      id,
      message: "",
      fields: Object.keys(close.content).map((key) => ({
        key,
        title: key,
        required: false,
        kind: { type: "text" },
      })),
      createdAt: "",
      orphan: true,
    })
  }
  const beforeEvents = [...older.values()].map((question) => {
    const close = closed.get(question.id)
    if (close) return { ...question, closed: close }
    if (open.has(question.id)) return question
    return { ...question, closed: GONE, lost: true as const }
  })
  const fromEvents = [...asked.values()].map((question) => {
    const close =
      closed.get(question.id) ??
      (question.eventId !== undefined && question.eventId <= through && !open.has(question.id)
        ? GONE
        : undefined)
    return close ? { ...question, closed: close } : question
  })
  return [...beforeEvents, ...fromEvents]
}

/** How a question closed that the console did not see close. */
const GONE: FormClosed = { action: "cancel", content: {}, closedBy: "unknown" }

/** The open questions of a session, oldest first. */
export async function listQuestions(host: Host, sessionId: string): Promise<QuestionDto[]> {
  const listed = await unwrap(
    api(host).GET("/v1/sessions/{id}/questions", { params: { path: { id: sessionId } } }),
  )
  return Array.isArray(listed) ? listed : []
}

/** What a person does with a question: send the form's content, or dismiss it. */
type FormAnswer = { action: "accept"; content: Record<string, unknown> } | { action: "decline" }

/** Post an answer to one question's own route, never the console input route. */
export async function answerQuestion(
  host: Host,
  sessionId: string,
  questionId: string,
  answer: FormAnswer,
): Promise<void> {
  await unwrap(
    api(host).POST("/v1/sessions/{id}/questions/{question_id}/answer", {
      params: { path: { id: sessionId, question_id: questionId } },
      // The schema types the daemon's free-form `content` object as an empty one.
      body: answer as never,
    }),
  )
}

/** What a field holds while the person fills it in: a choice, the choices, or the text typed. */
export type FieldValue = FormChoice["value"] | FormChoice["value"][] | string | undefined

/** The text of each choice `value` names, as the agent wrote it. */
export function answerText(field: FormField, value: unknown): string {
  const choices =
    field.kind.type === "single" || field.kind.type === "multi" ? field.kind.choices : []
  const one = (entry: unknown) =>
    choices.find((choice) => choice.value === entry)?.title ?? String(entry)
  if (Array.isArray(value)) return value.length === 0 ? "(none)" : value.map(one).join(", ")
  return one(value)
}

/**
 * The content a form holds, or why it cannot be sent yet. A field the person
 * left empty is left out, and an empty required one stops the send.
 */
export function formContent(
  fields: readonly FormField[],
  values: Readonly<Record<string, FieldValue>>,
): { content: Record<string, unknown> } | { problem: string } {
  const content: Record<string, unknown> = {}
  for (const field of fields) {
    const value = values[field.key]
    let given: unknown
    if (field.kind.type === "number") {
      const typed = typeof value === "string" ? value.trim() : ""
      if (typed !== "") {
        const number = Number(typed)
        if (!Number.isFinite(number) || (field.kind.integer && !Number.isInteger(number)))
          return { problem: `${field.title} takes ${field.kind.integer ? "a whole " : "a "}number` }
        given = number
      }
    } else if (field.kind.type === "text") {
      if (typeof value === "string" && value.trim() !== "") given = value
    } else if (field.kind.type === "multi") {
      if (Array.isArray(value) && value.length > 0) given = value
    } else if (value !== undefined && !Array.isArray(value)) given = value
    if (given === undefined) {
      if (field.required) return { problem: `${field.title} needs an answer` }
      continue
    }
    content[field.key] = given
  }
  return { content }
}

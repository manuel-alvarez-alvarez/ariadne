/**
 * The card a question an agent asked with its own tool is drawn as (021): the
 * agent's message, each field of its form, and Send and Dismiss. A single
 * select is a radio group, a multi select a checkbox group, free text and a
 * number an input. Send posts `accept` with the content, and Dismiss posts
 * `decline`, to this question's own route. A failed post shows on the card,
 * which stays answerable. An accepted one leaves the card waiting for the
 * question's `question.resolved` event, which closes it for good, so a
 * reload, a reconnect and a replay draw the closed card the same way.
 */

import { CircleHelpIcon, Loader2Icon } from "lucide-react"
import { type FormEvent, useId, useState } from "react"

import type { Host } from "@/api"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/format"

import {
  answerQuestion,
  answerText,
  type FieldValue,
  type FormChoice,
  type FormClosed,
  type FormField,
  type FormQuestion,
  formContent,
} from "./form-questions"

type Submitting = "idle" | "accept" | "decline" | "sent"

export function FormCard({
  host,
  sessionId,
  question,
  live,
  onSent,
}: {
  host: Host
  sessionId: string
  question: FormQuestion
  /** The session takes input: a card of a finished session is history. */
  live: boolean
  /** Told when this card's own answer is accepted. */
  onSent?: (questionId: string) => void
}) {
  const [values, setValues] = useState<Record<string, FieldValue>>({})
  const [submitting, setSubmitting] = useState<Submitting>("idle")
  const [error, setError] = useState<string | null>(null)
  const { closed, fields } = question
  const answerable = live && closed === undefined && submitting === "idle"
  const built = formContent(fields, values)
  const group = useId()

  async function send(answer: Parameters<typeof answerQuestion>[3], action: "accept" | "decline") {
    setSubmitting(action)
    setError(null)
    try {
      await answerQuestion(host, sessionId, question.id, answer)
      setSubmitting("sent")
      onSent?.(question.id)
    } catch (failure) {
      setError(`Could not send the answer: ${describeError(failure)}`)
      setSubmitting("idle")
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!answerable || "problem" in built) return
    void send({ action: "accept", content: built.content }, "accept")
  }

  const setValue = (key: string, value: FieldValue) =>
    setValues((now) => ({ ...now, [key]: value }))

  return (
    <section
      aria-label="Agent question"
      data-form-question={question.id}
      className="relative rounded-md bg-muted/30 py-2 pr-2 pl-7 text-xs"
    >
      <CircleHelpIcon aria-hidden className="absolute top-2.5 left-1.5 size-3.5 text-purple-500" />
      {question.message === "" ? null : (
        <p className="whitespace-pre-wrap font-medium text-foreground">{question.message}</p>
      )}
      {closed ? (
        <ClosedAnswers closed={closed} fields={fields} />
      ) : (
        <form onSubmit={submit} className="flex flex-col gap-2 pt-1.5">
          {fields.map((field, at) => (
            <FieldInput
              key={field.key}
              field={field}
              name={`${group}-${at}`}
              value={values[field.key]}
              disabled={!answerable}
              onChange={(value) => setValue(field.key, value)}
            />
          ))}
          {live && submitting !== "sent" ? (
            <div className="flex gap-1.5">
              <Button type="submit" size="sm" disabled={!answerable || "problem" in built}>
                {submitting === "accept" ? (
                  <Loader2Icon className="size-3 animate-spin" aria-hidden />
                ) : null}
                Send
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={!answerable}
                onClick={() => void send({ action: "decline" }, "decline")}
              >
                {submitting === "decline" ? (
                  <Loader2Icon className="size-3 animate-spin" aria-hidden />
                ) : null}
                Dismiss
              </Button>
              {"problem" in built && answerable ? (
                <span className="self-center text-muted-foreground">{built.problem}</span>
              ) : null}
            </div>
          ) : null}
          {error ? (
            <p role="alert" className="text-status-danger-fg">
              {error}
            </p>
          ) : null}
          {submitting === "sent" ? (
            <p className="font-mono text-muted-foreground">↳ Answer sent</p>
          ) : live ? null : (
            <p className="font-mono text-muted-foreground">↳ Not answered: the session ended</p>
          )}
        </form>
      )}
    </section>
  )
}

/** How a closed question ended: each answer under its field's title, or the way it closed without one. */
function ClosedAnswers({ closed, fields }: { closed: FormClosed; fields: readonly FormField[] }) {
  if (closed.action !== "accept") {
    return (
      <p className="pt-1 font-mono text-muted-foreground">
        ↳{" "}
        {closed.action === "decline"
          ? "Dismissed"
          : closed.closedBy === "agent"
            ? "Withdrawn by the agent"
            : closed.closedBy === "session"
              ? "Closed: the session ended"
              : "Cancelled"}
      </p>
    )
  }
  const answered = fields.filter((field) => field.key in closed.content)
  return (
    <div className="pt-1 font-mono text-muted-foreground">
      <p>↳ Answered</p>
      {answered.map((field) => (
        <p key={field.key} className="pl-3">
          {field.title} → {answerText(field, closed.content[field.key])}
        </p>
      ))}
    </div>
  )
}

function FieldInput({
  field,
  name,
  value,
  disabled,
  onChange,
}: {
  field: FormField
  name: string
  value: FieldValue
  disabled: boolean
  onChange: (value: FieldValue) => void
}) {
  const { kind } = field
  const label = (
    <>
      <span className="font-medium text-foreground">{field.title}</span>
      {field.required ? null : <span className="text-muted-foreground"> (optional)</span>}
      {field.description ? (
        <span className="block text-muted-foreground">{field.description}</span>
      ) : null}
    </>
  )
  if (kind.type === "text" || kind.type === "number") {
    return (
      <label className="flex flex-col gap-0.5">
        <span>{label}</span>
        <input
          type="text"
          inputMode={kind.type === "number" ? "decimal" : undefined}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.target.value)}
          disabled={disabled}
          className="min-w-0 rounded-md border border-input bg-transparent px-2 py-1 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
        />
      </label>
    )
  }
  const multi = kind.type === "multi"
  const chosen = Array.isArray(value) ? value : []
  function toggle(choice: FormChoice, on: boolean) {
    onChange(on ? [...chosen, choice.value] : chosen.filter((one) => one !== choice.value))
  }
  return (
    <fieldset className="flex min-w-0 flex-col gap-1" disabled={disabled}>
      <legend className="pb-0.5">{label}</legend>
      {kind.choices.map((choice) => (
        <label key={String(choice.value)} className="flex items-start gap-2">
          <input
            type={multi ? "checkbox" : "radio"}
            name={name}
            checked={multi ? chosen.includes(choice.value) : value === choice.value}
            onChange={(event) =>
              multi ? toggle(choice, event.target.checked) : onChange(choice.value)
            }
            className="mt-0.5 accent-primary"
          />
          <span>
            <span className="text-foreground">{choice.title}</span>
            {choice.description ? (
              <span className="block text-muted-foreground">{choice.description}</span>
            ) : null}
          </span>
        </label>
      ))}
    </fieldset>
  )
}

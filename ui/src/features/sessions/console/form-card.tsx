import { CircleHelpIcon } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { Host } from "@/api"
import { Markdown } from "@/components/markdown"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { describeError, formatClock } from "@/lib/format"
import {
  answerQuestion,
  answerText,
  choiceLabel,
  type FieldValue,
  type FormClosed,
  type FormQuestion,
  formContent,
  questionSteps,
} from "./form-questions"

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
  live: boolean
  onSent?: (id: string) => void
}) {
  const steps = questionSteps(question.fields)
  const [at, setAt] = useState(0)
  const [values, setValues] = useState<Record<string, FieldValue>>({})
  const [pending, setPending] = useState(false)
  const [sent, setSent] = useState<FormClosed>()
  const [expanded, setExpanded] = useState(false)
  const [error, setError] = useState<string>()
  const root = useRef<HTMLElement>(null)
  const closed = question.closed ?? sent
  const readonly = !!closed || !live
  const step = steps[at]
  const field = step?.field
  const kind = field?.kind
  const choices = kind?.type === "single" || kind?.type === "multi" ? kind.choices : []
  const multi = kind?.type === "multi"
  useEffect(() => {
    if (!readonly) root.current?.focus()
  }, [readonly])
  function move(index: number) {
    setAt(index)
    if (!readonly) root.current?.focus()
  }
  async function send(action: "accept" | "decline", content: Record<string, unknown> = {}) {
    if (pending || readonly) return
    setPending(true)
    setError(undefined)
    try {
      await answerQuestion(
        host,
        sessionId,
        question.id,
        action === "accept" ? { action, content } : { action },
      )
      setSent({ action, content })
      onSent?.(question.id)
    } catch (failure) {
      setError(`Could not send the answer: ${describeError(failure)}`)
    } finally {
      setPending(false)
    }
  }
  function advance(next = values) {
    if (!field || readonly || pending) return
    const built = formContent(
      question.fields.map((f) => ({ ...f, required: false })),
      next,
    )
    if ("problem" in built) {
      setError(built.problem)
      return
    }
    setValues(next)
    setError(undefined)
    if (at < steps.length - 1) move(at + 1)
    else void send("accept", built.content)
  }
  function pick(index: number) {
    const choice = choices[index]
    if (!choice || !field || pending || readonly) {
      if (index === choices.length) root.current?.querySelector<HTMLInputElement>("input")?.focus()
      return
    }
    const old = values[field.key]
    const selected = Array.isArray(old) ? old : []
    const next = {
      ...values,
      [field.key]: multi
        ? selected.includes(choice.value)
          ? selected.filter((v) => v !== choice.value)
          : [...selected, choice.value]
        : choice.value,
    }
    if (multi) setValues(next)
    else {
      if (step?.custom) delete next[step.custom.key]
      advance(next)
    }
  }
  function summary() {
    if (!closed) return "Closed: the session ended"
    if (closed.action !== "accept")
      return closed.action === "decline"
        ? "Dismissed"
        : closed.closedBy === "agent"
          ? "Withdrawn by the agent"
          : closed.closedBy === "session"
            ? "Closed: the session ended"
            : closed.closedBy === "unknown"
              ? "Closed"
              : "Cancelled"
    return steps.map(({ field: f, custom }) => {
      const answer = [
        closed.content[f.key] === undefined
          ? undefined
          : choiceLabel(answerText(f, closed.content[f.key])),
        custom ? closed.content[custom.key] : undefined,
      ]
        .filter(Boolean)
        .join(", ")
      return (
        <span className="block" key={f.key}>
          {f.title === f.key ? question.message : f.title} → {answer || "Skipped"}
        </span>
      )
    })
  }
  const content = closed?.content ?? values
  return (
    <section
      ref={root}
      // biome-ignore lint/a11y/noNoninteractiveTabindex: The card owns scoped question shortcuts.
      tabIndex={0}
      aria-label="Agent question"
      data-form-question={question.id}
      className="relative rounded-md bg-muted/30 p-3 pl-8 font-sans text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onKeyDown={(event) => {
        if (readonly || pending || (event.target as HTMLElement).closest("input, textarea")) return
        if (/^[1-9]$/.test(event.key)) {
          event.preventDefault()
          pick(Number(event.key) - 1)
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault()
          const rows = Array.from(
            root.current?.querySelectorAll<HTMLElement>("[data-choice], input") ?? [],
          )
          const index = rows.indexOf(document.activeElement as HTMLElement)
          rows[
            (index + (event.key === "ArrowDown" ? 1 : rows.length - 1) + rows.length) % rows.length
          ]?.focus()
        }
        if (event.key === "Backspace" || event.key === "BrowserBack" || event.key === "Back") {
          event.preventDefault()
          move(Math.max(0, at - 1))
        }
        if (event.key === "Enter" && event.target === root.current) {
          event.preventDefault()
          if (choices.length && !multi) pick(0)
          else advance()
        }
      }}
    >
      <CircleHelpIcon aria-hidden className="absolute top-3 left-2 size-4 text-purple-500" />
      {readonly && !expanded ? (
        <button
          type="button"
          className="w-full text-left"
          aria-expanded={false}
          onClick={() => setExpanded(true)}
        >
          {summary()}
        </button>
      ) : (
        <>
          <header className="flex items-center gap-3 pb-2">
            <strong>{field && field.title !== field.key ? field.title : "Question"}</strong>
            {steps.length > 1 ? (
              <span>
                {at + 1} of {steps.length}
              </span>
            ) : null}
            <time className="ml-auto text-muted-foreground">{formatClock(question.createdAt)}</time>
          </header>
          {steps.length > 1 ? (
            <nav aria-label="Questions" className="mb-3 flex gap-2">
              {steps.map(({ field: f, custom }, index) => (
                <Button
                  key={f.key}
                  variant={
                    content[f.key] !== undefined || (custom && content[custom.key] !== undefined)
                      ? "default"
                      : "outline"
                  }
                  aria-label={`Question ${index + 1}${content[f.key] !== undefined || (custom && content[custom.key] !== undefined) ? ", answered" : ""}`}
                  onClick={() => move(index)}
                >
                  {index + 1}
                </Button>
              ))}
            </nav>
          ) : null}
          <Markdown>
            {steps.length === 1
              ? [question.message, field?.description].filter(Boolean).join("\n\n")
              : (field?.description ?? question.message)}
          </Markdown>
          <form
            className="mt-3 flex flex-col gap-2"
            onSubmit={(event) => {
              event.preventDefault()
              advance()
            }}
          >
            {choices.map((choice, index) => {
              const selected = multi
                ? Array.isArray(content[field?.key ?? ""]) &&
                  (content[field?.key ?? ""] as unknown[]).includes(choice.value)
                : content[field?.key ?? ""] === choice.value
              const recommended = choiceLabel(choice.title) !== choice.title
              return (
                <Button
                  data-choice
                  role={multi ? "checkbox" : undefined}
                  aria-checked={multi ? selected : undefined}
                  key={String(choice.value)}
                  type="button"
                  variant={selected || recommended ? "default" : "outline"}
                  disabled={readonly || pending}
                  aria-pressed={selected}
                  onClick={() => pick(index)}
                  className="h-auto w-full justify-start whitespace-normal px-3 py-3 text-left text-sm"
                >
                  <span>{multi ? (selected ? "☑" : "☐") : index + 1}</span>
                  <span className="flex flex-col gap-1">
                    <span>
                      {choiceLabel(choice.title)}{" "}
                      {recommended ? <Badge className="text-sm">Recommended</Badge> : null}
                    </span>
                    {choice.description ? (
                      <span className="font-normal opacity-80">{choice.description}</span>
                    ) : null}
                  </span>
                </Button>
              )
            })}
            {field && (choices.length === 0 || step.custom) ? (
              <Input
                aria-label={choices.length ? "Type your own answer…" : field.title}
                placeholder={choices.length ? "Type your own answer…" : "Type your answer"}
                disabled={readonly || pending}
                inputMode={kind?.type === "number" ? "decimal" : undefined}
                value={String(content[step.custom?.key ?? field.key] ?? "")}
                onChange={(event) =>
                  setValues({ ...values, [step.custom?.key ?? field.key]: event.target.value })
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault()
                    const next = { ...values }
                    if (!multi && step.custom) delete next[field.key]
                    advance(next)
                  }
                }}
              />
            ) : null}
            {!readonly ? (
              <div className="flex gap-2 pt-2">
                {at > 0 ? (
                  <Button type="button" variant="outline" onClick={() => move(at - 1)}>
                    Back
                  </Button>
                ) : null}
                {multi || choices.length === 0 ? (
                  <Button type="submit" disabled={pending}>
                    {at === steps.length - 1 ? "Send" : "Next"}
                  </Button>
                ) : null}
                <Button
                  type="button"
                  variant="ghost"
                  disabled={pending}
                  onClick={() => {
                    const next = { ...values }
                    if (field) delete next[field.key]
                    if (step?.custom) delete next[step.custom.key]
                    advance(next)
                  }}
                >
                  Skip
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  disabled={pending}
                  onClick={() => void send("decline")}
                >
                  Dismiss
                </Button>
              </div>
            ) : (
              <Button type="button" variant="ghost" onClick={() => setExpanded(false)}>
                Collapse
              </Button>
            )}
          </form>
        </>
      )}
      {error ? <p role="alert">{error}</p> : null}
    </section>
  )
}

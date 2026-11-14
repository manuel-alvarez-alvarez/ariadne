/**
 * The card a question an agent asked through `wait_for_user` is drawn as
 * (031): the question, one button per option, the recommendation, and a
 * field for an answer of the user's own.
 *
 * Every answer goes to `POST /v1/sessions/{id}/agent-requests/{request_id}/answer`
 * for this card's own request, never to the console input route, so it can
 * never be read as a permission reply. A click on an option submits at
 * once. A failed submission is shown on the card, which stays answerable
 * for another attempt. An accepted one leaves the card waiting for the
 * request's `agent_request.resolved` event, which closes it for good.
 */

import { Loader2Icon } from "lucide-react"
import { type FormEvent, useState } from "react"

import { type AgentRequestDto, api, unwrap } from "@/api"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/format"

import type { Block, QuestionResolution, Transcript } from "./transcript"

type QuestionBlock = Extract<Block, { type: "question" }>

/**
 * The requests of a session that still wait for an answer, read with each
 * snapshot: a console opens on the newest page of events, and a question
 * asked before it has no opening line there.
 */
export async function pendingRequests(sessionId: string): Promise<AgentRequestDto[]> {
  const listed = await unwrap(
    api().GET("/v1/sessions/{id}/agent-requests", { params: { path: { id: sessionId } } }),
  )
  return Array.isArray(listed) ? listed : []
}

/** The card of each request read, so a request read once keeps one block and its card does not render again. */
const cards = new WeakMap<AgentRequestDto, QuestionBlock>()

/**
 * The cards to restore: each pending question the transcript neither shows
 * nor has seen closed since the list was read. An older page that brings its
 * opening line in draws it in place, with every close that followed it.
 */
export function restoredQuestions(
  requests: readonly AgentRequestDto[],
  transcript: Transcript,
): QuestionBlock[] {
  const shown = new Set(
    transcript.blocks.flatMap((block) => (block.type === "question" ? [block.requestId] : [])),
  )
  return requests.flatMap((request) => {
    const { question } = request
    if (!question || shown.has(request.id) || transcript.closedRequests.has(request.id)) return []
    let card = cards.get(request)
    if (!card) {
      card = {
        id: `pending:${request.id}`,
        createdAt: request.created_at,
        type: "question",
        requestId: request.id,
        hint: request.summary,
        question,
      }
      cards.set(request, card)
    }
    return [card]
  })
}

/**
 * Post an answer for one request: an option's id or the user's own text. The
 * card and the composer both answer through this, so neither can reach the
 * console input route.
 */
export async function answerRequest(
  sessionId: string,
  requestId: string,
  body: { option_id: string } | { text: string },
): Promise<void> {
  await unwrap(
    api().POST("/v1/sessions/{session_id}/agent-requests/{request_id}/answer", {
      params: { path: { session_id: sessionId, request_id: requestId } },
      body,
    }),
  )
}

/**
 * The question the composer answers: the newest one in view that nothing has
 * closed and nobody has answered yet.
 */
export function openQuestion(
  blocks: readonly Block[],
  answered: ReadonlySet<string>,
): QuestionBlock | undefined {
  return blocks.findLast(
    (block): block is QuestionBlock =>
      block.type === "question" && block.resolution === undefined && !answered.has(block.requestId),
  )
}

/** What the card is doing with an answer: the option it posts, or `custom`. */
type Submitting = { state: "idle" } | { state: "posting"; what: string } | { state: "accepted" }

export function QuestionCard({
  sessionId,
  block,
  live,
  sent = false,
  onAnswered,
}: {
  sessionId: string
  block: QuestionBlock
  /** The session takes input: a card of a finished session is history. */
  live: boolean
  /** An answer for this request was accepted elsewhere, the composer's say. */
  sent?: boolean
  /** Told when this card's own answer is accepted. */
  onAnswered?: (requestId: string) => void
}) {
  const [submitting, setSubmitting] = useState<Submitting>({ state: "idle" })
  const [error, setError] = useState<string | null>(null)
  const [custom, setCustom] = useState("")
  const { question, resolution } = block
  const options = question.options ?? []
  const recommended = options.find((option) => option.id === question.recommended_option_id)
  const answerable = live && resolution === undefined && !sent && submitting.state === "idle"

  async function submit(what: string, body: { option_id: string } | { text: string }) {
    setSubmitting({ state: "posting", what })
    setError(null)
    try {
      await answerRequest(sessionId, block.requestId, body)
      setSubmitting({ state: "accepted" })
      onAnswered?.(block.requestId)
    } catch (failure) {
      setError(`Could not send the answer: ${describeError(failure)}`)
      setSubmitting({ state: "idle" })
    }
  }

  function submitCustom(event: FormEvent) {
    event.preventDefault()
    const text = custom.trim()
    if (text === "" || !answerable) return
    void submit("custom", { text })
  }

  return (
    <section
      aria-label={`Question · ${block.hint}`}
      className="rounded-md border border-status-active/40 bg-status-active-soft/30 px-3 py-2 text-xs"
    >
      <div className="font-medium">Question · {block.hint}</div>
      <p className="whitespace-pre-wrap break-words pt-1 text-sm">{question.text}</p>
      {recommended ? (
        <p className="pt-1 text-muted-foreground">Recommended: {recommended.label}</p>
      ) : null}
      {options.length > 0 ? (
        <div className="flex flex-col gap-1 pt-2">
          {options.map((option) => (
            <Button
              key={option.id}
              type="button"
              variant={option.id === recommended?.id ? "default" : "outline"}
              disabled={!answerable}
              onClick={() => void submit(option.id, { option_id: option.id })}
              className="h-auto justify-start whitespace-normal py-1.5 text-left"
            >
              <span className="flex flex-col items-start">
                <span className="flex items-center gap-1.5 font-medium">
                  {option.label}
                  {option.id === recommended?.id ? (
                    <span className="text-[10px] uppercase opacity-80">recommended</span>
                  ) : null}
                  {submitting.state === "posting" && submitting.what === option.id ? (
                    <Loader2Icon className="size-3 animate-spin" aria-hidden />
                  ) : null}
                </span>
                {option.description ? (
                  <span className="font-normal opacity-80">{option.description}</span>
                ) : null}
              </span>
            </Button>
          ))}
        </div>
      ) : null}
      {resolution === undefined && live && !sent ? (
        <form onSubmit={submitCustom} className="flex gap-1.5 pt-2">
          <input
            aria-label="Your own answer"
            value={custom}
            onChange={(event) => setCustom(event.target.value)}
            disabled={!answerable}
            placeholder={options.length > 0 ? "Or type your own answer" : "Type your answer"}
            className="min-w-0 flex-1 rounded-md border border-input bg-transparent px-2 py-1 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
          />
          <Button type="submit" size="sm" disabled={!answerable || custom.trim() === ""}>
            {submitting.state === "posting" && submitting.what === "custom" ? (
              <Loader2Icon className="size-3 animate-spin" aria-hidden />
            ) : null}
            Answer
          </Button>
        </form>
      ) : null}
      {error ? (
        <p role="alert" className="pt-1.5 text-status-danger-fg">
          {error}
        </p>
      ) : null}
      <Outcome
        resolution={resolution}
        accepted={sent || submitting.state === "accepted"}
        live={live}
      />
    </section>
  )
}

function Outcome({
  resolution,
  accepted,
  live,
}: {
  resolution: QuestionResolution | undefined
  accepted: boolean
  live: boolean
}) {
  const text =
    resolution?.state === "answered"
      ? `Answered${resolution.answer ? ` · ${resolution.answer}` : ""}`
      : resolution?.state === "withdrawn"
        ? "Withdrawn by the agent"
        : resolution?.state === "expired"
          ? "Expired: the turn that asked it ended"
          : accepted
            ? "Answer sent"
            : live
              ? undefined
              : "Not answered: the session ended"
  if (text === undefined) return null
  return (
    <div className="pt-1.5 font-mono text-muted-foreground">
      <span aria-hidden>↳ </span>
      {text}
    </div>
  )
}

/**
 * A session's console as blocks: the events of the console stream folded the
 * way the CLI's transcript folds them (`crates/ariadne-console/src/transcript.rs`,
 * and the live `absorb` of its inline console).
 *
 * Pure: every call answers a new transcript and leaves the one it was given
 * as it was, so a React state can hold it. A `snapshot` replaces the whole
 * (`applySnapshot`), and every later `event` extends it (`applyEvent`).
 *
 * Every block keeps the id of the event it opened on, and the child that
 * spoke where `payload.subagent` names one. `subagents` lists each child in
 * the order it was first heard of, and `view` picks the blocks of one view:
 * the session's own, `main`, or one child's (008 rules 30 to 35).
 */

import type { AgentEventDto, AgentQuestion } from "@/api"
import { languageForPath } from "@/lib/highlight"

import { elapsed } from "./elapsed"

export * from "./elapsed"

interface Subagent {
  id: string
  title: string
}

interface BlockBase {
  /** The id of the event this block opened on; `local:<n>` for a local block. */
  id: string
  /** The `created_at` of the event this block opened on; the time a local block was added. */
  createdAt: string
  subagent?: Subagent
}

interface PlanEntry {
  content: string
  status: string
}

export interface PermissionOption {
  id: string
  name: string
  /** `allow_once`, `allow_always`, `reject_once` or `reject_always`. */
  kind?: string
}

interface ToolCall {
  id: string
  title: string
  /** The ACP kind: `read`, `edit`, `execute` and the rest. */
  kind?: string
  status?: string
  input?: unknown
  output?: string
  /** The file change as a unified diff. */
  diff?: string
  /** What the call is about: the command, the path, the pattern, the URL, else the title. */
  subject?: string
  /** The files the call names, as the ACP call lists them. */
  locations?: ToolLocation[]
  /** The `created_at` of the call's first event. */
  startedAt?: string
  /** The `created_at` of the update that gave it a terminal status. */
  endedAt?: string
  /** The whole command for `execute`, else the input as JSON; none for an empty input. */
  inputText?: string
  inputLanguage?: "shell" | "json"
  /** The language of `output`: `json`, or the file's for a `read`. */
  outputLanguage?: string
  /** The number a `read` output's first line carried before its prefixes were removed. */
  firstLine?: number
}

interface ToolLocation {
  path: string
  line?: number
}

export type Block =
  | (BlockBase & {
      type: "prompt"
      text: string
      /** Sent by the daemon (`source: "daemon"`), not typed by a person. */
      daemon: boolean
    })
  | (BlockBase & {
      type: "message" | "thought"
      text: string
      /** Still taking live chunks: the stored whole has not closed it. */
      open: boolean
      /** The id of the stored whole that closed it. */
      closedBy?: string
    })
  | (BlockBase & { type: "plan"; entries: PlanEntry[] })
  | (BlockBase & { type: "tool"; tool: ToolCall })
  | (BlockBase & {
      type: "permission"
      /** The call, and the child that asks where one does: `<tool> · asked by subagent <title>`. */
      title: string
      /** The child whose question this is. The session answers it, so it shows in `main`. */
      askedBy?: Subagent
      /** The command the call runs, where it runs one. */
      command?: string
      diff?: string
      /** Why the AI permission model had a part in the request, where it did (`ai_permission_note`). */
      aiNote?: string
      options: PermissionOption[]
      answer?: { optionId?: string; name: string; decidedBy?: string }
    })
  | (BlockBase & {
      type: "question"
      /** The request the question belongs to: what an answer is posted for. */
      requestId: string
      /** The one line the Needs attention list shows for it. */
      hint: string
      question: AgentQuestion
      /** How the request closed; none while it waits for an answer. */
      resolution?: QuestionResolution
    })
  | (BlockBase & { type: "note"; text: string })
  /** A line of `main` for a child's announcement or its end. */
  | (BlockBase & { type: "child"; text: string })
  | (BlockBase & { type: "error"; text: string; diagnosis?: string })
  | (BlockBase & { type: "raw"; kind: string; summary: string })

/** How a question's request closed, as its `agent_request.resolved` event says. */
export interface QuestionResolution {
  state: "answered" | "withdrawn" | "expired"
  /** The picked option's label, or the text the user typed. */
  answer?: string
}

/** One child the agent announced, as its latest `subagent_update` said. */
interface ChildAgent extends Subagent {
  /** `running`, `completed`, `failed` or `cancelled`, or none where the agent said none. */
  state?: string
  /** Whether the console may cancel this child now. */
  cancel: boolean
  /** When it was first heard of: what an end counts its time from. */
  announcedAt: string
}

/** A slash command the agent lists: an ACP `AvailableCommand`. */
export interface AgentCommand {
  /** The name, without its `/`. */
  name: string
  description: string
  /** What its argument is, as the agent says it. */
  hint?: string
}

/** A prompt the console sent that the stream has not echoed yet. */
interface QueuedPrompt {
  id: string
  text: string
  /** The newest event id the transcript held when it was sent: only a later echo matches. */
  after: string
}

export interface Transcript {
  blocks: Block[]
  /** Every child in the order it was first heard of. */
  subagents: ChildAgent[]
  /** A turn runs: a prompt or the agent's output since the last stop. */
  running: boolean
  /** The `created_at` of the running turn's first event. */
  turnStartedAt?: string
  /** The newest event id folded in. */
  newest: string
  /** The latest list of the session's slash commands, whole. */
  availableCommands: AgentCommand[]
  /** Prompts sent from this console, shown until their `user_prompt_submit` arrives. */
  queuedPrompts: QueuedPrompt[]
  /** The latest totals each launch reported on a `stop`, by launch. */
  launches: Readonly<Record<string, TokenTotals>>
  /** Every request an `agent_request.resolved` event closed, whether or not its question is shown. */
  closedRequests: ReadonlySet<string>
  /** Every event id folded in, so an event delivered twice is folded once. */
  seen: ReadonlySet<string>
  /** Every event folded in, oldest first, so an older page can be folded before them. */
  events: readonly AgentEventDto[]
}

export interface TokenTotals {
  input: number
  output: number
}

export function emptyTranscript(): Transcript {
  return {
    blocks: [],
    subagents: [],
    running: false,
    newest: "",
    availableCommands: [],
    queuedPrompts: [],
    launches: {},
    closedRequests: new Set(),
    seen: new Set(),
    events: [],
  }
}

/**
 * The transcript a `snapshot` opens on, in place of whatever was shown. A
 * prompt still queued stays, until the snapshot or a later event holds it.
 */
export function applySnapshot(transcript: Transcript, events: AgentEventDto[]): Transcript {
  const opened = { ...emptyTranscript(), queuedPrompts: transcript.queuedPrompts }
  return events.reduce(applyEvent, opened)
}

/** One more event of the stream, folded into the blocks so far. */
export function applyEvent(transcript: Transcript, event: AgentEventDto): Transcript {
  if (transcript.seen.has(event.id)) return transcript
  const seen = new Set(transcript.seen).add(event.id)
  if (event.kind === COMMANDS_KIND) {
    // A child's list is the child's, not the session's: any payload that names a `subagent`.
    if (at(event.payload, "subagent") !== undefined)
      return { ...transcript, events: [...transcript.events, event], seen }
    return {
      ...transcript,
      availableCommands: commandsOf(event),
      events: [...transcript.events, event],
      seen,
    }
  }
  const blocks = transcript.blocks.slice()
  const subagents = transcript.subagents.slice()
  if (event.kind === SUBAGENT_UPDATE) followChild(subagents, blocks, event)
  else {
    const id = childId(event)
    if (id !== undefined) childAt(subagents, event, id)
    fold(blocks, event)
  }
  const now = running(transcript.running, event.kind)
  return {
    ...transcript,
    blocks,
    subagents,
    running: now,
    newest: event.id > transcript.newest ? event.id : transcript.newest,
    turnStartedAt: now ? (transcript.turnStartedAt ?? event.created_at) : undefined,
    queuedPrompts: unqueue(transcript.queuedPrompts, event),
    launches: event.kind === "stop" ? launchesOf(transcript.launches, event) : transcript.launches,
    closedRequests: closedBy(transcript.closedRequests, event),
    seen,
    events: [...transcript.events, event],
  }
}

/** The id of the oldest event folded in, where one is; the cursor of an older page. */
export function oldestEventId(transcript: Transcript): string | undefined {
  let oldest: string | undefined
  for (const id of transcript.seen) if (oldest === undefined || id < oldest) oldest = id
  return oldest
}

/**
 * A page of events older than all the transcript holds, oldest first, folded
 * in before the events already shown, so a call or a run of text that spans
 * the page boundary folds as one block. The console's own error blocks stay.
 * The queue and the running turn are not events, so they stay as they were.
 */
export function prependEvents(transcript: Transcript, events: AgentEventDto[]): Transcript {
  const fresh = events.filter((event) => !transcript.seen.has(event.id))
  if (fresh.length === 0) return transcript
  const folded = [...fresh, ...transcript.events].reduce(applyEvent, emptyTranscript())
  const local = transcript.blocks.filter((block) => block.id.startsWith("local:"))
  return {
    ...transcript,
    blocks: [...folded.blocks, ...local],
    subagents: folded.subagents,
    seen: folded.seen,
    events: folded.events,
    availableCommands: folded.availableCommands,
    launches: folded.launches,
    closedRequests: folded.closedRequests,
  }
}

const COMMANDS_KIND = "available_commands_update"

function commandsOf(event: AgentEventDto): AgentCommand[] {
  return list(event.payload, "available_commands").flatMap((entry) => {
    const name = str(entry, "name")
    if (!name) return []
    return [
      {
        name,
        description: str(entry, "description") ?? "",
        hint: str(entry, "input", "hint"),
      },
    ]
  })
}

/** The matching echo of a prompt this console sent takes its queued block away. */
function unqueue(queued: QueuedPrompt[], event: AgentEventDto): QueuedPrompt[] {
  if (event.kind !== "user_prompt_submit" || str(event.payload, "source") !== "console") {
    return queued
  }
  // An answer's delivery prompt is not an echo of anything typed here.
  if (str(event.payload, "request_id") !== undefined) return queued
  const text = str(event.payload, "text")
  const index = queued.findIndex((prompt) => prompt.text === text && event.id > prompt.after)
  return index === -1 ? queued : queued.filter((_, at) => at !== index)
}

/** A launch's latest totals replace the ones before them. */
function launchesOf(
  launches: Readonly<Record<string, TokenTotals>>,
  event: AgentEventDto,
): Readonly<Record<string, TokenTotals>> {
  const source = str(event.payload, "ariadne_usage", "source")
  const input = at(event.payload, "ariadne_usage", "input_tokens")
  const output = at(event.payload, "ariadne_usage", "output_tokens")
  if (source === undefined || typeof input !== "number" || typeof output !== "number") {
    return launches
  }
  return { ...launches, [source]: { input, output } }
}

/**
 * What the session has spent: the sum of each launch's latest totals, or the
 * session's own usage where that is more, each count on its own.
 */
export function tokenTotals(transcript: Transcript, own?: TokenTotals): TokenTotals {
  const sum = Object.values(transcript.launches).reduce(
    (total, launch) => ({
      input: total.input + launch.input,
      output: total.output + launch.output,
    }),
    { input: 0, output: 0 },
  )
  return {
    input: Math.max(sum.input, own?.input ?? 0),
    output: Math.max(sum.output, own?.output ?? 0),
  }
}

/** The title of the tool call still open, if one is. */
export function runningTool(transcript: Transcript): string | undefined {
  const last = transcript.blocks.findLast((block) => block.type === "tool")
  return last?.type === "tool" && !isTerminal(last.tool.status) ? last.tool.title : undefined
}

/** Show a prompt at once, before the stream echoes it; `id` names this send alone. */
export function queuePrompt(transcript: Transcript, text: string, id: string): Transcript {
  const after = transcript.newest
  return { ...transcript, queuedPrompts: [...transcript.queuedPrompts, { id, text, after }] }
}

/**
 * Take a queued prompt back by the id of its send: that send failed. Where an
 * echo of the same text already took its entry, that echo was another send's,
 * so the oldest entry of that text left is the one that is stale.
 */
export function dequeuePrompt(transcript: Transcript, id: string, text: string): Transcript {
  const queued = transcript.queuedPrompts
  const found = queued.findIndex((prompt) => prompt.id === id)
  const index = found === -1 ? queued.findIndex((prompt) => prompt.text === text) : found
  if (index === -1) return transcript
  return { ...transcript, queuedPrompts: queued.filter((_, at) => at !== index) }
}

/**
 * An error the console itself met, such as a send the daemon refused, in the
 * view of the child it is about, or in `main`.
 */
export function appendError(transcript: Transcript, text: string, child?: Subagent): Transcript {
  // No event opened it, so its id is local, and unique among the blocks.
  const id = `local:${transcript.blocks.length}`
  const block: Block = {
    type: "error",
    id,
    createdAt: new Date().toISOString(),
    text,
    ...(child ? { subagent: child } : {}),
  }
  return { ...transcript, blocks: [...transcript.blocks, block] }
}

/**
 * The blocks of one view: a child's own, by its id, or with none the
 * session's own, `main`, which holds no child's events but its questions.
 */
export function view(transcript: Transcript, child?: string): Block[] {
  return transcript.blocks.filter((block) => block.subagent?.id === child)
}

/** The mark of a child's state, in the bar and in the lines of `main`. */
export function stateMark(state: string | undefined): string {
  switch (state) {
    case "running":
      return "●"
    case "completed":
      return "✓"
    case "failed":
      return "✗"
    case "cancelled":
      return "⊘"
    default:
      return "◇"
  }
}

const SUBAGENT_UPDATE = "subagent_update"
const ENDED_STATES = new Set(["completed", "failed", "cancelled"])

/**
 * Fold one `subagent_update` into the children, and into `main` the line it
 * is worth there: one for an announcement, with the title and the task, and
 * one for an end, with the time since the announcement. A first update that
 * already ends the child is both, and gets both lines.
 */
function followChild(subagents: ChildAgent[], blocks: Block[], event: AgentEventDto): void {
  const id = childId(event)
  if (id === undefined) return
  const known = subagents.some((child) => child.id === id)
  const index = childAt(subagents, event, id)
  const was = subagents[index] as ChildAgent
  const state = str(event.payload, "state")
  const child: ChildAgent = {
    ...was,
    title: titleOf(event, id),
    state,
    cancel: at(event.payload, "capabilities", "cancel") === true,
  }
  subagents[index] = child
  if (!known) {
    const task = str(event.payload, "subagent", "description")
    const text = `${stateMark(undefined)} subagent ${child.title}`
    blocks.push({
      id: event.id,
      createdAt: event.created_at,
      type: "child",
      text: task ? `${text} · ${task}` : text,
    })
  }
  if (state !== was.state && state !== undefined && ENDED_STATES.has(state)) {
    const text = `${stateMark(state)} subagent ${child.title} ${state}`
    const took = elapsed(was.announcedAt, event.created_at)
    blocks.push({
      id: `${event.id}:end`,
      createdAt: event.created_at,
      type: "child",
      text: took === undefined ? text : `${text} · ${took}`,
    })
  }
}

/** The place of the child `id` in `subagents`, adding it where it is new. */
function childAt(subagents: ChildAgent[], event: AgentEventDto, id: string): number {
  const at = subagents.findIndex((child) => child.id === id)
  if (at !== -1) return at
  subagents.push({ id, title: titleOf(event, id), cancel: false, announcedAt: event.created_at })
  return subagents.length - 1
}

function titleOf(event: AgentEventDto, id: string): string {
  return str(event.payload, "subagent", "title") || id
}

/** The child an event's payload names, where it names one. */
function childId(event: AgentEventDto): string | undefined {
  return str(event.payload, "subagent", "id")
}

/** A question and its reply are the session's, whoever asks: they show in `main`. */
function askedOfTheSession(kind: string): boolean {
  return kind === "permission_request" || kind === "permission.replied"
}

/** The newest permission nobody has answered, if one waits. */
export function pendingPermission(
  transcript: Transcript,
): Extract<Block, { type: "permission" }> | undefined {
  const last = transcript.blocks.findLast((block) => block.type === "permission")
  return last?.type === "permission" && last.answer === undefined ? last : undefined
}

const TURN_KINDS = new Set([
  "user_prompt_submit",
  "agent_message_chunk",
  "agent_thought_chunk",
  "pre_tool_use",
  "tool_call_update",
  "permission_request",
])

const ENDING_KINDS = new Set(["stop", "session_end", "session.error"])

function running(was: boolean, kind: string): boolean {
  if (ENDING_KINDS.has(kind)) return false
  return was || TURN_KINDS.has(kind)
}

const END_TURN = "end_turn"
const UNRECORDED_PROMPT = "(prompt text not recorded)"
const UNNAMED_TOOL = "ACP tool"

function fold(blocks: Block[], event: AgentEventDto): void {
  const payload = event.payload
  // A turn that ended the way turns end says nothing a reader needs.
  if (event.kind === "stop" && str(payload, "stop_reason") === END_TURN) return

  // The prompt that carries an answer to the agent: the question's own card
  // shows the answer, so this one draws nothing.
  if (event.kind === "user_prompt_submit" && str(payload, "request_id") !== undefined) return

  switch (event.kind) {
    case "agent_message_chunk":
    case "agent_thought_chunk":
      foldChunk(blocks, event)
      return
    case "agent_message":
    case "agent_thought":
      foldWhole(blocks, event)
      return
    case "permission.replied":
      foldReply(blocks, event)
      return
    case RESOLVED_KIND:
      foldResolution(blocks, event)
      return
    case "session.diagnosis":
      foldDiagnosis(blocks, event)
      return
    case "usage_update":
      return
    case "tool_call_update":
    case "post_tool_use":
      if (mergeTool(blocks, event)) return
  }
  blocks.push(blockOf(event))
}

function blockOf(event: AgentEventDto): Block {
  const payload = event.payload
  const base = baseOf(event)
  switch (event.kind) {
    case "user_prompt_submit":
      return {
        ...base,
        type: "prompt",
        text: str(payload, "text") ?? UNRECORDED_PROMPT,
        daemon: str(payload, "source") === "daemon",
      }
    case "agent_message":
    case "agent_message_chunk":
      return { ...base, type: "message", text: textOf(event), open: false }
    case "agent_thought":
    case "agent_thought_chunk":
      return { ...base, type: "thought", text: textOf(event), open: false }
    case "plan":
      return {
        ...base,
        type: "plan",
        entries: list(payload, "entries").map((entry) => ({
          content: str(entry, "content") ?? "",
          status: str(entry, "status") ?? "pending",
        })),
      }
    case "pre_tool_use":
    case "post_tool_use":
    case "tool_call_update":
      return { ...base, type: "tool", tool: openTool(event) }
    case "permission_request": {
      const tool = toolOf(payload, event.summary)
      const asker = childOf(event)
      return {
        ...base,
        type: "permission",
        title: asker ? `${tool.title} · asked by subagent ${asker.title}` : tool.title,
        ...(asker ? { askedBy: asker } : {}),
        command: commandOf(tool.input),
        diff: tool.diff,
        ...(aiNote(payload) ? { aiNote: aiNote(payload) } : {}),
        options: list(payload, "options").flatMap((option) => {
          const id = str(option, "optionId")
          if (id === undefined) return []
          return [{ id, name: str(option, "name") ?? id, kind: str(option, "kind") }]
        }),
      }
    }
    case "session.error":
      return { ...base, type: "error", text: errorText(event) }
    case "session_start":
      return { ...base, type: "note", text: "session started" }
    case "session_end":
      return { ...base, type: "note", text: "session ended" }
    case "stop":
      return { ...base, type: "note", text: stopNote(str(payload, "stop_reason")) }
    case "session.switched":
    case "session.pin_fallback":
      return { ...base, type: "note", text: event.summary }
    case "compaction_update":
      return { ...base, type: "note", text: "context compacted" }
    case "agent_request": {
      const hint = str(payload, "hint")
      const question = questionOf(at(payload, "question"))
      if (question)
        return {
          ...base,
          type: "question",
          requestId: str(payload, "request_id") ?? event.id,
          hint: hint ?? "",
          question,
        }
      return { ...base, type: "note", text: hint ? `waiting for you · ${hint}` : "waiting for you" }
    }
    case "permission.replied":
      return {
        ...base,
        type: "note",
        text: `permission answered: ${str(payload, "option_id") ?? "cancelled"}`,
      }
    default:
      return { ...base, type: "raw", kind: event.kind, summary: event.summary }
  }
}

const RESOLVED_KIND = "agent_request.resolved"

function closedBy(closed: ReadonlySet<string>, event: AgentEventDto): ReadonlySet<string> {
  const requestId = event.kind === RESOLVED_KIND ? str(event.payload, "request_id") : undefined
  return requestId === undefined ? closed : new Set(closed).add(requestId)
}

/** A question as the daemon recorded it, where the payload holds one. */
function questionOf(value: unknown): AgentQuestion | undefined {
  const text = str(value, "text")
  if (text === undefined) return undefined
  const options = list(value, "options").flatMap((option) => {
    const id = str(option, "id")
    const label = str(option, "label")
    if (id === undefined || label === undefined) return []
    const description = str(option, "description")
    return [description === undefined ? { id, label } : { id, label, description }]
  })
  const question: AgentQuestion = { text, options }
  const recommended = str(value, "recommended_option_id")
  if (recommended !== undefined) question.recommended_option_id = recommended
  const reason = str(value, "recommendation_reason")
  if (reason !== undefined) question.recommendation_reason = reason
  return question
}

/**
 * A request that closed closes its question's card, and adds no block: a
 * replay or a reload draws the card closed, never answerable again.
 */
function foldResolution(blocks: Block[], event: AgentEventDto): void {
  const requestId = str(event.payload, "request_id")
  const index = blocks.findLastIndex(
    (block) => block.type === "question" && block.requestId === requestId,
  )
  const block = blocks[index]
  if (block?.type !== "question") return
  const state = str(event.payload, "resolution")
  const answer = str(event.payload, "answer", "text")
  blocks[index] = {
    ...block,
    resolution: {
      state: state === "withdrawn" || state === "expired" ? state : "answered",
      ...(answer === undefined ? {} : { answer }),
    },
  }
}

function baseOf(event: AgentEventDto): BlockBase {
  const child = askedOfTheSession(event.kind) ? undefined : childOf(event)
  const base = { id: event.id, createdAt: event.created_at }
  return child ? { ...base, subagent: child } : base
}

function childOf(event: AgentEventDto): Subagent | undefined {
  const id = childId(event)
  return id === undefined ? undefined : { id, title: titleOf(event, id) }
}

/** Whether a block was spoken by the same agent: the session, or one child. */
function sameSpeaker(block: Block, event: AgentEventDto): boolean {
  return block.subagent?.id === childId(event)
}

/**
 * A live chunk grows the last block of its own speaker — the session, or one
 * child — where that block is still open, and opens one where it is not, so
 * a child's chunks interleaved with the parent's never join the parent's
 * text. A chunk that arrives after the whole of its turn — an id below that
 * whole's, and text the whole already holds — is dropped.
 */
function foldChunk(blocks: Block[], event: AgentEventDto): void {
  const type = event.kind === "agent_thought_chunk" ? "thought" : "message"
  const said = textOf(event)
  const index = blocks.findLastIndex((block) => sameSpeaker(block, event))
  const last = blocks[index]
  if (last?.type === type && last.open) {
    blocks[index] = { ...last, text: last.text + said }
    return
  }
  if (
    last?.type === type &&
    last.closedBy !== undefined &&
    event.id < last.closedBy &&
    last.text.includes(said)
  ) {
    return
  }
  blocks.push({ ...baseOf(event), type, text: said, open: true })
}

/**
 * The stored whole of a run of text closes every block its speaker's chunks
 * opened and adds no text; only a run that streamed nothing adds a block of
 * its own. A child's whole leaves the parent's open block open.
 */
function foldWhole(blocks: Block[], event: AgentEventDto): void {
  const type = event.kind === "agent_thought" ? "thought" : "message"
  let closed = false
  blocks.forEach((block, index) => {
    if (block.type === type && block.open && sameSpeaker(block, event)) {
      blocks[index] = { ...block, open: false, closedBy: event.id }
      closed = true
    }
  })
  if (!closed)
    blocks.push({ ...baseOf(event), type, text: textOf(event), open: false, closedBy: event.id })
}

function foldReply(blocks: Block[], event: AgentEventDto): void {
  const index = blocks.findLastIndex((block) => block.type === "permission" && !block.answer)
  const block = blocks[index]
  if (block?.type !== "permission") {
    blocks.push(blockOf(event))
    return
  }
  // A permission on one of Ariadne's own tools that the daemon allowed by
  // itself asked nobody anything: the call's own block says what happened.
  if (
    block.title.toLowerCase().includes("ariadne") &&
    daemonAllowed(event.payload, block.options)
  ) {
    blocks.splice(index, 1)
    return
  }
  const optionId = str(event.payload, "option_id")
  const chosen = str(event.payload, "console_option_id") ?? optionId
  const name =
    optionId === undefined
      ? "cancelled"
      : (block.options.find((option) => option.id === chosen)?.name ?? optionId)
  blocks[index] = {
    ...block,
    answer: { optionId, name, decidedBy: str(event.payload, "decided_by") },
  }
}

/** The AI permission model's note on a request or a reply, as the TUI words it. */
function aiNote(payload: unknown): string | undefined {
  const label = str(payload, "label")
  const metrics = aiMetrics(payload)
  if (label !== undefined && metrics !== undefined) {
    const decision =
      str(payload, "decided_by") === "ai"
        ? label === "deny"
          ? "AI denied"
          : "AI allowed"
        : `AI said ${label}`
    return `${decision} · ${metrics}`
  }
  const error = str(payload, "ai_error")
  return error === undefined ? undefined : `AI ${error}`
}

function aiMetrics(payload: unknown): string | undefined {
  const probabilities = at(payload, "probabilities")
  if (typeof probabilities !== "object" || probabilities === null) return undefined
  const count = Object.keys(probabilities).length
  const number = (value: unknown) => (typeof value === "number" ? value : undefined)
  const allowProbability = number(at(probabilities, "0"))
  const denyProbability = number(at(probabilities, String(count - 1)))
  const danger = number(at(payload, "danger"))
  const allow = number(at(payload, "allow_threshold"))
  const deny = number(at(payload, "deny_threshold"))
  if (
    allowProbability === undefined ||
    denyProbability === undefined ||
    danger === undefined ||
    allow === undefined ||
    deny === undefined
  )
    return undefined
  const pct = (value: number) => Math.round(value * 100)
  let metrics = `allow ${pct(allowProbability)}%, deny ${pct(denyProbability)}% · danger ${pct(danger)}% (allow up to ${pct(allow)}%, deny from ${pct(deny)}%)`
  const tags = list(payload, "risk_tags").filter((tag): tag is string => typeof tag === "string")
  if (tags.length > 0) metrics += ` · tags: ${tags.join(", ")}`
  const cap = str(payload, "cap")
  if (cap !== undefined) metrics += ` · capped by ${cap}`
  return metrics
}

/** Whether a reply is the daemon's own allow, with nobody at the console. */
function daemonAllowed(payload: unknown, options: PermissionOption[]): boolean {
  const id = str(payload, "option_id")
  if (id === undefined) return false
  if (str(payload, "decided_by") === "console" || str(payload, "label") === "deny") return false
  const kind = options.find((option) => option.id === id)?.kind
  return kind === undefined || kind.startsWith("allow")
}

/** A diagnosis reads onto the error it names by id, and onto nothing else. */
function foldDiagnosis(blocks: Block[], event: AgentEventDto): void {
  const errorId = str(event.payload, "error_event_id")
  const index = blocks.findLastIndex((block) => block.type === "error" && block.id === errorId)
  const block = blocks[index]
  if (block?.type === "error") blocks[index] = { ...block, diagnosis: event.summary }
}

/**
 * Merge a later event of a call into its speaker's open block; false where
 * none is open. A child's call never merges into the parent's or another
 * child's, even under the same call id or title.
 */
function mergeTool(blocks: Block[], event: AgentEventDto): boolean {
  const update = toolOf(event.payload, UNNAMED_TOOL)
  const index = blocks.findLastIndex(
    (block) =>
      block.type === "tool" &&
      sameSpeaker(block, event) &&
      !isTerminal(block.tool.status) &&
      (update.id ? block.tool.id === update.id : block.tool.title === update.title),
  )
  const block = blocks[index]
  if (block?.type !== "tool") return false
  const tool = { ...block.tool }
  if (update.title !== UNNAMED_TOOL) tool.title = update.title
  if (update.kind !== undefined) tool.kind = update.kind
  if (update.input !== undefined) tool.input = update.input
  if (update.locations !== undefined) tool.locations = update.locations
  if (update.diff !== undefined) tool.diff = update.diff
  if (update.status !== undefined) tool.status = update.status
  if (isTerminal(update.status)) tool.endedAt ??= event.created_at
  Object.assign(tool, describe(tool))
  if (update.output !== undefined) {
    tool.output = update.output
    tool.outputLanguage = update.outputLanguage
    tool.firstLine = undefined
  }
  // A new kind, location or input changes how the output reads, so shape it again.
  Object.assign(tool, shapeOutput(tool))
  blocks[index] = { ...block, tool }
  return true
}

function isTerminal(status: string | undefined): boolean {
  return status === "completed" || status === "failed"
}

/** The call an event carries: `tool_name` and `tool_input` over the ACP call under `acp`. */
function toolOf(payload: unknown, fallback: string): ToolCall {
  const acp = at(payload, "acp") ?? payload
  const input = at(payload, "tool_input") ?? at(acp, "rawInput")
  return {
    id: str(acp, "toolCallId") ?? str(payload, "tool_call_id") ?? "",
    title: str(payload, "tool_name") ?? str(acp, "title") ?? fallback,
    kind: str(acp, "kind"),
    status: str(acp, "status"),
    input: input ?? undefined,
    ...toolOutput(acp),
    diff: toolDiff(acp),
    locations: locationsOf(acp),
  }
}

/** The call of a first event, with everything the renderers read derived. */
function openTool(event: AgentEventDto): ToolCall {
  const tool = toolOf(event.payload, UNNAMED_TOOL)
  tool.startedAt = event.created_at
  if (isTerminal(tool.status)) tool.endedAt = event.created_at
  return { ...tool, ...describe(tool), ...shapeOutput(tool) }
}

function locationsOf(acp: unknown): ToolLocation[] | undefined {
  const found = list(acp, "locations").flatMap((entry): ToolLocation[] => {
    const path = str(entry, "path")
    if (path === undefined) return []
    const line = at(entry, "line")
    return [typeof line === "number" ? { path, line } : { path }]
  })
  return found.length > 0 ? found : undefined
}

/** The subject and the input of a call, as its title, kind, input and locations make them. */
function describe(tool: ToolCall): Pick<ToolCall, "subject" | "inputText" | "inputLanguage"> {
  const command = tool.kind === "execute" ? commandOf(tool.input) : undefined
  const empty =
    tool.input === undefined ||
    tool.input === null ||
    tool.input === "" ||
    (typeof tool.input === "object" && Object.keys(tool.input).length === 0)
  const input =
    command !== undefined
      ? { inputText: command, inputLanguage: "shell" as const }
      : empty
        ? {}
        : { inputText: JSON.stringify(tool.input, null, 2), inputLanguage: "json" as const }
  return { subject: subjectOf(tool), ...input }
}

/** What a call is about, as the CLI's pane says it (`blocks.rs` `subject`). */
function subjectOf(tool: ToolCall): string {
  const input = tool.input
  const first = tool.locations?.[0]
  let named: string | undefined
  switch (tool.kind) {
    case "execute":
      named = commandOf(input)?.split("\n")[0]
      break
    case "read":
    case "edit":
    case "delete":
    case "move": {
      const path = field(input, ["path", "file_path", "filePath", "file"]) ?? first?.path
      if (path === undefined) break
      const inputLine = at(input, "line")
      const line =
        typeof inputLine === "number" ? inputLine : first?.path === path ? first.line : undefined
      named = line === undefined ? path : `${path}:${line}`
      break
    }
    case "search": {
      const pattern = field(input, ["pattern", "query", "regex"])
      if (pattern === undefined) break
      const path = field(input, ["path", "file_path", "directory", "cwd"]) ?? first?.path
      named = path === undefined ? pattern : `${pattern} in ${path}`
      break
    }
    case "fetch":
      named = field(input, ["url"])
      break
  }
  let head = named ?? tool.title
  const elsewhere = (tool.locations ?? [])
    .filter((location) => !head.includes(location.path))
    .map((location) =>
      location.line === undefined ? location.path : `${location.path}:${location.line}`,
    )
  if (elsewhere.length > 0) head += ` (${elsewhere.join(", ")})`
  return head
}

/** The first of `keys` the input carries as a string. */
function field(input: unknown, keys: string[]): string | undefined {
  return keys.map((key) => str(input, key)).find((found) => found !== undefined)
}

/** A `read`'s fence and line-number prefixes removed, and every output's language set. */
function shapeOutput(tool: ToolCall): Pick<ToolCall, "output" | "outputLanguage" | "firstLine"> {
  const output = tool.output
  if (tool.kind === "read") {
    const path = tool.locations?.[0]?.path ?? str(tool.input, "file_path")
    const language = path === undefined ? undefined : languageForPath(path)
    const stripped = output === undefined ? undefined : stripNumbered(output)
    const firstLine = stripped?.firstLine ?? tool.firstLine
    const text = stripped?.text ?? output
    const pretty = firstLine === undefined ? prettyJson(text) : undefined
    return {
      output: pretty ?? text,
      outputLanguage: language ?? (pretty === undefined ? tool.outputLanguage : "json"),
      firstLine,
    }
  }
  const pretty = tool.outputLanguage === undefined ? prettyJson(output) : undefined
  if (pretty !== undefined) return { output: pretty, outputLanguage: "json" }
  return { output, outputLanguage: tool.outputLanguage }
}

/** Text that starts with `{` or `[` and parses as JSON, written with two-space indents. */
function prettyJson(text: string | undefined): string | undefined {
  if (text === undefined || !/^\s*[{[]/.test(text)) return undefined
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    return undefined
  }
}

/** Claude Code's Read: a fence around lines that start with a number and a tab. */
function stripNumbered(output: string): { text: string; firstLine: number } | undefined {
  const lines = output.split("\n")
  if (!lines[0]?.startsWith("```")) return undefined
  const end = lines.length > 1 && lines.at(-1)?.trim() === "```" ? -1 : undefined
  const body = lines.slice(1, end)
  const prefix = /^\s*(\d+)\t/
  if (body.length === 0 || !body.every((line) => prefix.test(line))) return undefined
  return {
    text: body.map((line) => line.replace(prefix, "")).join("\n"),
    firstLine: Number(prefix.exec(body[0] as string)?.[1]),
  }
}

function commandOf(input: unknown): string | undefined {
  const command = at(input, "command")
  if (typeof command === "string") return command
  if (Array.isArray(command)) return command.filter((part) => typeof part === "string").join(" ")
  return undefined
}

/** The text of the call's `content` entries, else its `rawOutput` as text. */
function toolOutput(acp: unknown): Pick<ToolCall, "output" | "outputLanguage"> {
  const text = list(acp, "content").flatMap((entry) => {
    if (str(entry, "type") !== "content") return []
    if (str(entry, "content", "type") === "image") {
      const mime = str(entry, "content", "mimeType")
      return [mime === undefined ? "[image]" : `[image] ${mime}`]
    }
    return [str(entry, "content", "text") ?? ""]
  })
  const lines = text.filter((line) => line.length > 0)
  if (lines.length > 0) return { output: lines.join("\n") }
  const raw = at(acp, "rawOutput")
  if (raw === undefined || raw === null) return {}
  if (typeof raw === "string") return { output: raw }
  if (typeof raw === "object")
    return { output: JSON.stringify(raw, null, 2), outputLanguage: "json" }
  return { output: JSON.stringify(raw) }
}

/** The agent's patch under a file header, or the lines between old and new text. */
function toolDiff(acp: unknown): string | undefined {
  const entry = list(acp, "content").find((content) => str(content, "type") === "diff")
  if (entry === undefined) return undefined
  const path = str(entry, "path")
  const patch = str(entry, "patch", "text")
  if (patch !== undefined) {
    const lines = patch.split("\n")
    const hunk = lines.findIndex((line) => line.startsWith("@@"))
    const head = hunk === -1 ? lines : lines.slice(0, hunk)
    const headed = head.some((line) => line.startsWith("--- ") || line.startsWith("+++ "))
    return path !== undefined && !headed ? `--- ${path}\n+++ ${path}\n${patch}` : patch
  }
  if (path === undefined) return undefined
  const lines = lineDiff(str(entry, "oldText") ?? "", str(entry, "newText") ?? "")
  return lines.length > 0 ? `--- ${path}\n+++ ${path}\n${lines.join("\n")}` : undefined
}

/** The most lines times lines a diff is computed over before it gives up on context. */
const DIFF_BUDGET = 1_000_000

/**
 * Every line of two texts marked kept, removed or added, by their longest
 * common run of lines. Empty where the texts are the same.
 */
function lineDiff(oldText: string, newText: string): string[] {
  if (oldText === newText) return []
  const before = splitLines(oldText)
  const after = splitLines(newText)
  if (before.length * after.length > DIFF_BUDGET) {
    return [...before.map((line) => `-${line}`), ...after.map((line) => `+${line}`)]
  }
  const common: number[][] = Array.from({ length: before.length + 1 }, () =>
    new Array<number>(after.length + 1).fill(0),
  )
  for (let i = before.length - 1; i >= 0; i--) {
    for (let j = after.length - 1; j >= 0; j--) {
      const row = common[i] as number[]
      row[j] =
        before[i] === after[j]
          ? (common[i + 1]?.[j + 1] ?? 0) + 1
          : Math.max(common[i + 1]?.[j] ?? 0, row[j + 1] ?? 0)
    }
  }
  const out: string[] = []
  let i = 0
  let j = 0
  while (i < before.length || j < after.length) {
    if (i < before.length && j < after.length && before[i] === after[j]) {
      out.push(` ${before[i]}`)
      i++
      j++
    } else if (
      j < after.length &&
      (i >= before.length || (common[i]?.[j + 1] ?? 0) > (common[i + 1]?.[j] ?? 0))
    ) {
      out.push(`+${after[j]}`)
      j++
    } else {
      out.push(`-${before[i]}`)
      i++
    }
  }
  return out
}

function splitLines(text: string): string[] {
  if (text.length === 0) return []
  const lines = text.split("\n")
  if (lines.at(-1) === "") lines.pop()
  return lines
}

function stopNote(reason: string | undefined): string {
  switch (reason) {
    case undefined:
      return "turn stopped"
    case "cancelled":
      return "turn cancelled"
    case "max_tokens":
      return "turn stopped: token limit reached"
    case "max_turn_requests":
      return "turn stopped: request limit reached"
    case "refusal":
      return "turn stopped: the agent refused"
    default:
      return `turn stopped: ${reason.replaceAll("_", " ")}`
  }
}

function errorText(event: AgentEventDto): string {
  const payload = event.payload
  return (
    str(payload, "error", "data", "message") ??
    str(payload, "error", "message") ??
    str(payload, "message") ??
    event.summary
  )
}

function textOf(event: AgentEventDto): string {
  return str(event.payload, "text") ?? event.summary
}

function at(value: unknown, ...path: string[]): unknown {
  let current = value
  for (const key of path) {
    if (typeof current !== "object" || current === null) return undefined
    current = (current as Record<string, unknown>)[key]
  }
  return current
}

function str(value: unknown, ...path: string[]): string | undefined {
  const found = at(value, ...path)
  return typeof found === "string" ? found : undefined
}

function list(value: unknown, key: string): unknown[] {
  const found = at(value, key)
  return Array.isArray(found) ? found : []
}

/**
 * The blocks as plain text, one section per block, for the copy of a loaded
 * transcript. It holds what the console has loaded and nothing older.
 */
export function blocksText(blocks: readonly Block[]): string {
  return blocks.map(blockText).join("\n\n")
}

function blockText(block: Block): string {
  switch (block.type) {
    case "prompt":
      return `> ${block.text}`
    case "message":
      return block.text
    case "thought":
      return `(thought) ${block.text}`
    case "plan":
      return block.entries.map((entry) => `[${entry.status}] ${entry.content}`).join("\n")
    case "tool":
      return [
        `[${block.tool.status ?? "pending"}] ${block.tool.kind ? `${block.tool.kind} ` : ""}${block.tool.title}`,
        block.tool.subject === block.tool.title ? undefined : block.tool.subject,
        block.tool.inputText,
        block.tool.output,
        block.tool.diff,
      ]
        .filter(Boolean)
        .join("\n")
    case "permission":
      return [
        `Permission: ${block.title}`,
        block.command,
        block.diff,
        block.aiNote,
        block.answer ? `-> ${block.answer.name}` : undefined,
      ]
        .filter(Boolean)
        .join("\n")
    case "question":
      return [
        `Question: ${block.question.text}`,
        block.resolution
          ? `-> ${block.resolution.state}${block.resolution.answer ? `: ${block.resolution.answer}` : ""}`
          : undefined,
      ]
        .filter(Boolean)
        .join("\n")
    case "note":
    case "child":
      return block.text
    case "error":
      return [`Error: ${block.text}`, block.diagnosis].filter(Boolean).join("\n")
    case "raw":
      return `${block.kind}: ${block.summary}`
  }
}

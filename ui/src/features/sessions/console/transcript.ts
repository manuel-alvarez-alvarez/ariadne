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
 * spoke where `payload.subagent` names one: a child's events stay inline,
 * tagged with its title.
 */

import type { AgentEventDto } from "@/api"

interface Subagent {
  id: string
  title: string
}

interface BlockBase {
  /** The id of the event this block opened on; `local:<n>` for a local block. */
  id: string
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
      title: string
      /** The command the call runs, where it runs one. */
      command?: string
      diff?: string
      options: PermissionOption[]
      answer?: { optionId?: string; name: string; decidedBy?: string }
    })
  | (BlockBase & { type: "note"; text: string })
  | (BlockBase & { type: "error"; text: string; diagnosis?: string })
  | (BlockBase & { type: "raw"; kind: string; summary: string })

export interface Transcript {
  blocks: Block[]
  /** A turn runs: a prompt or the agent's output since the last stop. */
  running: boolean
  /** Every event id folded in, so an event delivered twice is folded once. */
  seen: ReadonlySet<string>
}

export function emptyTranscript(): Transcript {
  return { blocks: [], running: false, seen: new Set() }
}

/** The transcript a `snapshot` opens on, in place of whatever was shown. */
export function applySnapshot(_transcript: Transcript, events: AgentEventDto[]): Transcript {
  return events.reduce(applyEvent, emptyTranscript())
}

/** One more event of the stream, folded into the blocks so far. */
export function applyEvent(transcript: Transcript, event: AgentEventDto): Transcript {
  if (transcript.seen.has(event.id)) return transcript
  const seen = new Set(transcript.seen).add(event.id)
  const blocks = transcript.blocks.slice()
  fold(blocks, event)
  return { blocks, running: running(transcript.running, event.kind), seen }
}

/** An error the console itself met, such as a send the daemon refused. */
export function appendError(transcript: Transcript, text: string): Transcript {
  // No event opened it, so its id is local, and unique among the blocks.
  const id = `local:${transcript.blocks.length}`
  return { ...transcript, blocks: [...transcript.blocks, { type: "error", id, text }] }
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
    case "session.diagnosis":
      foldDiagnosis(blocks, event)
      return
    case "tool_call_update":
    case "post_tool_use":
      if (mergeTool(blocks, toolOf(payload, UNNAMED_TOOL))) return
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
      return { ...base, type: "tool", tool: toolOf(payload, UNNAMED_TOOL) }
    case "permission_request": {
      const tool = toolOf(payload, event.summary)
      return {
        ...base,
        type: "permission",
        title: tool.title,
        command: commandOf(tool.input),
        diff: tool.diff,
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
    case "compaction_update":
      return { ...base, type: "note", text: "context compacted" }
    case "agent_request": {
      const hint = str(payload, "hint")
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

function baseOf(event: AgentEventDto): BlockBase {
  const id = str(event.payload, "subagent", "id")
  if (id === undefined) return { id: event.id }
  return { id: event.id, subagent: { id, title: str(event.payload, "subagent", "title") ?? id } }
}

/** Whether a block was spoken by the same agent: the session, or one child. */
function sameSpeaker(block: Block, event: AgentEventDto): boolean {
  return block.subagent?.id === str(event.payload, "subagent", "id")
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

/** Merge a later event of a call into its open block; false where none is open. */
function mergeTool(blocks: Block[], update: ToolCall): boolean {
  const index = blocks.findLastIndex(
    (block) =>
      block.type === "tool" &&
      !isTerminal(block.tool.status) &&
      (update.id ? block.tool.id === update.id : block.tool.title === update.title),
  )
  const block = blocks[index]
  if (block?.type !== "tool") return false
  const tool = { ...block.tool }
  if (update.title !== UNNAMED_TOOL) tool.title = update.title
  if (update.kind !== undefined) tool.kind = update.kind
  if (update.input !== undefined) tool.input = update.input
  if (update.output !== undefined) tool.output = update.output
  if (update.diff !== undefined) tool.diff = update.diff
  if (update.status !== undefined) tool.status = update.status
  blocks[index] = { ...block, tool }
  return true
}

export function isTerminal(status: string | undefined): boolean {
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
    output: toolOutput(acp),
    diff: toolDiff(acp),
  }
}

function commandOf(input: unknown): string | undefined {
  const command = at(input, "command")
  if (typeof command === "string") return command
  if (Array.isArray(command)) return command.filter((part) => typeof part === "string").join(" ")
  return undefined
}

/** The text of the call's `content` entries, else its `rawOutput` as text. */
function toolOutput(acp: unknown): string | undefined {
  const text = list(acp, "content")
    .filter((entry) => str(entry, "type") === "content")
    .map((entry) => str(entry, "content", "text") ?? "")
    .filter((line) => line.length > 0)
  if (text.length > 0) return text.join("\n")
  const raw = at(acp, "rawOutput")
  if (raw === undefined || raw === null) return undefined
  if (typeof raw === "string") return raw
  const streams = [str(raw, "stdout"), str(raw, "stderr")].filter((stream) => !!stream)
  if (streams.length > 0) return streams.join("\n")
  return JSON.stringify(raw)
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

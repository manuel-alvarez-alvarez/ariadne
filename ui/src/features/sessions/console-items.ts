/**
 * The console's transcript, folded from the events its stream delivers.
 *
 * The stream is one event per thing the runtime reported (008), and several
 * of those are pieces of one thing on screen: a turn's message arrives as
 * `agent_message_chunk`s and then, once, whole, as `agent_message`; a tool
 * call opens as `pre_tool_use`, moves through `tool_call_update`s and ends as
 * `post_tool_use`; a plan is replaced by every `plan`; a `permission.replied`
 * answers the `permission_request` before it. So the transcript is not a list
 * of events but a list of {@link ConsoleItem}s, and this is the fold that
 * turns one into the other — pure, so a snapshot and a delta go through the
 * same function and the view only ever renders what comes out.
 *
 * The kinds the fold recognizes are the ones the runtime reports today (021).
 * Anything else keeps the one-line-summary-plus-payload row every event gets
 * in the agent activity feed, so nothing the runtime starts reporting next
 * goes unrepresented.
 */

import type { AgentEventDto } from "@/api"

/** One permission option, as the runtime reports it. */
interface PermissionOption {
  optionId: string
  name: string
}

/** One entry of an ACP plan. */
export interface PlanEntry {
  content: string
  status: "pending" | "in_progress" | "completed"
  priority?: string
}

/** A tool call's `content` entry: what the tool showed, in the shapes ACP names. */
type ToolContent =
  | { type: "text"; text: string }
  | { type: "diff"; path: string; oldText: string | null; newText: string }
  | { type: "other"; raw: unknown }

/** A tool call as the runtime records it under `acp` — merged from every update. */
export interface ToolCall {
  toolCallId: string
  title: string
  status: string
  content: ToolContent[]
  rawInput: unknown
  rawOutput: unknown
}

export type ToolState = "running" | "done" | "failed"

export type ConsoleItem =
  | { kind: "prompt"; id: string; text: string; source: string }
  | { kind: "message"; id: string; text: string; live: boolean }
  | { kind: "thought"; id: string; text: string; live: boolean }
  | { kind: "tool"; id: string; call: ToolCall; state: ToolState }
  | { kind: "plan"; id: string; entries: PlanEntry[] }
  | {
      kind: "permission"
      id: string
      toolName: string
      options: PermissionOption[]
      /** `option_id` the daemon answered with, `null` for cancelled, absent while open. */
      answer?: string | null
    }
  | { kind: "note"; id: string; text: string; tone: "muted" | "stopped" }
  | { kind: "error"; id: string; text: string }
  | { kind: "raw"; id: string; event: AgentEventDto }

export interface ConsoleTranscriptState {
  items: ConsoleItem[]
  /** Between a `user_prompt_submit` and the `stop` that ends its turn. */
  turnRunning: boolean
}

export const EMPTY_TRANSCRIPT: ConsoleTranscriptState = { items: [], turnRunning: false }

/** The transcript with one more event folded in. */
export function foldConsoleEvent(
  state: ConsoleTranscriptState,
  event: AgentEventDto,
): ConsoleTranscriptState {
  const { items } = state
  const payload = record(event.payload)
  switch (event.kind) {
    case "user_prompt_submit":
      return {
        items: [
          ...items,
          {
            kind: "prompt",
            id: event.id,
            text: string(payload.text) ?? event.summary,
            source: string(payload.source) ?? "console",
          },
        ],
        turnRunning: true,
      }
    case "agent_message_chunk":
      return { ...state, items: appendChunk(items, "message", event.id, string(payload.text)) }
    case "agent_thought_chunk":
      return { ...state, items: appendChunk(items, "thought", event.id, string(payload.text)) }
    case "agent_message":
      return { ...state, items: settleText(items, "message", event.id, string(payload.text)) }
    case "agent_thought":
      return { ...state, items: settleText(items, "thought", event.id, string(payload.text)) }
    case "pre_tool_use":
      return { ...state, items: upsertTool(items, event.id, toolCall(event), "running") }
    case "tool_call_update": {
      const call = toolCall(event)
      return { ...state, items: upsertTool(items, event.id, call, toolState(call.status)) }
    }
    case "post_tool_use": {
      const call = toolCall(event)
      return {
        ...state,
        items: upsertTool(items, event.id, call, call.status === "failed" ? "failed" : "done"),
      }
    }
    case "plan":
      return { ...state, items: replacePlan(items, event.id, planEntries(payload.entries)) }
    case "permission_request":
      return {
        ...state,
        items: [
          ...items,
          {
            kind: "permission",
            id: event.id,
            toolName: string(payload.tool_name) ?? event.summary,
            options: permissionOptions(payload.options),
          },
        ],
      }
    case "permission.replied":
      return { ...state, items: answerPermission(items, string(payload.option_id) ?? null) }
    case "stop": {
      const reason = string(payload.stop_reason)
      const note: ConsoleItem[] =
        reason === "cancelled"
          ? [{ kind: "note", id: event.id, text: "Stopped", tone: "stopped" }]
          : reason && reason !== "end_turn"
            ? [{ kind: "note", id: event.id, text: `Stopped: ${reason}`, tone: "stopped" }]
            : []
      return { items: [...items, ...note], turnRunning: false }
    }
    case "session_start":
      return { ...state, items: [...items, note(event.id, "Session started")] }
    case "session_end":
      return { items: [...items, note(event.id, "Session ended")], turnRunning: false }
    case "compaction_update":
      return { ...state, items: [...items, note(event.id, "Conversation compacted")] }
    case "session.error":
      return {
        items: [...items, { kind: "error", id: event.id, text: event.summary }],
        turnRunning: false,
      }
    default:
      return { ...state, items: [...items, { kind: "raw", id: event.id, event }] }
  }
}

/** The transcript of a whole snapshot: every event folded, oldest first. */
export function foldConsoleSnapshot(events: AgentEventDto[]): ConsoleTranscriptState {
  return events.reduce(foldConsoleEvent, EMPTY_TRANSCRIPT)
}

/** The oldest permission request nothing has answered, if one is open. */
export function openPermission(
  items: ConsoleItem[],
): Extract<ConsoleItem, { kind: "permission" }> | undefined {
  return items.find(
    (item): item is Extract<ConsoleItem, { kind: "permission" }> =>
      item.kind === "permission" && item.answer === undefined,
  )
}

function note(id: string, text: string): ConsoleItem {
  return { kind: "note", id, text, tone: "muted" }
}

/**
 * One in-progress item per kind: the chunk grows the live item wherever it
 * is, or opens one at the end where none is live.
 */
function appendChunk(
  items: ConsoleItem[],
  kind: "message" | "thought",
  id: string,
  text: string | undefined,
): ConsoleItem[] {
  if (!text) return items
  const index = items.findLastIndex((item) => item.kind === kind && item.live)
  if (index === -1) return [...items, { kind, id, text, live: true }]
  return items.map((item, at) =>
    at === index && item.kind === kind ? { ...item, text: item.text + text } : item,
  )
}

/**
 * The stored text of a turn, which replaces the live item its chunks grew:
 * dropped where it was, and put where the record has it — after the turn's
 * tool calls, which is where a fresh snapshot shows it too.
 */
function settleText(
  items: ConsoleItem[],
  kind: "message" | "thought",
  id: string,
  text: string | undefined,
): ConsoleItem[] {
  const kept = items.filter((item) => !(item.kind === kind && item.live))
  if (!text) return kept
  return [...kept, { kind, id, text, live: false }]
}

/** The row of the call the update is about, or a new row where none is. */
function upsertTool(
  items: ConsoleItem[],
  id: string,
  call: ToolCall,
  state: ToolState,
): ConsoleItem[] {
  const index = call.toolCallId
    ? items.findLastIndex(
        (item) => item.kind === "tool" && item.call.toolCallId === call.toolCallId,
      )
    : -1
  if (index === -1) return [...items, { kind: "tool", id, call, state }]
  return items.map((item, at) => (at === index ? { ...item, call, state } : item))
}

/**
 * A plan update replaces the plan of the turn it is in: the last plan on
 * record, unless a prompt has opened a new turn since it.
 */
function replacePlan(items: ConsoleItem[], id: string, entries: PlanEntry[]): ConsoleItem[] {
  for (let at = items.length - 1; at >= 0; at -= 1) {
    const item = items[at]
    if (item?.kind === "prompt") break
    if (item?.kind === "plan") {
      return items.map((each, index) => (index === at ? { ...each, entries } : each))
    }
  }
  return [...items, { kind: "plan", id, entries }]
}

/**
 * `permission.replied` carries no id back to the request it answers, so the
 * reply is paired with the oldest request still open.
 */
function answerPermission(items: ConsoleItem[], answer: string | null): ConsoleItem[] {
  const index = items.findIndex((item) => item.kind === "permission" && item.answer === undefined)
  if (index === -1) return items
  return items.map((item, at) =>
    at === index && item.kind === "permission" ? { ...item, answer } : item,
  )
}

function toolState(status: string): ToolState {
  if (status === "failed") return "failed"
  if (status === "completed") return "done"
  return "running"
}

/**
 * The call under `acp`, as `pre_tool_use`, `tool_call_update` and
 * `post_tool_use` all carry it (021). An event without one — an older row,
 * or a runtime that is not ACP — still has a tool name and its input.
 */
function toolCall(event: AgentEventDto): ToolCall {
  const payload = record(event.payload)
  const acp = record(payload.acp)
  return {
    toolCallId: string(acp.toolCallId) ?? string(payload.tool_call_id) ?? "",
    title: string(acp.title) ?? string(payload.tool_name) ?? event.summary,
    status: string(acp.status) ?? "",
    content: toolContent(acp.content),
    rawInput: acp.rawInput ?? payload.tool_input,
    rawOutput: acp.rawOutput,
  }
}

function toolContent(value: unknown): ToolContent[] {
  if (!Array.isArray(value)) return []
  return value.map((entry): ToolContent => {
    const fields = record(entry)
    if (fields.type === "diff" && typeof fields.path === "string") {
      return {
        type: "diff",
        path: fields.path,
        oldText: typeof fields.oldText === "string" ? fields.oldText : null,
        newText: string(fields.newText) ?? "",
      }
    }
    if (fields.type === "content") {
      const text = string(record(fields.content).text)
      if (text !== undefined) return { type: "text", text }
    }
    return { type: "other", raw: entry }
  })
}

function planEntries(value: unknown): PlanEntry[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((entry): PlanEntry[] => {
    const fields = record(entry)
    const content = string(fields.content)
    if (content === undefined) return []
    const status = fields.status
    return [
      {
        content,
        status: status === "completed" || status === "in_progress" ? status : "pending",
        priority: string(fields.priority),
      },
    ]
  })
}

function permissionOptions(value: unknown): PermissionOption[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((option): PermissionOption[] => {
    const fields = record(option)
    const optionId = string(fields.optionId)
    if (optionId === undefined) return []
    return [{ optionId, name: string(fields.name) ?? optionId }]
  })
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {}
}

function string(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined
}

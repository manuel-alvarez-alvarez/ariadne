/**
 * The console's transcript model: the events of the console stream folded
 * into the blocks a person reads, the same fold the CLI's transcript makes
 * (`crates/ariadne-console/src/transcript.rs`).
 */

import { expect, it } from "vitest"

import type { AgentEventDto } from "@/api"

import { applyEvent, applySnapshot, type Block, emptyTranscript } from "./transcript"

let next = 0

function event(kind: string, payload: unknown = {}, id?: string): AgentEventDto {
  next += 1
  return {
    id: id ?? `01EVENT${String(next).padStart(19, "0")}`,
    kind,
    payload,
    summary: `${kind} summary`,
    created_at: "2026-11-14T10:00:00Z",
    session_id: "session",
    task_id: null,
  }
}

function fold(events: AgentEventDto[]): Block[] {
  return applySnapshot(emptyTranscript(), events).blocks
}

it("folds a prompt, marking one the daemon sent", () => {
  const blocks = fold([
    event("user_prompt_submit", { text: "Fix the bug" }),
    event("user_prompt_submit", { text: "Resume", source: "daemon" }),
    event("user_prompt_submit", {}),
  ])

  expect(blocks.map((block) => block.type === "prompt" && [block.text, block.daemon])).toEqual([
    ["Fix the bug", false],
    ["Resume", true],
    ["(prompt text not recorded)", false],
  ])
})

it("folds the agent's message and thought, and a plan with its statuses", () => {
  const blocks = fold([
    event("agent_thought", { text: "Thinking" }),
    event("agent_message", { text: "**Done**" }),
    event("plan", {
      entries: [{ content: "Read", status: "completed" }, { content: "Write" }],
    }),
  ])

  expect(blocks[0]).toMatchObject({ type: "thought", text: "Thinking" })
  expect(blocks[1]).toMatchObject({ type: "message", text: "**Done**" })
  expect(blocks[2]).toMatchObject({
    type: "plan",
    entries: [
      { content: "Read", status: "completed" },
      { content: "Write", status: "pending" },
    ],
  })
})

it("merges every event of one tool call into one block", () => {
  const blocks = fold([
    event("pre_tool_use", {
      acp: { toolCallId: "call-1", title: "Run tests", kind: "execute", status: "pending" },
    }),
    event("tool_call_update", { acp: { toolCallId: "call-1", status: "in_progress" } }),
    event("post_tool_use", {
      acp: {
        toolCallId: "call-1",
        status: "completed",
        content: [{ type: "content", content: { type: "text", text: "3 passed" } }],
      },
    }),
    event("pre_tool_use", { acp: { toolCallId: "call-2", title: "Edit", kind: "edit" } }),
  ])

  expect(blocks).toHaveLength(2)
  expect(blocks[0]).toMatchObject({
    type: "tool",
    tool: {
      id: "call-1",
      title: "Run tests",
      kind: "execute",
      status: "completed",
      output: "3 passed",
    },
  })
  expect(blocks[1]).toMatchObject({ type: "tool", tool: { id: "call-2", title: "Edit" } })
})

it("reads a tool call's diff from a patch or from its old and new text", () => {
  const blocks = fold([
    event("post_tool_use", {
      acp: {
        toolCallId: "patch",
        status: "completed",
        content: [{ type: "diff", path: "a.txt", patch: { text: "@@ -1 +1 @@\n-old\n+new\n" } }],
      },
    }),
    event("post_tool_use", {
      acp: {
        toolCallId: "texts",
        status: "completed",
        content: [{ type: "diff", path: "b.txt", oldText: "one\ntwo\n", newText: "one\n2\n" }],
      },
    }),
  ])

  expect(blocks[0]).toMatchObject({
    tool: { diff: "--- a.txt\n+++ a.txt\n@@ -1 +1 @@\n-old\n+new\n" },
  })
  expect(blocks[1]).toMatchObject({ tool: { diff: "--- b.txt\n+++ b.txt\n one\n-two\n+2" } })
})

it("notes a session's start and end, a stop other than end_turn, compaction and a request", () => {
  const blocks = fold([
    event("session_start"),
    event("stop", { stop_reason: "end_turn" }),
    event("stop", { stop_reason: "cancelled" }),
    event("stop", { stop_reason: "max_tokens" }),
    event("compaction_update"),
    event("agent_request", { hint: "pick a database" }),
    event("session_end"),
  ])

  expect(blocks.map((block) => block.type === "note" && block.text)).toEqual([
    "session started",
    "turn cancelled",
    "turn stopped: token limit reached",
    "context compacted",
    "waiting for you · pick a database",
    "session ended",
  ])
})

it("reads a diagnosis onto the error it names, and an unknown kind as its summary", () => {
  const error = event("session.error", { error: { message: "quota", data: { message: "429" } } })
  const blocks = fold([
    error,
    event("session.diagnosis", { error_event_id: error.id }),
    event("session.diagnosis", { error_event_id: "elsewhere" }),
    event("mystery"),
  ])

  expect(blocks).toHaveLength(2)
  expect(blocks[0]).toMatchObject({
    type: "error",
    text: "429",
    diagnosis: "session.diagnosis summary",
  })
  expect(blocks[1]).toMatchObject({ type: "raw", kind: "mystery", summary: "mystery summary" })
})

it("builds one open block from live chunks, closed by the stored whole without repeating it", () => {
  let transcript = applySnapshot(emptyTranscript(), [])
  transcript = applyEvent(transcript, event("agent_message_chunk", { text: "Hello " }))
  transcript = applyEvent(transcript, event("agent_message_chunk", { text: "world" }))

  expect(transcript.blocks).toEqual([
    expect.objectContaining({ type: "message", text: "Hello world", open: true }),
  ])

  transcript = applyEvent(transcript, event("agent_message", { text: "Hello world" }))

  expect(transcript.blocks).toEqual([
    expect.objectContaining({ type: "message", text: "Hello world", open: false }),
  ])
})

it("drops a late chunk of a turn its whole already closed", () => {
  const late = event("agent_thought_chunk", { text: "so far" })
  const transcript = [
    event("agent_thought", { text: "so far, and then more" }),
    event("agent_thought_chunk", { text: "so far" }, late.id),
  ].reduce(applyEvent, emptyTranscript())

  expect(transcript.blocks).toHaveLength(1)
  expect(transcript.blocks[0]).toMatchObject({ text: "so far, and then more" })
})

it("asks a permission with its call, command and options, then shows the reply", () => {
  const request = event("permission_request", {
    acp: { toolCallId: "call-9", title: "Bash", rawInput: { command: "rm -rf build" } },
    options: [
      { optionId: "allow", name: "Allow", kind: "allow_once" },
      { optionId: "reject", name: "Reject", kind: "reject_once" },
    ],
  })

  const asked = fold([request])
  expect(asked[0]).toMatchObject({
    type: "permission",
    title: "Bash",
    command: "rm -rf build",
    options: [
      { id: "allow", name: "Allow" },
      { id: "reject", name: "Reject" },
    ],
  })
  expect(asked[0]).not.toHaveProperty("answer")

  const answered = fold([
    request,
    event("permission.replied", { option_id: "reject", decided_by: "console" }),
  ])
  expect(answered[0]).toMatchObject({
    type: "permission",
    answer: { optionId: "reject", name: "Reject", decidedBy: "console" },
  })
})

it("drops a permission on Ariadne's own tool that the daemon allowed", () => {
  const blocks = fold([
    event("permission_request", {
      tool_name: "mcp__ariadne__complete_step",
      options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
    }),
    event("permission.replied", { option_id: "allow", decided_by: "mode" }),
  ])

  expect(blocks).toEqual([])
})

it("tags a subagent's block with its id and title", () => {
  const blocks = fold([
    event("agent_message", { text: "child", subagent: { id: "child-1", title: "Explorer" } }),
  ])

  expect(blocks[0]).toMatchObject({ subagent: { id: "child-1", title: "Explorer" } })
})

it("keeps a block's first event id", () => {
  const first = event("pre_tool_use", { acp: { toolCallId: "c", title: "Read" } })
  const blocks = fold([
    first,
    event("post_tool_use", { acp: { toolCallId: "c", status: "completed" } }),
  ])

  expect(blocks[0]?.id).toBe(first.id)
})

it("replaces the transcript with a snapshot, and adds an event it already holds once", () => {
  const prompt = event("user_prompt_submit", { text: "one" })
  let transcript = applySnapshot(emptyTranscript(), [prompt])
  transcript = applyEvent(transcript, prompt)

  expect(transcript.blocks).toHaveLength(1)

  transcript = applySnapshot(transcript, [prompt, event("agent_message", { text: "two" })])

  expect(transcript.blocks.map((block) => block.type)).toEqual(["prompt", "message"])
})

it("says a turn runs from its prompt until its stop", () => {
  let transcript = applyEvent(emptyTranscript(), event("user_prompt_submit", { text: "go" }))
  expect(transcript.running).toBe(true)

  transcript = applyEvent(transcript, event("stop", { stop_reason: "end_turn" }))
  expect(transcript.running).toBe(false)
})

it.each([
  ["message", "agent_message"],
  ["thought", "agent_thought"],
] as const)("keeps a parent's and a child's interleaved %s runs apart", (type, whole) => {
  const child = { id: "child-1", title: "Explorer" }
  const transcript = [
    event(`${whole}_chunk`, { text: "parent " }),
    event(`${whole}_chunk`, { text: "child", subagent: child }),
    event(`${whole}_chunk`, { text: "more" }),
    event(whole, { text: "child", subagent: child }),
    event(whole, { text: "parent more" }),
  ].reduce(applyEvent, emptyTranscript())

  expect(transcript.blocks).toEqual([
    expect.objectContaining({ type, text: "parent more", open: false }),
    expect.objectContaining({ type, text: "child", open: false, subagent: child }),
  ])
  expect(transcript.blocks[0]).not.toHaveProperty("subagent")
})

it("leaves a parent's open block open when a child's whole arrives", () => {
  const transcript = [
    event("agent_message_chunk", { text: "parent" }),
    event("agent_message", { text: "child", subagent: { id: "child-1", title: "Explorer" } }),
  ].reduce(applyEvent, emptyTranscript())

  expect(transcript.blocks[0]).toMatchObject({ text: "parent", open: true })
  expect(transcript.blocks[1]).toMatchObject({ text: "child", open: false })
})

/**
 * The console's transcript model: the events of the console stream folded
 * into the blocks a person reads, the same fold the CLI's transcript makes
 * (`crates/ariadne-console/src/transcript.rs`).
 */

import { expect, it } from "vitest"

import type { AgentEventDto } from "@/api"

import {
  applyEvent,
  applySnapshot,
  type Block,
  emptyTranscript,
  oldestEventId,
  prependEvents,
  view,
} from "./transcript"

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

it("merges a call whose start is in an older page with its end already shown", () => {
  const prompt = event("user_prompt_submit", { text: "go" })
  const start = event("pre_tool_use", {
    tool_name: "Bash",
    acp: { toolCallId: "call-1", status: "in_progress" },
  })
  const end = event("post_tool_use", {
    tool_name: "Bash",
    acp: { toolCallId: "call-1", status: "completed" },
  })
  const after = event("agent_message", { text: "done" })
  const shown = applySnapshot(emptyTranscript(), [end, after])

  const full = prependEvents(shown, [prompt, start])

  expect(full.blocks.map((block) => block.type)).toEqual(["prompt", "tool", "message"])
  expect(full.blocks[1]).toMatchObject({
    id: start.id,
    tool: { id: "call-1", title: "Bash", status: "completed" },
  })
  expect(oldestEventId(full)).toBe(prompt.id)
})

it("prepends an older page once, and leaves the shown transcript as it was", () => {
  const older = event("user_prompt_submit", { text: "first" })
  const shown = applySnapshot(emptyTranscript(), [event("agent_message", { text: "later" })])

  const full = prependEvents(shown, [older])

  expect(shown.blocks).toHaveLength(1)
  expect(prependEvents(full, [older])).toBe(full)
})

it("folds a message and a call that span the page boundary into one block each", () => {
  const start = event("pre_tool_use", { tool_name: "Bash" })
  const chunk = event("agent_message_chunk", { text: "Hello " })
  const more = event("agent_message_chunk", { text: "world" })
  const whole = event("agent_message", { text: "Hello world" })
  const end = event("post_tool_use", { tool_name: "Bash", acp: { status: "completed" } })
  const shown = applySnapshot(emptyTranscript(), [more, whole, end])

  const full = prependEvents(shown, [start, chunk])

  expect(full.blocks.map((block) => block.type)).toEqual(["tool", "message"])
  expect(full.blocks[0]).toMatchObject({ tool: { title: "Bash", status: "completed" } })
  expect(full.blocks[1]).toMatchObject({ text: "Hello world", open: false })
})

/** A `subagent_update` as the daemon stores it (021), at `at`. */
function update(id: string, title: string, state: string, at: string, more: object = {}) {
  return {
    ...event("subagent_update", {
      subagent: { id, title, description: `${title} task` },
      state,
      capabilities: { cancel: false },
      ...more,
    }),
    created_at: at,
  }
}

const EXPLORER = { id: "child-1", title: "Explorer" }

it("lists each child in announcement order with its state and whether it may be cancelled", () => {
  const transcript = applySnapshot(emptyTranscript(), [
    update("child-1", "Explorer", "running", "2026-11-14T10:00:00Z"),
    update("child-2", "Tester", "running", "2026-11-14T10:00:01Z", {
      capabilities: { cancel: true },
    }),
    update("child-1", "Explorer", "completed", "2026-11-14T10:00:05Z"),
  ])

  expect(transcript.subagents.map((child) => [child.title, child.state, child.cancel])).toEqual([
    ["Explorer", "completed", false],
    ["Tester", "running", true],
  ])
})

it("gives main a line per announcement and per end, timed from the announcement", () => {
  const transcript = applySnapshot(emptyTranscript(), [
    update("child-1", "Explorer", "running", "2026-11-14T10:00:00Z"),
    update("child-1", "Explorer", "running", "2026-11-14T10:00:30Z"),
    update("child-1", "Explorer", "failed", "2026-11-14T10:01:05Z"),
  ])

  expect(view(transcript).map((block) => block.type === "child" && block.text)).toEqual([
    "◇ subagent Explorer · Explorer task",
    "✗ subagent Explorer failed · 1m 05s",
  ])
})

it("gives a child first heard of at its end both lines", () => {
  const transcript = applySnapshot(emptyTranscript(), [
    update("child-1", "Explorer", "cancelled", "2026-11-14T10:00:00Z"),
  ])

  expect(view(transcript).map((block) => block.type === "child" && block.text)).toEqual([
    "◇ subagent Explorer · Explorer task",
    "⊘ subagent Explorer cancelled · 0s",
  ])
})

it("shows a child's events in its own view only", () => {
  const transcript = applySnapshot(emptyTranscript(), [
    event("agent_message", { text: "parent" }),
    event("agent_message", { text: "child", subagent: EXPLORER }),
    event("agent_message", { text: "other", subagent: { id: "child-2", title: "Tester" } }),
  ])

  expect(view(transcript).map((block) => block.type === "message" && block.text)).toEqual([
    "parent",
  ])
  expect(
    view(transcript, "child-1").map((block) => block.type === "message" && block.text),
  ).toEqual(["child"])
  // A child first heard of through its own event is in the list too.
  expect(transcript.subagents.map((child) => child.id)).toEqual(["child-1", "child-2"])
})

it("asks a child's permission in main, naming the child, and answers it there", () => {
  const transcript = applySnapshot(emptyTranscript(), [
    event("permission_request", {
      tool_name: "Bash",
      subagent: EXPLORER,
      options: [{ optionId: "allow", name: "Allow" }],
    }),
    event("permission.replied", { option_id: "allow", subagent: EXPLORER, decided_by: "console" }),
  ])

  expect(view(transcript, "child-1")).toEqual([])
  expect(view(transcript)).toEqual([
    expect.objectContaining({
      type: "permission",
      title: "Bash · asked by subagent Explorer",
      askedBy: EXPLORER,
      answer: { optionId: "allow", name: "Allow", decidedBy: "console" },
    }),
  ])
})

it("rebuilds the children from a fresh snapshot", () => {
  const before = applySnapshot(emptyTranscript(), [
    update("child-1", "Explorer", "running", "2026-11-14T10:00:00Z"),
  ])

  const after = applySnapshot(before, [
    update("child-2", "Tester", "running", "2026-11-14T10:00:00Z"),
  ])

  expect(after.subagents.map((child) => child.id)).toEqual(["child-2"])
})

it.each([
  ["the same call id", { acp: { toolCallId: "call-1", title: "Run" } }],
  ["no call id and the same title", { tool_name: "Run" }],
])("keeps a parent's and each child's tool calls apart with %s", (_label, call) => {
  const done = (subagent?: object) =>
    event("post_tool_use", {
      ...call,
      ...(subagent ? { subagent } : {}),
      acp: { ...("acp" in call ? call.acp : {}), status: "completed" },
    })
  const transcript = applySnapshot(emptyTranscript(), [
    event("pre_tool_use", call),
    event("pre_tool_use", { ...call, subagent: EXPLORER }),
    event("pre_tool_use", { ...call, subagent: { id: "child-2", title: "Tester" } }),
    // Each update comes after another speaker's call of the same id or title.
    done(),
    done(EXPLORER),
  ])

  const status = (blocks: Block[]) =>
    blocks.map((block) => block.type === "tool" && (block.tool.status ?? "pending"))
  expect(status(view(transcript))).toEqual(["completed"])
  expect(status(view(transcript, "child-1"))).toEqual(["completed"])
  expect(status(view(transcript, "child-2"))).toEqual(["pending"])
})

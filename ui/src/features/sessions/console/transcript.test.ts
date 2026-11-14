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
  blocksText,
  dequeuePrompt,
  elapsed,
  emptyTranscript,
  oldestEventId,
  prependEvents,
  queuePrompt,
  runningTool,
  tokenTotals,
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

it("draws no prompt block for the prompt that delivers an answer", () => {
  const blocks = fold([
    event("user_prompt_submit", { text: "Fix the bug", source: "console" }),
    event("user_prompt_submit", {
      text: "The user answered your question.",
      source: "console",
      request_id: "req-1",
      prompt_kind: "answer_delivery",
    }),
  ])

  expect(blocks.map((block) => block.type === "prompt" && block.text)).toEqual(["Fix the bug"])
  expect(blocksText(blocks)).toBe("> Fix the bug")
})

it("keeps a queued prompt when a delivery prompt carries the same text", () => {
  let transcript = queuePrompt(emptyTranscript(), "Main", "queued:1")
  transcript = applyEvent(
    transcript,
    event("user_prompt_submit", { text: "Main", source: "console", request_id: "req-1" }),
  )
  expect(transcript.queuedPrompts).toHaveLength(1)

  transcript = applyEvent(
    transcript,
    event("user_prompt_submit", { text: "Main", source: "console" }),
  )
  expect(transcript.queuedPrompts).toHaveLength(0)
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

function commands(...names: string[]) {
  return event("available_commands_update", {
    available_commands: names.map((name) => ({ name, description: `${name} does it` })),
  })
}

it("replaces the command list with each later one, an empty list included", () => {
  const events = [
    commands("init", "review"),
    event("available_commands_update", {
      available_commands: [{ name: "plan", description: "Plan", input: { hint: "the goal" } }],
    }),
  ]
  const listed = applySnapshot(emptyTranscript(), events)
  expect(listed.availableCommands).toEqual([
    { name: "plan", description: "Plan", hint: "the goal" },
  ])
  expect(listed.blocks).toEqual([])

  const emptied = applyEvent(listed, event("available_commands_update", { available_commands: [] }))
  expect(emptied.availableCommands).toEqual([])
})

it("ignores a command list a subagent sent", () => {
  const listed = applySnapshot(emptyTranscript(), [
    commands("init"),
    event("available_commands_update", {
      subagent: { id: "child" },
      available_commands: [{ name: "child-only" }],
    }),
  ])

  expect(listed.availableCommands.map((command) => command.name)).toEqual(["init"])
})

it("shows a queued prompt until its console echo arrives, and drops it on a failed send", () => {
  const queued = queuePrompt(emptyTranscript(), "ship it", "q1")
  expect(queued.queuedPrompts.map((prompt) => prompt.text)).toEqual(["ship it"])

  const typedElsewhere = applyEvent(queued, event("user_prompt_submit", { text: "ship it" }))
  expect(typedElsewhere.queuedPrompts).toHaveLength(1)
  const other = applyEvent(queued, event("user_prompt_submit", { text: "else", source: "console" }))
  expect(other.queuedPrompts).toHaveLength(1)
  const echoed = applyEvent(
    queued,
    event("user_prompt_submit", { text: "ship it", source: "console" }),
  )
  expect(echoed.queuedPrompts).toEqual([])

  expect(dequeuePrompt(queued, "q1", "ship it").queuedPrompts).toEqual([])
})

it("keeps a queued prompt across a snapshot that does not hold its echo", () => {
  const queued = queuePrompt(emptyTranscript(), "ship it", "q1")
  expect(applySnapshot(queued, []).queuedPrompts).toHaveLength(1)
  const echoed = [event("user_prompt_submit", { text: "ship it", source: "console" })]
  expect(applySnapshot(queued, echoed).queuedPrompts).toEqual([])
})

it("totals the latest usage of each launch, never below the session's own", () => {
  const stop = (source: string, input: number, output: number) =>
    event("stop", {
      stop_reason: "end_turn",
      ariadne_usage: { source, input_tokens: input, output_tokens: output },
    })
  const transcript = applySnapshot(emptyTranscript(), [
    stop("a", 100, 10),
    stop("a", 300, 30),
    stop("b", 50, 5),
  ])

  expect(tokenTotals(transcript)).toEqual({ input: 350, output: 35 })
  expect(tokenTotals(transcript, { input: 1000, output: 1 })).toEqual({ input: 1000, output: 35 })
})

it("starts the turn clock at the turn's first event and names the open tool", () => {
  const first = {
    ...event("user_prompt_submit", { text: "go" }),
    created_at: "2026-11-14T10:00:00Z",
  }
  const later = {
    ...event("pre_tool_use", { tool_name: "Bash" }),
    created_at: "2026-11-14T10:00:05Z",
  }
  const running = applySnapshot(emptyTranscript(), [first, later])
  expect(running.turnStartedAt).toBe("2026-11-14T10:00:00Z")
  expect(runningTool(running)).toBe("Bash")

  const stopped = applyEvent(running, event("stop", { stop_reason: "cancelled" }))
  expect(stopped.turnStartedAt).toBeUndefined()
})

it.each([
  ["a title only", { title: "child" }],
  ["a bare string", "child"],
  ["a null", null],
])("ignores a command list whose subagent is %s", (_label, subagent) => {
  const listed = applySnapshot(emptyTranscript(), [
    commands("init"),
    event("available_commands_update", { subagent, available_commands: [{ name: "other" }] }),
  ])

  expect(listed.availableCommands.map((command) => command.name)).toEqual(["init"])
})

it("matches a queued prompt only to an echo newer than its send", () => {
  const old = event("user_prompt_submit", { text: "again", source: "console" })
  const seen = applySnapshot(emptyTranscript(), [old])
  const queued = queuePrompt(seen, "again", "q1")

  // A resync replays the old echo first: it is not the answer to the new send.
  const replayed = applySnapshot(queued, [old])
  expect(replayed.queuedPrompts).toHaveLength(1)

  const echoed = applyEvent(
    replayed,
    event("user_prompt_submit", { text: "again", source: "console" }),
  )
  expect(echoed.queuedPrompts).toEqual([])
})

it("takes back only the queued send that failed when the same text is queued twice", () => {
  const twice = queuePrompt(queuePrompt(emptyTranscript(), "again", "q1"), "again", "q2")

  expect(dequeuePrompt(twice, "q1", "again").queuedPrompts.map((prompt) => prompt.id)).toEqual([
    "q2",
  ])
})

it("leaves no stale entry when an echo takes a failed send's entry before its failure arrives", () => {
  const twice = queuePrompt(queuePrompt(emptyTranscript(), "again", "q1"), "again", "q2")
  // The second send succeeded, and its echo came first: it takes the oldest entry.
  const echoed = applyEvent(
    twice,
    event("user_prompt_submit", { text: "again", source: "console" }),
  )
  expect(echoed.queuedPrompts.map((prompt) => prompt.id)).toEqual(["q2"])

  // The first send's failure then finds its own entry gone, and clears the stale one.
  expect(dequeuePrompt(echoed, "q1", "again").queuedPrompts).toEqual([])
})

const QUESTION = {
  text: "Which base branch?",
  options: [
    { id: "main", label: "Main", description: "Land on the trunk." },
    { id: "next", label: "Next" },
  ],
  recommended_option_id: "main",
}

it("folds a structured question into one card that its resolution closes", () => {
  const blocks = fold([
    event("agent_request", { request_id: "01RQ", hint: "Pick a base", question: QUESTION }),
    event("agent_request.resolved", {
      request_id: "01RQ",
      resolution: "answered",
      answer: { option_id: "next", text: "Next" },
    }),
  ])

  expect(blocks).toHaveLength(1)
  expect(blocks[0]).toMatchObject({
    type: "question",
    requestId: "01RQ",
    hint: "Pick a base",
    question: QUESTION,
    resolution: { state: "answered", answer: "Next" },
  })
})

it("keeps a hint-only request a note, and a resolution of it adds no block", () => {
  const blocks = fold([
    event("agent_request", { request_id: "01RQ", hint: "Approve the plan?" }),
    event("agent_request.resolved", { request_id: "01RQ", resolution: "expired" }),
  ])

  expect(blocks).toEqual([
    expect.objectContaining({ type: "note", text: "waiting for you · Approve the plan?" }),
  ])
})

it("replays a withdrawn question closed, so it is never answerable again", () => {
  const events = [
    event("agent_request", { request_id: "01RQ", hint: "Pick a base", question: QUESTION }),
    event("agent_request.resolved", { request_id: "01RQ", resolution: "withdrawn" }),
  ]
  const replayed = applySnapshot(applySnapshot(emptyTranscript(), events), events)

  expect(replayed.blocks).toEqual([
    expect.objectContaining({ type: "question", resolution: { state: "withdrawn" } }),
  ])
})

function call(
  acp: Record<string, unknown>,
  at = "2026-11-14T10:00:00Z",
  kind = "tool_call_update",
) {
  return { ...event(kind, { acp: { toolCallId: "c1", ...acp } }), created_at: at }
}

it("stamps every block with the time of the event it opened on", () => {
  const prompt = {
    ...event("user_prompt_submit", { text: "hi" }),
    created_at: "2026-11-14T10:00:01Z",
  }
  expect(fold([prompt])[0]?.createdAt).toBe("2026-11-14T10:00:01Z")
})

it("carries a call's subject, locations, input and timing", () => {
  const blocks = fold([
    call(
      {
        kind: "execute",
        title: "Bash",
        status: "in_progress",
        rawInput: { command: "ls -la\necho hi" },
        locations: [{ path: "/a.ts", line: 3 }],
      },
      "2026-11-14T10:00:00Z",
    ),
    call({ status: "completed" }, "2026-11-14T10:00:02Z"),
  ])
  const tool = blocks[0]?.type === "tool" ? blocks[0].tool : undefined
  expect(tool).toMatchObject({
    subject: "ls -la (/a.ts:3)",
    inputText: "ls -la\necho hi",
    inputLanguage: "shell",
    startedAt: "2026-11-14T10:00:00Z",
    endedAt: "2026-11-14T10:00:02Z",
    locations: [{ path: "/a.ts", line: 3 }],
  })
  expect(elapsed(tool?.startedAt ?? "", tool?.endedAt ?? "")).toBe("2s")
})

it("names a read, a search and a fetch by what they touch", () => {
  const subject = (acp: Record<string, unknown>) => {
    const block = fold([call({ toolCallId: "x", ...acp })])[0]
    return block?.type === "tool" ? block.tool.subject : undefined
  }
  expect(
    subject({
      kind: "read",
      title: "Read",
      rawInput: { file_path: "/a.ts" },
      locations: [{ path: "/a.ts", line: 9 }],
    }),
  ).toBe("/a.ts:9")
  expect(
    subject({ kind: "search", title: "Grep", rawInput: { pattern: "foo", path: "src" } }),
  ).toBe("foo in src")
  expect(subject({ kind: "fetch", title: "Web", rawInput: { url: "https://x.y" } })).toBe(
    "https://x.y",
  )
  expect(subject({ kind: "other", title: "Thing", rawInput: {} })).toBe("Thing")
})

it("pretty-prints json input and output, and marks the language", () => {
  const block = fold([
    call({
      kind: "other",
      title: "T",
      rawInput: { a: 1 },
      rawOutput: { b: [2] },
      status: "completed",
    }),
  ])[0]
  const tool = block?.type === "tool" ? block.tool : undefined
  expect(tool?.inputText).toBe('{\n  "a": 1\n}')
  expect(tool?.inputLanguage).toBe("json")
  expect(tool?.output).toBe('{\n  "b": [\n    2\n  ]\n}')
  expect(tool?.outputLanguage).toBe("json")
  const text = fold([
    call({
      kind: "other",
      title: "T",
      content: [{ type: "content", content: { type: "text", text: '{"a":1}' } }],
    }),
  ])[0]
  expect(text?.type === "tool" && text.tool.output).toBe('{\n  "a": 1\n}')
  expect(tool?.startedAt).toBeDefined()
})

it("strips a numbered fence from a read and keeps the first line number", () => {
  const out = "```\n  7\tconst a = 1\n  8\tconst b = 2\n```"
  const block = fold([
    call({
      kind: "read",
      title: "Read",
      rawInput: { file_path: "/x/a.ts" },
      content: [{ type: "content", content: { type: "text", text: out } }],
    }),
  ])[0]
  expect(block?.type === "tool" && block.tool).toMatchObject({
    output: "const a = 1\nconst b = 2",
    firstLine: 7,
    outputLanguage: "typescript",
  })
})

it("counts an image as one output line", () => {
  const block = fold([
    call({ content: [{ type: "content", content: { type: "image", mimeType: "image/png" } }] }),
  ])[0]
  expect(block?.type === "tool" && block.tool.output).toBe("[image] image/png")
})

it("notes the daemon's switch events, draws no usage update, and keeps unknown kinds", () => {
  const blocks = fold([
    event("session.switched"),
    event("session.pin_fallback"),
    event("usage_update"),
    event("mystery"),
  ])
  expect(blocks.map((block) => block.type)).toEqual(["note", "note", "raw"])
  expect(blocks[0]).toMatchObject({ text: "session.switched summary" })
})

it("formats an object output as json even when it holds stdout, and a read's too", () => {
  const tool = (acp: Record<string, unknown>) => {
    const block = fold([call(acp)])[0]
    return block?.type === "tool" ? block.tool : undefined
  }
  const out = tool({ kind: "execute", title: "Bash", rawOutput: { stdout: "hi" } })
  expect(out).toMatchObject({ output: '{\n  "stdout": "hi"\n}', outputLanguage: "json" })
  const read = tool({ kind: "read", title: "Read", rawOutput: { a: 1 } })
  expect(read).toMatchObject({ output: '{\n  "a": 1\n}', outputLanguage: "json" })
})

it("sets a read's language again when a later update names its file", () => {
  const blocks = fold([
    call({ kind: "read", title: "Read", status: "in_progress", rawOutput: "const a = 1" }),
    call({ locations: [{ path: "/x/a.ts" }] }),
  ])
  expect(blocks[0]?.type === "tool" && blocks[0].tool.outputLanguage).toBe("typescript")
})

it("keeps the time of the first terminal update as the end", () => {
  const blocks = fold([
    call({ title: "T", status: "in_progress" }, "2026-11-14T10:00:00Z"),
    call({ status: "failed" }, "2026-11-14T10:00:05Z"),
    call({ status: "completed" }, "2026-11-14T10:00:09Z"),
  ])
  expect(blocks[0]?.type === "tool" && blocks[0].tool.endedAt).toBe("2026-11-14T10:00:05Z")
})

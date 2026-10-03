// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { anAgentEvent } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SessionActivity } from "./session-activity"

const EVENT = anAgentEvent({
  summary: "Read the repository guidance.",
  payload: { cwd: "/Users/me/dev/ariadne", tool_name: "Read" },
})

beforeEach(() => {
  daemonFetch.mockImplementation(async () => jsonResponse([EVENT]))
})

function renderActivity() {
  renderScreen(<SessionActivity sessionId={EVENT.session_id ?? ""} />)
}

function row(text: string) {
  const button = screen.getByText(text).closest("button")
  if (!button) throw new Error(`"${text}" has no row`)
  return button
}

it("shows the summary the daemon sent", async () => {
  renderActivity()

  expect(await screen.findByText(EVENT.summary)).toBeDefined()
  expect(screen.queryByText(/cwd/)).toBeNull()
})

it("shows plain words for all known event kinds", async () => {
  const kinds: Array<[string, string]> = [
    ["pre_tool_use", "Tool call"],
    ["post_tool_use", "Tool result"],
    ["tool_call_update", "Tool update"],
    ["permission_request", "Permission asked"],
    ["permission.replied", "Permission answered"],
    ["agent_message", "Agent said"],
    ["agent_thought", "Agent thought"],
    ["agent_message_chunk", "Agent message"],
    ["agent_thought_chunk", "Thinking"],
    ["user_message_chunk", "User message"],
    ["user_prompt_submit", "User submitted"],
    ["available_commands_update", "Commands updated"],
    ["session_start", "Session started"],
    ["stop", "Stopped"],
    ["plan", "Plan"],
  ]

  const events = kinds.map(([kind], idx) =>
    anAgentEvent({
      id: `01JEVENT${idx.toString().padStart(20, "0")}`,
      kind,
      summary: "test",
      payload: {},
    }),
  )

  daemonFetch.mockImplementation(async () => jsonResponse(events))
  renderActivity()

  for (const [, label] of kinds) {
    expect(await screen.findByText(label)).toBeDefined()
  }
})

it("shows raw name for unknown event kind", async () => {
  daemonFetch.mockImplementation(async () =>
    jsonResponse([
      anAgentEvent({
        kind: "unknown_event_kind_xyz",
        summary: "test",
        payload: {},
      }),
    ]),
  )

  renderActivity()

  expect(await screen.findByText("unknown_event_kind_xyz")).toBeDefined()
})

it("opens and closes an event payload under its row", async () => {
  const user = userEvent.setup()
  renderActivity()
  await screen.findByText(EVENT.summary)

  await user.click(row(EVENT.summary))
  expect(screen.getByRole("region", { name: "post_tool_use payload" }).textContent).toContain(
    '"cwd": "/Users/me/dev/ariadne"',
  )

  await user.click(row(EVENT.summary))
  expect(screen.queryByRole("region", { name: "post_tool_use payload" })).toBeNull()
})

it("derives a summary from tool name and first argument when daemon sends empty", async () => {
  const emptyEvent = anAgentEvent({
    kind: "pre_tool_use",
    summary: "",
    payload: { tool_name: "Read", file_path: "/tmp/x.png" },
  })

  daemonFetch.mockImplementation(async () => jsonResponse([emptyEvent]))

  renderActivity()

  expect(await screen.findByText("Read /tmp/x.png")).toBeDefined()
})

it("folds seven consecutive Read calls to one row with ×7", async () => {
  const user = userEvent.setup()
  const readEvents = Array.from({ length: 7 }, (_, i) =>
    anAgentEvent({
      id: `01JEVENT${i.toString().padStart(20, "0")}`,
      kind: "pre_tool_use",
      summary: "",
      payload: { tool_name: "Read", file_path: `/file${i}.ts` },
    }),
  )

  daemonFetch.mockImplementation(async () => jsonResponse(readEvents))

  renderActivity()

  const foldedRow = await screen.findByText(/Read \/file\d\.ts ×7/)
  expect(foldedRow).toBeDefined()

  const foldedButton = foldedRow.closest("button")
  if (!foldedButton) throw new Error("folded row has no button")
  await user.click(foldedButton)

  for (let i = 0; i < 7; i++) {
    expect(screen.getByText(`Read /file${i}.ts`)).toBeDefined()
  }
})

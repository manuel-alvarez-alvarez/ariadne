// @vitest-environment jsdom

/**
 * The channel a task's agents talk on, as the panel shows it.
 *
 * One list carries every kind, so what is worth pinning is that they are told
 * apart — the two verdicts are the ones that move the task and read like it,
 * and a question is not dressed as one — and that both ends of a message are
 * named by something a reader can act on. An agent has no name, so a message
 * that said "01AGENT… → 01AGENT…" would say nothing at all.
 */

import { screen, within } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { type MessageDto, qk, type TaskDto } from "@/api"
import { aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TaskMessages } from "./task-messages"

const TASK: TaskDto = aTask({
  agents: [
    { id: "01AUTHOR", seat: "author", skills: ["coding"] },
    { id: "01REVIEWER", seat: "reviewer", skills: ["code-review"] },
  ],
})

function message(over: Partial<MessageDto>): MessageDto {
  return {
    id: "01MSG0000000000000000000A",
    goal_id: TASK.goal_id,
    task_id: TASK.id,
    round: 1,
    kind: "note",
    from_actor: "reviewer",
    from_agent_id: "01REVIEWER",
    to_actor: "author",
    to_agent_id: "01AUTHOR",
    body: "said something",
    created_at: "2026-01-01T00:00:00Z",
    ...over,
  }
}

/** The channel, with the task already in the cache the panel shares. */
function render(messages: MessageDto[]) {
  daemonFetch.mockImplementation(async () => jsonResponse(messages))
  return renderScreen(<TaskMessages taskId={TASK.id} />, {
    seed: (queryClient) => {
      queryClient.setQueryData(qk.tasks.detail(TASK.id), TASK)
      queryClient.setQueryData(qk.tasks.messages(TASK.id), messages)
    },
  })
}

describe("the channel", () => {
  it("says so when the agents have said nothing", () => {
    render([])
    expect(screen.getByText("The agents have said nothing yet")).toBeDefined()
  })

  it("groups by round, newest round first", () => {
    render([
      message({ id: "01MSGROUND1", round: 1, body: "first round" }),
      message({ id: "01MSGROUND2", round: 2, body: "second round" }),
    ])

    const headings = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent)
    expect(headings).toEqual(["Round 21 message", "Round 11 message"])
  })

  it("names both ends by the skills they work with", () => {
    render([message({ kind: "question", body: "Why is the retry unbounded?" })])

    const card = screen.getByText("Why is the retry unbounded?").closest("article")
    expect(card).not.toBeNull()
    expect(within(card as HTMLElement).getByText(/code-review/).textContent).toContain("coding")
  })

  it("names the orchestrator by what it is, since it has no id", () => {
    render([
      message({
        kind: "question",
        to_actor: "orchestrator",
        to_agent_id: null,
        body: "Does this cover the CLI?",
      }),
    ])

    const card = screen.getByText("Does this cover the CLI?").closest("article")
    expect(within(card as HTMLElement).getByText(/orchestrator/)).toBeDefined()
  })

  it("falls back to the id for an agent the task staffs no longer", () => {
    render([message({ from_agent_id: "01GONE", body: "left behind" })])

    const card = screen.getByText("left behind").closest("article")
    expect(within(card as HTMLElement).getByText(/01GONE/)).toBeDefined()
  })

  it("tells every kind apart, and reads the two verdicts as verdicts", () => {
    render([
      message({ id: "01A", kind: "review_request", body: "please look" }),
      message({ id: "01B", kind: "question", body: "why?" }),
      message({ id: "01C", kind: "answer", body: "because" }),
      message({ id: "01D", kind: "request_changes", body: "rename it" }),
      message({ id: "01E", kind: "approve", body: "looks right" }),
      message({ id: "01F", kind: "note", body: "for the record" }),
    ])

    for (const label of [
      "Review requested",
      "Question",
      "Answer",
      "Changes requested",
      "Approved",
      "Note",
    ]) {
      expect(screen.getByText(label), label).toBeDefined()
    }
  })
})

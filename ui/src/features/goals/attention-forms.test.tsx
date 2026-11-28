// @vitest-environment jsdom

/**
 * A question an agent asks with its own tool, on the Needs attention list: a
 * form of one single-select question is answered in place, any other form is
 * left to the console its row opens.
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import { afterEach, expect, it } from "vitest"

import type { AttentionItemDto } from "@/api"
import { aSessionPage } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"

import { AttentionPopover } from "./attention-strip"

afterEach(forgetHosts)

const ITEM: AttentionItemDto = {
  id: "question:Q1",
  occurrence: "Q1",
  producer: "agent_request",
  reason: "unknown",
  summary: "Which database should I use?",
  required_action: "Answer the question in the agent's console.",
  since: "2026-11-25T10:00:00Z",
  affected: [{ kind: "session", id: "S1", label: "orchestrator" }],
  target: { kind: "console", session_id: "S1" },
}

const choice = (value: string, title: string) => ({ const: value, title })
const SINGLE = {
  type: "object",
  properties: { db: { type: "string", title: "Database", oneOf: [choice("pg", "Postgres")] } },
}
const TWO_FIELDS = {
  type: "object",
  properties: { ...SINGLE.properties, note: { type: "string", title: "Note" } },
}

function open(schema: unknown, message = "Which database should I use?") {
  const seen = stubHosts({
    local: {
      "/v1/tasks": [],
      "/v1/goals": [],
      "/v1/sessions": aSessionPage([]),
      "/v1/attention": { items: [ITEM], complete: true },
      "/v1/sessions/S1/questions": [
        {
          id: "Q1",
          session_id: "S1",
          message,
          schema,
          created_at: "2026-11-25T10:00:00Z",
        },
      ],
      "/v1/sessions/S1/questions/Q1/answer": () => new Response(null, { status: 204 }),
    },
  })
  renderScreen(<AttentionPopover />, { route: "/goals" })
  return seen
}

it("answers a form of one single-select question in place", async () => {
  const seen = open(SINGLE)

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const form = await screen.findByRole("region", { name: "Agent question" })
  fireEvent.click(within(form).getByRole("button", { name: "Postgres" }))

  await waitFor(() => expect(within(form).getByRole("status").textContent).toBe("Answer sent."))
  const answer = seen.find((request) => request.method === "POST")
  expect(answer?.url.pathname).toBe("/v1/sessions/S1/questions/Q1/answer")
  expect(JSON.parse(answer?.body ?? "{}")).toEqual({ action: "accept", content: { db: "pg" } })
})

it("opens the console of the session for a form it does not answer in place", async () => {
  open(TWO_FIELDS)

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const link = await screen.findByRole("link", { name: /Which database should I use\?/ })

  expect(link.getAttribute("href")).toContain("session=S1")
  expect(screen.queryByRole("region", { name: "Agent question" })).toBeNull()
})

it("answers one Claude question with its companion and Recommended badge in place", async () => {
  const seen = open({
    type: "object",
    properties: {
      question_0: {
        type: "string",
        title: "Database",
        oneOf: [choice("Postgres (Recommended)", "Postgres (Recommended)")],
      },
      question_0_custom: { type: "string", title: "Other" },
    },
  })
  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const form = await screen.findByRole("region", { name: "Agent question" })
  within(form).getByText("Recommended")
  expect(form.textContent).not.toContain("(Recommended)")
  fireEvent.click(within(form).getByRole("button", { name: /Postgres/ }))
  await waitFor(() =>
    expect(seen.find((r) => r.method === "POST")?.body).toBe(
      JSON.stringify({ action: "accept", content: { question_0: "Postgres (Recommended)" } }),
    ),
  )
})

it("highlights untrusted prose in an agent question", async () => {
  open(SINGLE, "Choose <<<untrusted>>>**unsafe**<<<end untrusted>>> data")
  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const form = await screen.findByRole("region", { name: "Agent question" })
  expect(within(form).getByText("unsafe").closest("mark")?.textContent).toBe("unsafe")
  expect(form.textContent).not.toContain("<<<untrusted>>>")
})

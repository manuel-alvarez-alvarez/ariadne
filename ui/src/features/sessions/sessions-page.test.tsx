// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { SessionsPage } from "./sessions-page"

const AGENT_A = "claude-agent-acp:claude-sonnet-5"
const AGENT_B = "codex-acp:gpt-6"

const BUILD = aSession({
  id: "01JSESS0000000000000000001",
  task_id: "01JTASK0000000000000000001",
  model: AGENT_A,
  worktree_path: "/work/alpha/build",
  last_activity_at: "2026-11-10T12:00:00Z",
})
const DOCS = aSession({
  id: "01JSESS0000000000000000002",
  task_id: "01JTASK0000000000000000002",
  model: AGENT_B,
  worktree_path: "/work/beta/docs",
  last_activity_at: "2026-11-01T12:00:00Z",
})

beforeEach(() => {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/sessions")
      return Promise.resolve(jsonResponse(aSessionPage([BUILD, DOCS])))
    if (pathname === "/v1/tasks") {
      return Promise.resolve(
        jsonResponse([
          aTask({ id: BUILD.task_id ?? "", title: "Build the parser" }),
          aTask({ id: DOCS.task_id ?? "", title: "Write the docs" }),
        ]),
      )
    }
    return Promise.resolve(jsonResponse([]))
  })
})

async function shownTitles(route: string): Promise<{ build: boolean; docs: boolean }> {
  renderScreen(<SessionsPage />, { route })
  await screen.findByText(/ of 2 sessions/)
  return {
    build: screen.queryByRole("row", { name: "Build the parser" }) !== null,
    docs: screen.queryByRole("row", { name: "Write the docs" }) !== null,
  }
}

it("lists every Ariadne session when no filter is set", async () => {
  expect(await shownTitles("/sessions")).toEqual({ build: true, docs: true })
})

it("narrows the rows to the sessions of one agent", async () => {
  expect(await shownTitles("/sessions?agent=codex-acp")).toEqual({ build: false, docs: true })
})

it("narrows the rows to the sessions under a directory", async () => {
  expect(await shownTitles("/sessions?dir=/work/alpha")).toEqual({ build: true, docs: false })
})

it("narrows the rows to the sessions active since a day", async () => {
  expect(await shownTitles("/sessions?since=2026-11-05")).toEqual({ build: true, docs: false })
})

it("narrows the rows to the sessions active until a day", async () => {
  expect(await shownTitles("/sessions?until=2026-11-02")).toEqual({ build: false, docs: true })
})

it("narrows the rows to the titles a search matches, ignoring case", async () => {
  expect(await shownTitles("/sessions?q=PARSER")).toEqual({ build: true, docs: false })
})

it("starts no session of its own", async () => {
  renderScreen(<SessionsPage />, { route: "/sessions" })
  await screen.findByText(/ of 2 sessions/)
  expect(screen.queryByRole("button", { name: /new session/i })).toBeNull()
})

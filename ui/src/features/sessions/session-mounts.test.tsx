// @vitest-environment jsdom

/**
 * The turn console is the console of a session in each place that mounts the
 * session view: the session panel, a task, a goal and a pull request. Each
 * reads the trajectory stream, and none reads the console stream.
 */

import { screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { GoalSessionView } from "@/features/goals/goal-sessions"
import { PullRequestPanel } from "@/features/pull-requests/pull-request-panel"
import { TaskSessionView } from "@/features/tasks/task-sessions"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"

import { SessionPanel } from "./session-panel"

const GOAL_ID = "01JGOAL000000000000000001"
const TASK_ID = "01JTASK000000000000000001"
const session = aSession({
  id: "01JSESS0000000000000000001",
  status: "idle",
  goal_id: GOAL_ID,
  task_id: TASK_ID,
  pull_request_id: "pull-42",
})

beforeEach(() => {
  stubEventSource()
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const path = new URL(request.url).pathname
    if (path === `/v1/sessions/${session.id}`) return jsonResponse(session)
    if (path === "/v1/pull-requests/pull-42") return jsonResponse(pull)
    return jsonResponse([])
  })
})

/** The URLs of the streams the view opened for the session's console. */
async function consoleStreams(): Promise<string[]> {
  await screen.findByRole("textbox", { name: "Message the agent" })
  const asked = daemonFetch.mock.calls.map(([input]) => new URL((input as Request).url).pathname)
  expect(asked.filter((path) => /\/console(\/stream)?$/.test(path))).toEqual([])
  return FakeEventSource.instances
    .map((one) => one.url)
    .filter((url) => url.includes(`/sessions/${session.id}/`))
}

const TRAJECTORY = [`http://127.0.0.1:7676/v1/sessions/${session.id}/trajectory/stream`]

it("shows the turn console in the session panel", async () => {
  renderScreen(<SessionPanel host="local" sessionId={session.id} onClose={() => {}} />)

  expect(await consoleStreams()).toEqual(TRAJECTORY)
})

it("shows the turn console in a task", async () => {
  renderScreen(<TaskSessionView taskId={TASK_ID} sessionId={session.id} onSelect={() => {}} />)

  expect(await consoleStreams()).toEqual(TRAJECTORY)
})

it("shows the turn console in a goal", async () => {
  renderScreen(
    <GoalSessionView host="local" goalId={GOAL_ID} sessionId={session.id} onSelect={() => {}} />,
  )

  expect(await consoleStreams()).toEqual(TRAJECTORY)
})

it("shows the turn console in a pull request", async () => {
  renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: `/forge/pull-requests?pr=pull-42&session=${session.id}`,
  })

  expect(await consoleStreams()).toEqual(TRAJECTORY)
})

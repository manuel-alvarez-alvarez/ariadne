// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { afterEach, expect, it } from "vitest"
import { rememberHosts } from "@/api"
import { renderScreen } from "@/test/harness"
import { BOX, forgetHosts, stubHosts } from "@/test/hosts"
import { pull } from "@/test/pull-request"
import { PullRequestPanel } from "./pull-request-panel"

afterEach(forgetHosts)

it("links the task on the request host", async () => {
  rememberHosts([BOX])
  stubHosts({
    box: {
      "/v1/pull-requests/pull-42": { ...pull, origin_task_id: "task-42" },
      "/v1/repositories": [],
      "/v1/sessions": { sessions: [], next_cursor: null },
    },
  })
  renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=pull-42&host=box",
  })
  const link = await screen.findByRole("link", { name: /the author of task/ })
  expect(link.getAttribute("href")).toContain("host=box")
  expect(link.getAttribute("href")).toContain("task=task-42")
})

it("reads the selected session on the request host", async () => {
  rememberHosts([BOX])
  const seen = stubHosts({
    box: {
      "/v1/pull-requests/pull-42": pull,
      "/v1/sessions/session-42": () =>
        new Response(
          JSON.stringify({
            error: { code: "session_not_found", message: "Missing session" },
          }),
          { status: 404, headers: { "content-type": "application/json" } },
        ),
    },
  })
  renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=pull-42&host=box&session=session-42",
  })
  await screen.findByText(/Could not load session/)
  expect(
    seen.some(
      (request) =>
        request.origin === "http://box.test:7676" &&
        request.url.pathname === "/v1/sessions/session-42",
    ),
  ).toBe(true)
})

// @vitest-environment jsdom

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { rememberHosts } from "@/api"
import { HostPicker } from "@/components/host-picker"
import { useSettingsStore } from "@/stores/settings"
import { aForge, aModel, aRepository, aSession, aSessionPage } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, stubHosts } from "@/test/hosts"
import { pull } from "@/test/pull-request"
import { PullRequestPanel } from "./pull-request-panel"
import { PullRequestsPage } from "./pull-requests-page"

afterEach(() => {
  vi.restoreAllMocks()
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

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

it("shows one row for the same request on two hosts, with the session link of each", async () => {
  useSettingsStore.setState({ hostFilter: "" })
  const errors = vi.spyOn(console, "error")
  const repository = aRepository({ id: pull.repository_id, forge: aForge({ enabled: true }) })
  const answers = {
    "/v1/repositories": [repository],
    "/v1/pull-requests": {
      pull_requests: [{ ...pull, session_id: "session-42" }],
      next_cursor: null,
    },
  }
  stubHosts({ local: answers, box: answers })
  renderScreen(
    <>
      <HostPicker />
      <PullRequestsPage />
    </>,
    { route: "/forge/pull-requests" },
  )
  const row = await screen.findByRole("row", { name: "Open #42 Fix widgets" })
  const box = await within(row).findByRole("link", { name: "Open the session of #42 on box" })
  expect(box.getAttribute("href")).toContain("session=session-42")
  expect(box.getAttribute("href")).toContain("host=box")
  const local = within(row).getByRole("link", { name: "Open the session of #42 on local" })
  expect(local.getAttribute("href")).not.toContain("host=")
  expect(screen.getAllByRole("row", { name: "Open #42 Fix widgets" })).toHaveLength(1)
  expect(errors.mock.calls.flat().join(" ")).not.toMatch(/same key/)
  errors.mockRestore()
})

it("shows the panel host label when a named host is listed", async () => {
  rememberHosts([BOX])
  stubHosts({
    box: {
      "/v1/pull-requests/pull-42": pull,
      "/v1/repositories": [],
      "/v1/sessions": aSessionPage([]),
    },
  })
  renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=pull-42&host=box",
  })
  expect((await screen.findByTitle("Host")).textContent).toBe("box")
})

it("opens a session from the sessions tab on box", async () => {
  rememberHosts([BOX])
  const session = aSession({ id: "session-42", pull_request_id: "pull-42" })
  stubHosts({
    box: {
      "/v1/pull-requests/pull-42": pull,
      "/v1/repositories": [],
      "/v1/sessions": aSessionPage([session]),
      "/v1/sessions/session-42": session,
    },
  })
  const { location } = renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=pull-42&host=box&tab=sessions",
  })
  fireEvent.click(await screen.findByRole("button", { name: /Open .* session/ }))
  expect(location.url).toContain("session=session-42")
  expect(location.url).toContain("host=box")
})

it.each([
  ["review", "Start an Ariadne review", "reviewer"],
  ["babysit", "Start Ariadne babysitting", "keeper"],
] as const)("opens the new %s console on box", async (action, title, seat) => {
  rememberHosts([BOX])
  const user = userEvent.setup()
  let started = false
  const session = aSession({ id: "session-new", seat, pull_request_id: "pull-42", task_id: null })
  const repository = aRepository({ id: pull.repository_id, forge: aForge({ enabled: true }) })
  const seen = stubHosts({
    local: { "/v1/repositories": [repository], "/v1/skills": [] },
    box: {
      "/v1/pull-requests/pull-42": pull,
      "/v1/repositories": [repository],
      "/v1/models": [aModel({ id: "stub:review-model" })],
      "/v1/skills": [],
      "/v1/acp-agents": [],
      "/v1/sessions": () => jsonResponse(aSessionPage(started ? [session] : [])),
      "/v1/sessions/session-new": session,
      [`/v1/repositories/${pull.repository_id}/pull-requests/42/ariadne-${action}`]: (
        _url: URL,
        request: Request,
      ) => {
        if (request.method === "PUT") started = true
        return jsonResponse(pull)
      },
    },
  })
  const { location, queryClient } = renderScreen(
    <PullRequestPanel id="pull-42" onClose={() => {}} />,
    {
      route: "/forge/pull-requests?pr=pull-42&host=box",
    },
  )
  fireEvent.click(await screen.findByRole("button", { name: `Start ${action}` }))
  const dialog = await screen.findByRole("dialog", { name: title })
  fireEvent.click(within(dialog).getByRole("button", { name: "Model" }))
  fireEvent.click(await screen.findByText("stub:review-model"))
  await user.keyboard("{Escape}")
  fireEvent.click(within(dialog).getByRole("button", { name: `Start ${action}` }))
  await waitFor(() => expect(started).toBe(true))
  await queryClient.invalidateQueries()
  await screen.findByText("started")
  expect(screen.getByRole("tab", { name: "Console" }).getAttribute("aria-selected")).toBe("true")
  expect(location.url).toContain("session=session-new")
  expect(location.url).toContain("host=box")
  expect(
    seen.some(
      (request) =>
        request.origin === "http://box.test:7676" &&
        request.url.pathname === "/v1/sessions/session-new",
    ),
  ).toBe(true)
})

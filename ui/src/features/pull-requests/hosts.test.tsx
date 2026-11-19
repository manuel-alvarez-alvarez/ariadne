// @vitest-environment jsdom

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { rememberHosts } from "@/api"
import { HostPicker } from "@/components/host-picker"
import { useSettingsStore } from "@/stores/settings"
import { aForge, aModel, aRepository, aSession, aSessionPage } from "@/test/fixtures"
import { errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, type SeenRequest, stubHosts } from "@/test/hosts"
import { pull } from "@/test/pull-request"
import { PullRequestPanel } from "./pull-request-panel"
import { PullRequestsPage } from "./pull-requests-page"

const BOX_ORIGIN = "http://box.test:7676"

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

it("shows both host labels and the box session link for the same request", async () => {
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
  await screen.findAllByRole("row", { name: "Open #42 Fix widgets" })
  await waitFor(() =>
    expect(screen.getAllByRole("row", { name: "Open #42 Fix widgets" })).toHaveLength(2),
  )
  const both = screen.getAllByRole("row", { name: "Open #42 Fix widgets" })
  expect(both).toHaveLength(2)
  for (const host of ["local", "box"]) {
    const row = both.find((row) => within(row).queryByText(host))
    if (!row) throw new Error(`Missing ${host} row`)
    expect(within(row).getByTitle("Host").textContent).toBe(host)
    const href = within(row)
      .getByRole("link", { name: "Open the session of #42" })
      .getAttribute("href")
    expect(href).toContain("session=session-42")
    if (host === "box") expect(href).toContain("host=box")
  }
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
    local: {
      "/v1/repositories": [repository],
      "/v1/skills": [],
      // Local has the repository too, and does no work on the request.
      [`/v1/repositories/${pull.repository_id}/pull-requests/42`]: { ...pull, id: null },
    },
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

// One forge repository, registered on both hosts under ids of their own.
const forge = aForge({ enabled: true })
const LOCAL_REPO = aRepository({ id: "repo-local", forge })
const BOX_REPO = aRepository({ id: "repo-box", forge })
const localCopy = { ...pull, id: null, repository_id: "repo-local" }
const boxCopy = { ...pull, id: "pull-box", repository_id: "repo-box" }

/** `local` and `box` with the request, each answering it with `local` and `box`. */
function twoCopies(
  local: Partial<typeof pull>,
  box: Partial<typeof pull>,
  more: { local?: Record<string, unknown>; box?: Record<string, unknown> } = {},
) {
  rememberHosts([BOX])
  return stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/repositories/repo-local/pull-requests/42": { ...localCopy, ...local },
      "/v1/sessions": aSessionPage([]),
      ...more.local,
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/repositories/repo-box/pull-requests/42": { ...boxCopy, ...box },
      "/v1/sessions": aSessionPage([]),
      ...more.box,
    },
  })
}

const posted = (seen: SeenRequest[], origin: string, path: string, method: string) =>
  seen.some(
    (request) =>
      request.origin === origin && request.url.pathname === path && request.method === method,
  )

it("names the host that reviews and the task that keeps a request read on another host", async () => {
  twoCopies(
    { id: "pull-local", origin_task_id: "task-7" },
    {
      review_asked: true,
      operations: [{ operation: "review", goal_id: "goal-box", task_id: "task-box" }],
    },
  )
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  expect(await screen.findByText("Ariadne reviewing on box")).toBeTruthy()
  expect(screen.getByText("Keeps it for a task on local; Reviews it on box")).toBeTruthy()
  const kept = screen.getByRole("link", { name: /the author of task/ }).getAttribute("href")
  expect(kept).toContain("task=task-7")
  expect(kept).not.toContain("host=")
  const goal = screen.getByRole("link", { name: "Review goal" }).getAttribute("href")
  expect(goal).toContain("goal=goal-box")
  expect(goal).toContain("host=box")
})

it("links the task that keeps the request on the host of the task", async () => {
  twoCopies({}, { origin_task_id: "task-9" })
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  const link = await screen.findByRole("link", { name: /the author of task/ })
  expect(link.getAttribute("href")).toContain("task=task-9")
  expect(link.getAttribute("href")).toContain("host=box")
  expect(screen.queryByRole("button", { name: "Start babysit" })).toBeNull()
})

it("links the babysit goal on the host that babysits", async () => {
  twoCopies(
    {},
    {
      babysit_asked: true,
      operations: [{ operation: "babysit", goal_id: "goal-sit", task_id: "task-sit" }],
    },
  )
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  const link = await screen.findByRole("link", { name: "Babysit goal" })
  expect(link.getAttribute("href")).toContain("goal=goal-sit")
  expect(link.getAttribute("href")).toContain("host=box")
})

it("sends Review again and Stop review to the host that reviews, with its repository id", async () => {
  const seen = twoCopies(
    {},
    { review_asked: true },
    {
      box: {
        "/v1/repositories/repo-box/pull-requests/42/review-round": () => jsonResponse(boxCopy),
        "/v1/repositories/repo-box/pull-requests/42/ariadne-review": () =>
          jsonResponse({ ...boxCopy, review_asked: false }),
      },
    },
  )
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  fireEvent.click(await screen.findByRole("button", { name: "Review again" }))
  expect(screen.queryByRole("button", { name: "Start review" })).toBeNull()
  await waitFor(() =>
    expect(
      posted(seen, BOX_ORIGIN, "/v1/repositories/repo-box/pull-requests/42/review-round", "POST"),
    ).toBe(true),
  )
  fireEvent.click(screen.getByRole("button", { name: "Stop review" }))
  await waitFor(() =>
    expect(
      posted(seen, BOX_ORIGIN, "/v1/repositories/repo-box/pull-requests/42/ariadne-review", "PUT"),
    ).toBe(true),
  )
  expect(seen.some((request) => request.method !== "GET" && request.origin !== BOX_ORIGIN)).toBe(
    false,
  )
})

it("sends Keeper turn and Stop babysit to the host that keeps, with its repository id", async () => {
  const seen = twoCopies(
    {},
    { babysit_asked: true },
    {
      box: {
        "/v1/repositories/repo-box/pull-requests/42/keeper-turn": () => jsonResponse(boxCopy),
        "/v1/repositories/repo-box/pull-requests/42/ariadne-babysit": () =>
          jsonResponse({ ...boxCopy, babysit_asked: false }),
      },
    },
  )
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  fireEvent.click(await screen.findByRole("button", { name: "Keeper turn" }))
  expect(screen.queryByRole("button", { name: "Start babysit" })).toBeNull()
  await waitFor(() =>
    expect(
      posted(seen, BOX_ORIGIN, "/v1/repositories/repo-box/pull-requests/42/keeper-turn", "POST"),
    ).toBe(true),
  )
  fireEvent.click(screen.getByRole("button", { name: "Stop babysit" }))
  await waitFor(() =>
    expect(
      posted(seen, BOX_ORIGIN, "/v1/repositories/repo-box/pull-requests/42/ariadne-babysit", "PUT"),
    ).toBe(true),
  )
  expect(seen.some((request) => request.method !== "GET" && request.origin !== BOX_ORIGIN)).toBe(
    false,
  )
})

it("lists the sessions of every host, each row named by its host, and opens one on its host", async () => {
  const localReview = aSession({ id: "session-local", seat: "reviewer", task_id: null })
  const boxKeeper = aSession({
    id: "session-box",
    seat: "keeper",
    task_id: null,
    pull_request_id: "pull-box",
  })
  const byRequest = (id: string, session: ReturnType<typeof aSession>) => (url: URL) =>
    jsonResponse(aSessionPage(url.searchParams.get("pull_request") === id ? [session] : []))
  const seen = twoCopies(
    { id: "pull-local" },
    {},
    {
      local: { "/v1/sessions": byRequest("pull-local", localReview) },
      box: {
        "/v1/sessions": byRequest("pull-box", boxKeeper),
        "/v1/sessions/session-box": boxKeeper,
      },
    },
  )
  const { location } = renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42&tab=sessions",
  })
  const reviewer = await screen.findByRole("button", { name: "Open Reviewer session" })
  const keeper = await screen.findByRole("button", { name: "Open Keeper session" })
  expect(within(reviewer.closest("tr") as HTMLElement).getByTitle("Host").textContent).toBe("local")
  expect(within(keeper.closest("tr") as HTMLElement).getByTitle("Host").textContent).toBe("box")
  fireEvent.click(keeper)
  expect(location.url).toContain("session=session-box")
  expect(location.url).toContain("host=box")
  expect(location.url).toContain("pr=repo-box%3A42")
  await waitFor(() =>
    expect(posted(seen, BOX_ORIGIN, "/v1/sessions/session-box", "GET")).toBe(true),
  )
})

it("warns of a host it cannot read, by name and reason, and keeps the other hosts' work", async () => {
  twoCopies(
    { review_asked: true },
    {},
    {
      box: {
        "/v1/repositories/repo-box/pull-requests/42": () =>
          errorResponse(502, "forge_unavailable", "The forge did not answer"),
      },
    },
  )
  renderScreen(<PullRequestPanel id="repo-local:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo-local%3A42",
  })
  expect(await screen.findByText("Could not load the pull request on box")).toBeTruthy()
  expect(screen.getByText(/The forge did not answer/)).toBeTruthy()
  expect(screen.getByText("Ariadne reviewing on local")).toBeTruthy()
  expect(screen.getByRole("button", { name: "Stop review" })).toBeTruthy()
})

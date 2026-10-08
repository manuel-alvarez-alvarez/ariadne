// @vitest-environment jsdom
import { act, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import { DetailPanels } from "@/components/detail-panels"
import { dispatchDomainEvent } from "@/events/dispatch"
import { aForge, aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"
import { PullRequestsPage } from "./pull-requests-page"

it("renders rows, updates from an event, and opens the floating panel", async () => {
  let row = pull
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (path === "/v1/repositories")
      return jsonResponse([aRepository({ id: "repo", forge: aForge({ enabled: true }) })])
    if (path === "/v1/pull-requests") return jsonResponse([row])
    return jsonResponse(row)
  })
  const { queryClient, location } = renderScreen(
    <>
      <PullRequestsPage />
      <DetailPanels />
    </>,
    { route: "/pull-requests" },
  )
  expect((await screen.findByRole("link", { name: "#42 Fix widgets" })).getAttribute("href")).toBe(
    pull.url,
  )
  row = { ...pull, checks: "success" }
  act(() => dispatchDomainEvent(queryClient, { event: "pull_request_updated", data: row }))
  expect(await screen.findByText("success")).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "Inspect #42" }))
  expect(location.url).toBe("/pull-requests?pr=pull-42")
  expect(await screen.findByText("head_sha")).toBeTruthy()
})

it("searches enabled repositories and adds a match", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    const path = new URL(request.url).pathname
    if (path === "/v1/repositories")
      return jsonResponse([
        aRepository({ id: "repo", forge: aForge({ enabled: true }) }),
        aRepository({ id: "off", forge: null }),
      ])
    if (path.endsWith("/search"))
      return jsonResponse([
        {
          number: 42,
          url: pull.url,
          title: pull.title,
          author_login: "me",
          role: "author",
          tracked: false,
        },
      ])
    return jsonResponse(request.method === "POST" ? pull : [])
  })
  renderScreen(<PullRequestsPage />)
  await userEvent.click(screen.getByRole("button", { name: "Add pull request" }))
  await waitFor(() =>
    expect(
      screen.getByLabelText("Search repository").querySelector('option[value="repo"]'),
    ).not.toBeNull(),
  )
  await userEvent.selectOptions(screen.getByLabelText("Search repository"), "repo")
  await userEvent.type(screen.getByLabelText("Search requests"), "widgets")
  await userEvent.click(screen.getByRole("button", { name: "Search" }))
  expect(await screen.findByText("me · author")).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "Add #42" }))
  await waitFor(() =>
    expect(
      requests.some((r) => r.method === "POST" && new URL(r.url).pathname === "/v1/pull-requests"),
    ).toBe(true),
  )
  const added = requests.find((r) => r.method === "POST")
  expect(await added?.json()).toEqual({ repository_id: "repo", number: 42 })
})

it("adds a URL without issuing a search for an empty repository", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    return jsonResponse(request.method === "POST" ? pull : [])
  })
  renderScreen(<PullRequestsPage />)
  await userEvent.click(screen.getByRole("button", { name: "Add pull request" }))
  await userEvent.type(screen.getByLabelText("Pull request URL"), pull.url)
  await userEvent.click(screen.getByRole("button", { name: "Add URL" }))
  expect(await screen.findByRole("status")).toBeTruthy()
  expect(requests.some((request) => request.url.includes("/search"))).toBe(false)
  const added = requests.find((request) => request.method === "POST")
  expect(await added?.json()).toEqual({ url: pull.url })
})

it("filters, refreshes, and removes through the same routes as the CLI", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname === "/v1/repositories")
      return jsonResponse([aRepository({ id: "repo", forge: aForge({ enabled: true }) })])
    if (request.method === "POST") return new Response(null, { status: 202 })
    if (request.method === "DELETE") return new Response(null, { status: 204 })
    return jsonResponse([pull])
  })
  renderScreen(<PullRequestsPage />)
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  await waitFor(() =>
    expect(
      screen.getByLabelText("Repository").querySelector('option[value="repo"]'),
    ).not.toBeNull(),
  )
  await userEvent.selectOptions(screen.getByLabelText("Repository"), "repo")
  await userEvent.selectOptions(screen.getByLabelText("Role"), "author")
  await userEvent.click(screen.getByLabelText("Include closed and merged"))
  await userEvent.click(screen.getByRole("button", { name: "Refresh" }))
  await userEvent.click(screen.getByRole("button", { name: "Remove" }))
  await waitFor(() => expect(requests.some((r) => r.method === "DELETE")).toBe(true))
  expect(
    requests.some(
      (r) =>
        new URL(r.url).searchParams.get("state") === "all" &&
        new URL(r.url).searchParams.get("role") === "author",
    ),
  ).toBe(true)
  expect(
    requests.some(
      (r) =>
        r.method === "POST" &&
        new URL(r.url).pathname === "/v1/pull-requests/refresh" &&
        new URL(r.url).searchParams.get("repo") === "repo",
    ),
  ).toBe(true)
  expect(
    requests.some(
      (r) => r.method === "DELETE" && new URL(r.url).pathname === "/v1/pull-requests/pull-42",
    ),
  ).toBe(true)
})

it("opens a request's session panel from its row and shows its unanswered comments", async () => {
  const row = { ...pull, session_id: "01JSESS00000000000000PULL1" }
  daemonFetch.mockImplementation(async (input) => {
    const path = new URL((input as Request).url).pathname
    if (path === "/v1/pull-requests")
      return jsonResponse([row, { ...pull, id: "pull-7", number: 7 }])
    return jsonResponse([])
  })
  const { location } = renderScreen(<PullRequestsPage />, { route: "/pull-requests?pr=pull-42" })
  const link = await screen.findByRole("link", { name: "Open the session of #42" })
  // A request with no session yet has nothing to open.
  expect(screen.queryByRole("link", { name: "Open the session of #7" })).toBeNull()
  expect(link.closest("tr")?.textContent).toContain(String(pull.unanswered_comments))
  await userEvent.click(link)
  expect(location.url).toBe(`/pull-requests?session=${row.session_id}`)
})

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
    { route: "/forge/pull-requests" },
  )
  expect((await screen.findByRole("link", { name: "#42 Fix widgets" })).getAttribute("href")).toBe(
    pull.url,
  )
  row = { ...pull, checks: "success" }
  act(() => dispatchDomainEvent(queryClient, { event: "pull_request_updated", data: row }))
  expect(await screen.findByText("Passing")).toBeTruthy()
  // The row holds no button: a click on it opens the panel.
  expect(screen.queryByRole("button", { name: /#42/ })).toBeNull()
  await userEvent.click(screen.getByRole("row", { name: "Open #42 Fix widgets" }))
  expect(location.url).toBe("/forge/pull-requests?pr=pull-42")
  expect(await screen.findByText("Unanswered comments")).toBeTruthy()
})

it("lists every open request by default, narrows to mine or to review requests, and adds none by hand", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    return jsonResponse([pull])
  })
  const { location } = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  expect(screen.queryByRole("button", { name: "Add pull request" })).toBeNull()
  expect(screen.queryByRole("switch", { name: "Include closed and merged" })).toBeNull()
  const lists = () =>
    requests
      .filter((r) => new URL(r.url).pathname === "/v1/pull-requests")
      .map((r) => new URL(r.url).searchParams)
  const roles = () => lists().map((params) => params.get("role"))
  expect(roles()).toContain(null)
  expect(screen.getByRole("button", { name: "All" }).getAttribute("aria-pressed")).toBe("true")

  await userEvent.click(screen.getByRole("button", { name: "Mine" }))
  await waitFor(() => expect(roles()).toContain("author"))
  expect(location.url).toBe("/forge/pull-requests?role=author")
  await userEvent.click(screen.getByRole("button", { name: "Review requests" }))
  await waitFor(() =>
    expect(lists().some((p) => p.get("role") === "reviewer" && p.get("requested") === "true")).toBe(
      true,
    ),
  )
  // Open requests alone: nothing asks for the closed and merged ones.
  expect(requests.every((r) => new URL(r.url).searchParams.get("state") === null)).toBe(true)
})

it("filters and refreshes through the same routes as the CLI", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname === "/v1/repositories")
      return jsonResponse([aRepository({ id: "repo", forge: aForge({ enabled: true }) })])
    if (request.method === "POST") return new Response(null, { status: 202 })
    return jsonResponse([pull])
  })
  renderScreen(<PullRequestsPage />)
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  await userEvent.click(screen.getByRole("combobox", { name: "Filter by repository" }))
  await userEvent.click(await screen.findByRole("option", { name: "acme/widgets" }))
  await userEvent.click(screen.getByRole("button", { name: "Mine" }))
  await userEvent.click(screen.getByRole("button", { name: "Refresh" }))
  await waitFor(() => expect(requests.some((r) => r.method === "POST")).toBe(true))
  expect(
    requests.some(
      (r) =>
        new URL(r.url).searchParams.get("repo") === "repo" &&
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
})

it("opens a request's session panel from its row and shows its unanswered comments", async () => {
  const row = { ...pull, session_id: "01JSESS00000000000000PULL1" }
  daemonFetch.mockImplementation(async (input) => {
    const path = new URL((input as Request).url).pathname
    if (path === "/v1/pull-requests")
      return jsonResponse([row, { ...pull, id: "pull-7", number: 7 }])
    return jsonResponse([])
  })
  const { location } = renderScreen(<PullRequestsPage />, {
    route: "/forge/pull-requests?pr=pull-42",
  })
  const link = await screen.findByRole("link", { name: "Open the session of #42" })
  // A request with no session yet has nothing to open.
  expect(screen.queryByRole("link", { name: "Open the session of #7" })).toBeNull()
  expect(link.closest("tr")?.textContent).toContain(String(pull.unanswered_comments))
  await userEvent.click(link)
  expect(location.url).toBe(`/forge/pull-requests?session=${row.session_id}`)
})

it("narrows the rows to the words of their title or description", async () => {
  const other = { ...pull, id: "pull-7", number: 7, title: "Add gadgets", body: "Ships gadgets." }
  daemonFetch.mockImplementation(async () => jsonResponse([pull, other]))
  const { location } = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#7 Add gadgets" })

  await userEvent.type(screen.getByRole("searchbox", { name: "Filter by text" }), "empty list")
  await waitFor(() => expect(screen.queryByRole("link", { name: "#7 Add gadgets" })).toBeNull())
  expect(screen.getByRole("link", { name: "#42 Fix widgets" })).toBeTruthy()
  expect(location.url).toBe("/forge/pull-requests?q=empty+list")
})

// @vitest-environment jsdom
import { act, fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { DetailPanels } from "@/components/detail-panels"
import { dispatchDomainEvent } from "@/events/dispatch"
import { aForge, aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"
import { PullRequestsPage } from "./pull-requests-page"

/** One page of `GET /v1/pull-requests`: the last one where `next` is null. */
function page(rows: (typeof pull)[], next: string | null = null) {
  return jsonResponse({ pull_requests: rows, next_cursor: next })
}

/**
 * An `IntersectionObserver` stand-in: jsdom has none. `reach()` tells the
 * newest observer that its mark under the table scrolled into view.
 */
function stubObserver() {
  type Callback = (entries: { isIntersecting: boolean }[]) => void
  const observers: Callback[] = []
  let made = 0
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      callback: Callback
      constructor(callback: Callback) {
        this.callback = callback
        observers.push(callback)
        made += 1
      }
      observe() {}
      disconnect() {
        observers.splice(observers.indexOf(this.callback), 1)
      }
    },
  )
  return {
    reach: () => act(() => observers.at(-1)?.([{ isIntersecting: true }])),
    count: () => made,
  }
}

/** The search params of every `GET /v1/pull-requests` sent. */
function listsOf(requests: Request[]) {
  return requests
    .filter((r) => new URL(r.url).pathname === "/v1/pull-requests")
    .map((r) => new URL(r.url).searchParams)
}

it("renders rows, updates from an event, and opens the floating panel", async () => {
  let row = pull
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (path === "/v1/repositories")
      return jsonResponse([aRepository({ id: "repo", forge: aForge({ enabled: true }) })])
    if (path === "/v1/pull-requests") return page([row])
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
  act(() =>
    dispatchDomainEvent(queryClient, {
      event: "pull_requests_changed",
      data: { repository_id: "repo" },
    }),
  )
  expect(await screen.findByText("Passing")).toBeTruthy()
  // The row holds no button: a click on it opens the panel.
  expect(screen.queryByRole("button", { name: /#42/ })).toBeNull()
  await userEvent.click(screen.getByRole("row", { name: "Open #42 Fix widgets" }))
  expect(location.url).toBe("/forge/pull-requests?pr=repo%3A42")
  expect(await screen.findByText("Unanswered comments")).toBeTruthy()
})

it("shows the Ariadne babysitting pill on a row a keeper session babysits by hand", async () => {
  const row = { ...pull, role: "author", author_login: "me", babysit_asked: true }
  daemonFetch.mockImplementation(async (input) => {
    const path = new URL((input as Request).url).pathname
    if (path === "/v1/pull-requests") return page([row])
    return jsonResponse([])
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  expect(await screen.findByText("Ariadne babysitting")).toBeTruthy()
})

it("lists every open request by default, narrows to mine or to review requests, and adds none by hand", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname === "/v1/pull-requests") return page([pull])
    return jsonResponse([pull])
  })
  const { location } = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  expect(screen.queryByRole("button", { name: "Add pull request" })).toBeNull()
  expect(screen.queryByRole("switch", { name: "Include closed and merged" })).toBeNull()
  const lists = () => listsOf(requests)
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
    if (new URL(request.url).pathname === "/v1/pull-requests") return page([pull])
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
    if (path === "/v1/pull-requests") return page([row, { ...pull, id: "pull-7", number: 7 }])
    return jsonResponse([])
  })
  const { location } = renderScreen(<PullRequestsPage />, {
    route: "/forge/pull-requests?pr=repo%3A42",
  })
  const link = await screen.findByRole("link", { name: "Open the session of #42" })
  // A request with no session yet has nothing to open.
  expect(screen.queryByRole("link", { name: "Open the session of #7" })).toBeNull()
  expect(link.closest("tr")?.textContent).toContain(String(pull.unanswered_comments))
  await userEvent.click(link)
  expect(location.url).toBe(`/forge/pull-requests?session=${row.session_id}`)
})

it("sends the text filter to the daemon and shows every matching row across pages", async () => {
  const observer = stubObserver()
  const other = { ...pull, id: "pull-7", number: 7, title: "Add gadgets", body: "Ships gadgets." }
  const third = { ...pull, id: "pull-3", number: 3, title: "Fix more widgets" }
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname !== "/v1/pull-requests") return jsonResponse([])
    const params = new URL(request.url).searchParams
    if (!params.get("q")) return page([pull, other])
    // The daemon filters every request; the matches span two pages.
    return params.get("cursor") === "widgets-2" ? page([third]) : page([pull], "widgets-2")
  })
  const { location } = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#7 Add gadgets" })

  fireEvent.change(screen.getByRole("searchbox", { name: "Filter by text" }), {
    target: { value: "widgets" },
  })
  await waitFor(() => expect(screen.queryByRole("link", { name: "#7 Add gadgets" })).toBeNull())
  expect(listsOf(requests).some((params) => params.get("q") === "widgets")).toBe(true)
  expect(location.url).toBe("/forge/pull-requests?q=widgets")
  observer.reach()
  expect(await screen.findByRole("link", { name: "#3 Fix more widgets" })).toBeTruthy()
  expect(screen.getByRole("link", { name: "#42 Fix widgets" })).toBeTruthy()
  expect(
    listsOf(requests).some((p) => p.get("q") === "widgets" && p.get("cursor") === "widgets-2"),
  ).toBe(true)
})

it("loads the next page near the end, and an event reads the list again from its first page", async () => {
  const observer = stubObserver()
  const seven = { ...pull, id: "pull-7", number: 7, title: "Add gadgets" }
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname !== "/v1/pull-requests") return jsonResponse([])
    return new URL(request.url).searchParams.get("cursor") === "page-2"
      ? page([seven])
      : page([pull], "page-2")
  })
  const { queryClient } = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  // The first page alone, until the table scrolls near its end.
  expect(screen.queryByRole("link", { name: "#7 Add gadgets" })).toBeNull()
  expect(listsOf(requests).map((p) => p.get("cursor"))).toEqual([null])

  observer.reach()
  expect(await screen.findByRole("link", { name: "#7 Add gadgets" })).toBeTruthy()
  // The last page has no cursor, so nothing is left to watch for.
  const observers = observer.count()
  observer.reach()
  expect(observer.count()).toBe(observers)

  requests.length = 0
  act(() =>
    dispatchDomainEvent(queryClient, {
      event: "pull_requests_changed",
      data: { repository_id: "repo" },
    }),
  )
  await waitFor(() => expect(listsOf(requests).map((p) => p.get("cursor"))[0]).toBe(null))
})

it("stops loading pages by itself once the next page fails", async () => {
  const observer = stubObserver()
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    requests.push(request)
    if (new URL(request.url).pathname !== "/v1/pull-requests") return jsonResponse([])
    if (new URL(request.url).searchParams.get("cursor") === "page-2")
      return jsonResponse({ error: { code: "internal", message: "boom" } }, 500)
    return page([pull], "page-2")
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("link", { name: "#42 Fix widgets" })

  observer.reach()
  // The first page stays on screen, so the failed page reads as a failed refresh.
  expect(await screen.findByText("Could not refresh pull requests")).toBeTruthy()
  // The failed page leaves no live observer: the mark cannot ask again by itself.
  await act(async () => {})
  observer.reach()
  await act(async () => {})
  expect(listsOf(requests).map((p) => p.get("cursor"))).toEqual([null, "page-2"])
})

it("offers a refresh only while a repository in view polls or its webhook is down", async () => {
  const live = aRepository({
    id: "live",
    forge: aForge({ enabled: true, webhook: { ...aForge().webhook, state: "live" } }),
  })
  const polled = aRepository({
    id: "polled",
    forge: aForge({ enabled: true, name: "gadgets" }),
  })
  daemonFetch.mockImplementation(async (input) => {
    const path = new URL((input as Request).url).pathname
    if (path === "/v1/repositories") return jsonResponse([live, polled])
    if (path === "/v1/forge/tunnel")
      return jsonResponse({ enabled: true, state: "up", url: null, listen: null, since: null })
    if (path === "/v1/pull-requests") return page([pull])
    return jsonResponse([pull])
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests?repo=live" })
  await screen.findByRole("link", { name: "#42 Fix widgets" })
  // The live webhook brings every change on its own.
  await waitFor(() => expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull())

  await userEvent.click(screen.getByRole("combobox", { name: "Filter by repository" }))
  await userEvent.click(await screen.findByRole("option", { name: "acme/gadgets" }))
  expect(await screen.findByRole("button", { name: "Refresh" })).toBeTruthy()
})

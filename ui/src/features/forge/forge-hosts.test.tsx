// @vitest-environment jsdom

/** The Forge tabs over a settings daemon and a named host. */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { DetailPanels } from "@/components/detail-panels"
import { HostPicker } from "@/components/host-picker"
import { IssuesPage } from "@/features/issues/issues-page"
import { PullRequestsPage } from "@/features/pull-requests/pull-requests-page"
import { useSettingsStore } from "@/stores/settings"
import { aForge, aModel, aRepository } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { pull } from "@/test/pull-request"

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

const forge = aForge({ enabled: true })
const LOCAL_REPO = aRepository({ id: "repo-local", forge })
const BOX_REPO = aRepository({ id: "repo-box", forge })
// Another repository, which only `local` has registered.
const ONLY_LOCAL = aRepository({
  id: "repo-only",
  path: "/home/me/dev/only",
  forge: aForge({ enabled: true, name: "only" }),
})

const page = (rows: unknown[]) => ({ pull_requests: rows, next_cursor: null })

const localPull = { ...pull, id: "pull-local", repository_id: "repo-local", title: "Local fix" }
const boxPull = {
  ...pull,
  id: "pull-box",
  repository_id: "repo-box",
  title: "Box fix",
  updated_at: "2026-10-08T01:00:00Z",
}

it("merges the pull requests of every host under All, and opens a panel on its row's host", async () => {
  // Another request than local's, so each is a row of its own.
  const other = { ...boxPull, number: 43 }
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": page([localPull]),
      "/v1/pull-requests/pull-local": localPull,
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([other]),
      "/v1/pull-requests/pull-box": other,
      "/v1/repositories/repo-box/pull-requests/43": other,
    },
  })
  const { location } = renderScreen(
    <>
      <PullRequestsPage />
      <DetailPanels />
    </>,
    { route: "/forge/pull-requests" },
  )

  const boxRow = await screen.findByRole("row", { name: "Open #43 Box fix" })
  await screen.findByRole("row", { name: "Open #42 Local fix" })
  // Most recently updated first, whichever host it is on.
  expect(screen.getAllByRole("row").map((row) => row.getAttribute("aria-label"))).toEqual([
    null,
    "Open #43 Box fix",
    "Open #42 Local fix",
  ])

  await userEvent.click(boxRow)
  expect(location.url).toContain("host=box")
  expect(await screen.findByText("Unanswered comments")).toBeTruthy()
})

it("narrows to one repository by its forge name on every host, and skips a host without it", async () => {
  const seen = stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO, ONLY_LOCAL],
      "/v1/pull-requests": page([localPull]),
    },
    box: { "/v1/repositories": [BOX_REPO], "/v1/pull-requests": page([boxPull]) },
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests?repo=repo-local" })

  await screen.findByRole("row", { name: "Open #42 Box fix" })
  const lists = seen.filter((request) => request.url.pathname === "/v1/pull-requests")
  expect(lists.map((request) => [request.origin, request.url.searchParams.get("repo")])).toEqual(
    expect.arrayContaining([
      ["http://127.0.0.1:7676", "repo-local"],
      ["http://box.test:7676", "repo-box"],
    ]),
  )

  // A repository only `local` has is read on `local` alone.
  seen.length = 0
  const view = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests?repo=repo-only" })
  await waitFor(() =>
    expect(seen.some((request) => request.url.pathname === "/v1/pull-requests")).toBe(true),
  )
  expect(
    seen
      .filter((request) => request.url.pathname === "/v1/pull-requests")
      .every((request) => request.origin === "http://127.0.0.1:7676"),
  ).toBe(true)
  view.queryClient.clear()
})

it("offers the hosts where the repository is registered for Start review, the row's host first", async () => {
  const seen = stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests/pull-box": boxPull,
      "/v1/models": [],
      "/v1/skills": [],
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests/pull-box": boxPull,
      "/v1/models": [],
      "/v1/skills": [],
      "/v1/repositories/repo-box/pull-requests/42/ariadne-review": (_url: URL, request: Request) =>
        request.method === "PUT" ? jsonResponse({ ...boxPull, review_asked: true }) : undefined,
    },
  })
  // The header's host picker is what reads the hosts list in the app.
  renderScreen(
    <>
      <HostPicker />
      <DetailPanels />
    </>,
    { route: "/forge/pull-requests?pr=pull-box&host=box" },
  )

  await userEvent.click(await screen.findByRole("button", { name: "Start review" }))
  const select = await screen.findByRole("combobox", { name: "Host" })
  expect(select.textContent).toContain("box")
  await userEvent.click(select)
  expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([
    "box",
    "local",
  ])
  const modelReads = () =>
    seen.filter((request) => request.url.pathname === "/v1/models").map((request) => request.origin)
  expect(modelReads()).toEqual(["http://box.test:7676"])
  await userEvent.click(screen.getByRole("option", { name: "local" }))
  await waitFor(() => expect(modelReads()).toContain("http://127.0.0.1:7676"))
})

it("merges the issues of every host, and names the host of each", async () => {
  const issue = (title: string) => ({
    number: 7,
    title,
    body: "",
    labels: [],
    assignees: [],
    url: `https://github.com/acme/widgets/issues/${title}`,
    updated_at: "2026-10-07T01:00:00Z",
  })
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/repositories/repo-local/issues": [issue("local-issue")],
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/repositories/repo-box/issues": [{ ...issue("box-issue"), number: 8 }],
    },
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  const boxIssue = await screen.findByRole("link", { name: /box-issue/ })
  expect(await screen.findByRole("link", { name: /local-issue/ })).toBeTruthy()
  const row = boxIssue.closest("tr") as HTMLElement
  expect(within(row).getByTitle("Host").textContent).toBe("box")
})

const anIssue = (title: string) => ({
  number: 7,
  title,
  body: "",
  labels: [],
  assignees: [],
  url: `https://github.com/acme/widgets/issues/${title}`,
  updated_at: "2026-10-07T01:00:00Z",
})

it("names local on a local issue row under All, and keeps it when a named host goes down", async () => {
  stubHosts(
    {
      local: {
        "/v1/repositories": [LOCAL_REPO],
        "/v1/repositories/repo-local/issues": [anIssue("local-issue")],
      },
    },
    [LOCAL, { ...BOX, status: "down", error: "connection refused" }],
  )
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  const row = (await screen.findByRole("link", { name: /local-issue/ })).closest(
    "tr",
  ) as HTMLElement
  await waitFor(() => expect(within(row).getByTitle("Host").textContent).toBe("local"))
})

it("names no host on an issue row under one picked host", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  stubHosts({
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/repositories/repo-box/issues": [anIssue("box-issue")],
    },
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  const row = (await screen.findByRole("link", { name: /box-issue/ })).closest("tr") as HTMLElement
  expect(within(row).queryByTitle("Host")).toBeNull()
})

it("shows no request of the host picked before when the newly picked host fails", async () => {
  stubHosts({
    local: { "/v1/repositories": [LOCAL_REPO], "/v1/pull-requests": page([localPull]) },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": () =>
        new Response(JSON.stringify({ error: { code: "boom", message: "box is broken" } }), {
          status: 500,
          headers: { "content-type": "application/json" },
        }),
    },
  })
  useSettingsStore.setState({ hostFilter: "local" })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await screen.findByRole("row", { name: "Open #42 Local fix" })

  act(() => useSettingsStore.setState({ hostFilter: "box" }))

  expect(await screen.findByText("Could not load pull requests")).toBeTruthy()
  expect(screen.queryByRole("row", { name: "Open #42 Local fix" })).toBeNull()
})

it("keeps the filtered repository when another host, with another id for it, is picked", async () => {
  const seen = stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": page([localPull]),
      "/v1/repositories/repo-local/issues": [],
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([boxPull]),
      "/v1/repositories/repo-box/issues": [],
    },
  })
  // `local` is picked first, so the box's requests are only read once it is picked.
  useSettingsStore.setState({ hostFilter: "local" })
  const view = renderScreen(
    <>
      <PullRequestsPage />
      <IssuesPage />
    </>,
    { route: "/forge/pull-requests?repo=repo-local" },
  )
  await screen.findByRole("row", { name: "Open #42 Local fix" })
  expect(
    seen.some(
      (request) =>
        request.origin === "http://box.test:7676" &&
        request.url.pathname.endsWith("/pull-requests"),
    ),
  ).toBe(false)

  act(() => useSettingsStore.setState({ hostFilter: "box" }))

  await waitFor(() => {
    const onBox = seen.filter((request) => request.origin === "http://box.test:7676")
    // Pull requests read the box's own id for the repository...
    expect(
      onBox
        .filter((request) => request.url.pathname === "/v1/pull-requests")
        .map((request) => request.url.searchParams.get("repo")),
    ).toEqual(["repo-box"])
    // ...and so do the issues, which are read per repository.
    expect(
      onBox.some((request) => request.url.pathname === "/v1/repositories/repo-box/issues"),
    ).toBe(true)
  })
  // The filter still names the repository, in both tabs.
  for (const trigger of screen.getAllByRole("combobox", { name: "Filter by repository" })) {
    expect(trigger.textContent).toContain("acme/widgets")
  }
  view.queryClient.clear()
})

it("lists the issues of every host most recently updated first", async () => {
  const issue = (title: string, updated_at: string) => ({
    number: 7,
    title,
    body: "",
    labels: [],
    assignees: [],
    url: `https://github.com/acme/widgets/issues/${title}`,
    updated_at,
  })
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/repositories/repo-local/issues": [issue("older-local", "2026-10-01T00:00:00Z")],
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/repositories/repo-box/issues": [
        { ...issue("newer-box", "2026-10-09T00:00:00Z"), number: 8 },
      ],
    },
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  await screen.findByRole("link", { name: /older-local/ })
  expect(
    screen.getAllByRole("link", { name: /-(local|box)$/ }).map((link) => link.textContent),
  ).toEqual(["#8 newer-box", "#7 older-local"])
})

it("does not wait for a session on the panel's host when Start review picks another host", async () => {
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/models": [aModel({ id: "stub:review-model" })],
      "/v1/skills": [],
      "/v1/repositories/repo-local/pull-requests/42/ariadne-review": (
        _url: URL,
        request: Request,
      ) =>
        request.method === "PUT" ? jsonResponse({ ...localPull, review_asked: true }) : undefined,
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests/pull-box": boxPull,
      "/v1/sessions": [],
    },
  })
  renderScreen(
    <>
      <HostPicker />
      <DetailPanels />
    </>,
    { route: "/forge/pull-requests?pr=pull-box&host=box" },
  )

  await userEvent.click(await screen.findByRole("button", { name: "Start review" }))
  const dialog = await screen.findByRole("dialog", { name: "Start an Ariadne review" })
  await userEvent.click(within(dialog).getByRole("combobox", { name: "Host" }))
  await userEvent.click(await screen.findByRole("option", { name: "local" }))
  await userEvent.click(within(dialog).getByRole("button", { name: "Model" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await userEvent.click(within(models).getByText("stub:review-model"))
  await userEvent.keyboard("{Escape}")
  await userEvent.click(within(dialog).getByRole("button", { name: "Start review" }))

  expect(await screen.findByText(/The session started on host local/)).toBeTruthy()
  expect(screen.queryByText(/Starting the session/)).toBeNull()
})

/** The same request as both hosts list it: each under its own repository id and its own facts. */
const onLocal = { ...pull, id: null, repository_id: "repo-local", title: "Old title" }
const onBox = {
  ...pull,
  id: null,
  repository_id: "repo-box",
  title: "New title",
  checks: "failure",
  updated_at: "2026-10-09T00:00:00Z",
}

it("shows one row for a request two hosts list, with the newest forge facts", async () => {
  stubHosts({
    local: { "/v1/repositories": [LOCAL_REPO], "/v1/pull-requests": page([onLocal]) },
    box: { "/v1/repositories": [BOX_REPO], "/v1/pull-requests": page([onBox]) },
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })

  const row = await screen.findByRole("row", { name: "Open #42 New title" })
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(2))
  expect(screen.queryByRole("row", { name: "Open #42 Old title" })).toBeNull()
  expect(within(row).getByText("Failing")).toBeTruthy()
  // Nobody works on it, so the row names no host.
  expect(within(row).queryByTitle("Host")).toBeNull()
})

it("names the host of each Ariadne review and each keeper on the one row", async () => {
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": page([{ ...onLocal, review_asked: true }]),
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([{ ...onBox, babysit_asked: true }]),
    },
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })

  const row = await screen.findByRole("row", { name: "Open #42 New title" })
  await waitFor(() => expect(within(row).getAllByTitle("Host")).toHaveLength(2))
  const review = within(row).getByText("Ariadne reviewing").closest("[data-work]") as HTMLElement
  const keeper = within(row).getByText("Ariadne babysitting").closest("[data-work]") as HTMLElement
  expect(within(review).getByTitle("Host").textContent).toBe("local")
  expect(within(keeper).getByTitle("Host").textContent).toBe("box")
})

it("links the session of each host that has one, each on its host", async () => {
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": page([{ ...onLocal, session_id: "session-local" }]),
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([{ ...onBox, session_id: "session-box" }]),
    },
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })

  const row = await screen.findByRole("row", { name: "Open #42 New title" })
  const box = await within(row).findByRole("link", { name: "Open the session of #42 on box" })
  const local = within(row).getByRole("link", { name: "Open the session of #42 on local" })
  expect(box.getAttribute("href")).toContain("session=session-box")
  expect(box.getAttribute("href")).toContain("host=box")
  expect(local.getAttribute("href")).toContain("session=session-local")
  expect(local.getAttribute("href")).not.toContain("host=")
})

it("opens the panel on the host that holds Ariadne work", async () => {
  stubHosts({
    local: { "/v1/repositories": [LOCAL_REPO], "/v1/pull-requests": page([onLocal]) },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([{ ...onBox, review_asked: true }]),
    },
  })
  const view = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  const row = await screen.findByRole("row", { name: "Open #42 New title" })
  await waitFor(() => expect(within(row).getByTitle("Host").textContent).toBe("box"))
  await userEvent.click(row)
  expect(view.location.url).toContain("pr=repo-box%3A42")
  expect(view.location.url).toContain("host=box")
})

it("opens the panel on local when no host holds Ariadne work", async () => {
  stubHosts({
    local: { "/v1/repositories": [LOCAL_REPO], "/v1/pull-requests": page([onLocal]) },
    box: { "/v1/repositories": [BOX_REPO], "/v1/pull-requests": page([onBox]) },
  })
  const again = renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(2))
  await userEvent.click(screen.getByRole("row", { name: "Open #42 New title" }))
  expect(again.location.url).toContain("pr=repo-local%3A42")
  expect(again.location.url).not.toContain("host=")
})

it("sends the filters to every host and lists a request on a later page of one host once", async () => {
  type Callback = (entries: { isIntersecting: boolean }[]) => void
  const observers: Callback[] = []
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: Callback) {
        observers.push(callback)
      }
      observe() {}
      disconnect() {}
    },
  )
  const seven = { ...onLocal, number: 7, title: "Seven" }
  const seen = stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": (url: URL) =>
        jsonResponse(
          url.searchParams.get("cursor") === "page-2"
            ? page([onLocal])
            : { pull_requests: [seven], next_cursor: "page-2" },
        ),
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([onBox, { ...seven, repository_id: "repo-box" }]),
    },
  })
  renderScreen(<PullRequestsPage />, {
    route: "/forge/pull-requests?repo=repo-local&role=author&q=title",
  })
  await screen.findByRole("row", { name: "Open #42 New title" })
  expect(
    seen
      .filter((request) => request.url.pathname === "/v1/pull-requests")
      .map((request) => [
        request.origin,
        request.url.searchParams.get("repo"),
        request.url.searchParams.get("role"),
        request.url.searchParams.get("q"),
      ]),
  ).toEqual(
    expect.arrayContaining([
      ["http://127.0.0.1:7676", "repo-local", "author", "title"],
      ["http://box.test:7676", "repo-box", "author", "title"],
    ]),
  )

  act(() => observers.at(-1)?.([{ isIntersecting: true }]))
  await waitFor(() =>
    expect(seen.some((request) => request.url.searchParams.get("cursor") === "page-2")).toBe(true),
  )
  await waitFor(() =>
    expect(screen.getAllByRole("row").map((row) => row.getAttribute("aria-label"))).toEqual([
      null,
      "Open #42 New title",
      "Open #7 Seven",
    ]),
  )
  vi.unstubAllGlobals()
})

it("keeps the rows of the other host and the error of the host that fails", async () => {
  stubHosts({
    local: { "/v1/repositories": [LOCAL_REPO], "/v1/pull-requests": page([onLocal]) },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": () =>
        jsonResponse({ error: { code: "boom", message: "box is broken" } }, 500),
    },
  })
  renderScreen(<PullRequestsPage />, { route: "/forge/pull-requests" })

  expect(await screen.findByRole("row", { name: "Open #42 Old title" })).toBeTruthy()
  expect(await screen.findByText(/box is broken/)).toBeTruthy()
})

it("shows one row for an issue two hosts list, with its newest facts", async () => {
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/repositories/repo-local/issues": [anIssue("old")],
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/repositories/repo-box/issues": [
        { ...anIssue("new"), updated_at: "2026-10-09T00:00:00Z" },
      ],
    },
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  await screen.findByRole("link", { name: /new$/ })
  await waitFor(() =>
    expect(screen.getAllByRole("button", { name: "Create goal" })).toHaveLength(1),
  )
  expect(screen.queryByRole("link", { name: /old$/ })).toBeNull()
})

const LOCAL_CHECKOUT = { ...LOCAL_REPO, path: "/local/widgets" }
const BOX_CHECKOUT = { ...BOX_REPO, path: "/box/widgets" }

it.each([
  // The picked host has the repository.
  ["box", [LOCAL_CHECKOUT], "box", "/box/widgets"],
  // Under All, `local` comes first.
  ["", [LOCAL_CHECKOUT], "local", "/local/widgets"],
  // Without it on `local`, the first host that has it.
  ["", [], "box", "/box/widgets"],
])(
  "opens Create goal under the pick %j on a host with the repository",
  async (pick, onLocalHost, host, path) => {
    useSettingsStore.setState({ hostFilter: pick })
    const issues = [anIssue("issue")]
    stubHosts({
      local: {
        "/v1/repositories": onLocalHost,
        "/v1/repositories/repo-local/issues": issues,
        "/v1/workflows": [],
      },
      box: {
        "/v1/repositories": [BOX_CHECKOUT],
        "/v1/repositories/repo-box/issues": issues,
        "/v1/workflows": [],
      },
    })
    renderScreen(<IssuesPage />, { route: "/forge/issues" })

    await screen.findByRole("link", { name: /issue$/ })
    await waitFor(() =>
      expect(screen.getAllByRole("button", { name: "Create goal" })).toHaveLength(1),
    )
    await userEvent.click(screen.getByRole("button", { name: "Create goal" }))
    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByRole("combobox", { name: "Host" }).textContent).toContain(host)
    expect(await within(dialog).findByRole("button", { name: `Remove ${path}` })).toBeTruthy()
  },
)

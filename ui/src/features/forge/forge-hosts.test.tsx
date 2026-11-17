// @vitest-environment jsdom

/** The Forge tabs over a settings daemon and a named host. */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"
import { DetailPanels } from "@/components/detail-panels"
import { HostPicker } from "@/components/host-picker"
import { IssuesPage } from "@/features/issues/issues-page"
import { PullRequestsPage } from "@/features/pull-requests/pull-requests-page"
import { useSettingsStore } from "@/stores/settings"
import { aForge, aRepository } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
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

it("merges the pull requests of every host under All, badged, and opens a panel on its row's host", async () => {
  stubHosts({
    local: {
      "/v1/repositories": [LOCAL_REPO],
      "/v1/pull-requests": page([localPull]),
      "/v1/pull-requests/pull-local": localPull,
    },
    box: {
      "/v1/repositories": [BOX_REPO],
      "/v1/pull-requests": page([boxPull]),
      "/v1/pull-requests/pull-box": boxPull,
      "/v1/repositories/repo-box/pull-requests/42": boxPull,
    },
  })
  const { location } = renderScreen(
    <>
      <PullRequestsPage />
      <DetailPanels />
    </>,
    { route: "/forge/pull-requests" },
  )

  const boxRow = await screen.findByRole("row", { name: "Open #42 Box fix" })
  const localRow = screen.getByRole("row", { name: "Open #42 Local fix" })
  expect(within(boxRow).getByText("box")).toBeTruthy()
  expect(within(localRow).getByText("local")).toBeTruthy()
  // Most recently updated first, whichever host it is on.
  expect(screen.getAllByRole("row").map((row) => row.getAttribute("aria-label"))).toEqual([
    null,
    "Open #42 Box fix",
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

it("merges the issues of every host, and fills the goal dialog on the issue's host", async () => {
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
      "/v1/repositories/repo-box/issues": [issue("box-issue")],
    },
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  const boxIssue = await screen.findByRole("link", { name: /box-issue/ })
  expect(await screen.findByRole("link", { name: /local-issue/ })).toBeTruthy()
  const row = boxIssue.closest("tr") as HTMLElement
  expect(within(row).getByText("box")).toBeTruthy()
  await userEvent.click(within(row).getByRole("button", { name: "Create goal" }))
  const host = await screen.findByRole("combobox", { name: "Host" })
  expect(host.textContent).toContain("box")
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

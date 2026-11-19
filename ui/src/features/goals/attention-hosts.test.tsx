// @vitest-environment jsdom

/**
 * The attention list across hosts: every reachable host is read whatever the
 * header's pick, a toast of a named host names it, one host's error names that
 * host and leaves the others' items and toasts alone, and a host's first
 * answer is never news.
 */

import { fireEvent, screen, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { AttentionItemDto, HostDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { useSettingsStore } from "@/stores/settings"
import { aSessionPage } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

import { AttentionAlerts } from "./attention-alerts"
import { AttentionPopover } from "./attention-strip"

function anItem(id: string, overrides: Partial<AttentionItemDto> = {}): AttentionItemDto {
  return {
    id,
    occurrence: "",
    producer: "recovery",
    reason: "resource",
    summary: `Summary of ${id}`,
    required_action: "Free file descriptors.",
    since: "2026-11-17T10:00:00Z",
    affected: [{ kind: "task", id: "01OTHER", label: "another task" }],
    target: { kind: "task", task_id: "01OTHER" },
    ...overrides,
  }
}

const MINE = anItem("local-item")
const THEIRS = anItem("box-item")

/** What each daemon answers for its attention list right now. */
let local: AttentionItemDto[]
let box: AttentionItemDto[] | "fail"
let hosts: HostDto[]

const EMPTY = { "/v1/goals": [], "/v1/tasks": [], "/v1/sessions": aSessionPage([]) }

function stub() {
  stubHosts(
    {
      local: {
        ...EMPTY,
        "/v1/hosts": () => jsonResponse(hosts),
        "/v1/attention": () => jsonResponse({ items: local, complete: true }),
      },
      box: {
        ...EMPTY,
        "/v1/attention": () =>
          box === "fail"
            ? jsonResponse({ error: { code: "internal", message: "box is down" } }, 500)
            : jsonResponse({ items: box, complete: true }),
      },
    },
    hosts,
  )
}

beforeEach(() => {
  localStorage.clear()
  toast.dismiss()
  useSettingsStore.setState({ hostFilter: "" })
  local = []
  box = []
  hosts = [LOCAL, BOX]
  document.title = "Ariadne Desktop"
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }))
  stub()
})

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

function renderAlerts(route = "/agents") {
  return renderScreen(
    <>
      <AttentionAlerts />
      <AttentionPopover />
      <Toaster />
    </>,
    { route },
  )
}

it("counts the items of every reachable host under a pick of one", async () => {
  useSettingsStore.setState({ hostFilter: "local" })
  local = [MINE]
  box = [THEIRS]
  renderAlerts()

  await waitFor(() => expect(document.title).toBe("(2) Ariadne"))
})

it("names the host on the toast of an item of a named host", async () => {
  const { queryClient } = renderAlerts()
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))

  box = [THEIRS]
  await queryClient.invalidateQueries()

  const toastText = await screen.findByText(/Summary of box-item · box/)
  expect(toastText).toBeTruthy()
})

it("names the failed host with Retry, keeps the other items and keeps announcing", async () => {
  local = [MINE]
  box = "fail"
  const { queryClient } = renderAlerts()

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention/ }))
  expect(await screen.findByText(/Some items could not load on box/)).toBeTruthy()
  expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy()
  expect(screen.getByText(MINE.summary)).toBeTruthy()

  local = [MINE, anItem("local-news", { since: "2026-11-17T11:00:00Z" })]
  await queryClient.invalidateQueries()

  expect(
    await screen.findByText(/Summary of local-news/, { selector: "[data-title], div" }),
  ).toBeTruthy()
})

it("raises no toast for the first answer of a host that becomes listed", async () => {
  hosts = [LOCAL]
  box = [THEIRS]
  const { queryClient } = renderAlerts()
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))

  hosts = [LOCAL, BOX]
  await queryClient.invalidateQueries()

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
  expect(screen.queryByText(/Summary of box-item/)).toBeNull()
})

it("raises no toast for what a returning host raised while it was away, once its fresh answer lands", async () => {
  const { queryClient } = renderAlerts()
  // The cache outlives a host that leaves, as it does in the app.
  queryClient.setDefaultOptions({ queries: { retry: false, gcTime: 60_000, staleTime: 0 } })
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  await queryClient.invalidateQueries()

  hosts = [LOCAL]
  await queryClient.invalidateQueries()
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))

  // The host returns holding its old, empty list; its fresh answer is late.
  box = [THEIRS]
  let release: () => void = () => {}
  const late = new Promise<void>((resolve) => {
    release = resolve
  })
  stubHosts(
    {
      local: {
        ...EMPTY,
        "/v1/hosts": () => jsonResponse(hosts),
        "/v1/attention": () => jsonResponse({ items: local, complete: true }),
      },
      box: {
        ...EMPTY,
        "/v1/attention": (() =>
          late.then(() => jsonResponse({ items: box, complete: true }))) as never,
      },
    },
    [LOCAL, BOX],
  )
  hosts = [LOCAL, BOX]
  await queryClient.invalidateQueries()

  release()
  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
  expect(screen.queryByText(/Summary of box-item/)).toBeNull()
})

it("opens the agents of a named host with the pick set to All hosts", async () => {
  useSettingsStore.setState({ hostFilter: "local" })
  box = [
    anItem("box-agents", {
      target: { kind: "settings", section: "agents" },
      affected: [],
    }),
  ]
  const { location } = renderAlerts("/goals")

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention/ }))
  fireEvent.click(await screen.findByText("Summary of box-agents"))

  expect(location.url).toBe("/agents?host=box")
  expect(useSettingsStore.getState().hostFilter).toBe("")
})

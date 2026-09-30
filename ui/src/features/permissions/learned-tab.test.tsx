// @vitest-environment jsdom

/**
 * The Learned tab against a stubbed daemon.
 *
 * Two rows, from two repositories, one that allowed and one that denied: what
 * is worth pinning is that the table joins a row's `repository_id` against
 * the registry for its path in the filter, that the `repository` filter
 * narrows the list the daemon's own way, that the selected option's outcome
 * reads from the matching entry in `options`, that there is no add or edit
 * control left, that Delete still sends the contract's request and toasts a
 * refusal rather than swallow it, and that the detail view renders every
 * field — the JSON blocks included — and stays live on an `updated` event.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { LearnedPermissionDto, RepositoryDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { dispatchDomainEvent } from "@/events/dispatch"
import { shortId } from "@/lib/format"
import { aLearnedPermission, aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { LearnedPermissionsTab } from "./learned-tab"

const REPO_A: RepositoryDto = aRepository({
  id: "01JREPO00000000000000AAA",
  path: "/home/me/dev/ariadne",
})
const REPO_B: RepositoryDto = aRepository({
  id: "01JREPO00000000000000BBB",
  path: "/home/me/dev/sandbox",
})

const ALLOWED_ROW: LearnedPermissionDto = aLearnedPermission({
  id: "01JLEARN0000000000000ALW",
  repository_id: REPO_A.id,
  tool_name: "Bash",
  tool_call: { title: "Bash", kind: "execute", rawInput: { command: "ls" } },
  options: [
    { optionId: "yes", name: "Allow", kind: "allow_once" },
    { optionId: "no", name: "Deny", kind: "reject_once" },
  ],
  selected_option: "yes",
  target: "learn",
  output: null,
  created_at: "2026-09-20T10:00:00.000Z",
  updated_at: "2026-09-25T10:00:00.000Z",
})

const DENIED_ROW: LearnedPermissionDto = aLearnedPermission({
  id: "01JLEARN0000000000000DNY",
  repository_id: REPO_B.id,
  tool_name: "Write",
  tool_call: {
    title: "Write",
    kind: "edit",
    rawInput: { file_path: "/home/me/dev/sandbox/src/main.rs" },
  },
  options: [
    { optionId: "yes", name: "Allow", kind: "allow_once" },
    { optionId: "no", name: "Reject", kind: "reject_once" },
  ],
  selected_option: "no",
  target: "ai",
  output: { label: "deny", danger: 0.91 },
})

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []
let rows: LearnedPermissionDto[] = []
let deleteFailure: { status: number; code: string; message: string } | null = null

function stubDaemon() {
  requests = []
  rows = [ALLOWED_ROW, DENIED_ROW]
  deleteFailure = null

  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? (JSON.parse(raw) as Record<string, unknown>) : null
    requests.push({ method: request.method, path: url.pathname, body })

    if (url.pathname === "/v1/repositories") return jsonResponse([REPO_A, REPO_B])

    if (url.pathname === "/v1/permissions/learned") {
      if (request.method === "GET") {
        const repository = url.searchParams.get("repository")
        const items = repository ? rows.filter((row) => row.repository_id === repository) : rows
        return jsonResponse({ items })
      }
    }

    const match = /^\/v1\/permissions\/learned\/(.+)$/.exec(url.pathname)
    if (match) {
      const id = match[1]
      const row = rows.find((one) => one.id === id)
      if (request.method === "GET") {
        return row
          ? jsonResponse(row)
          : errorResponse(404, "learned_permission_not_found", "no such row")
      }
      if (request.method === "DELETE") {
        if (deleteFailure) {
          const { status, code, message } = deleteFailure
          return errorResponse(status, code, message)
        }
        rows = rows.filter((one) => one.id !== id)
        return new Response(null, { status: 204 })
      }
    }

    throw new Error(`unhandled request ${request.method} ${url.pathname}`)
  })
}

beforeEach(() => {
  stubDaemon()
})

describe("the table", () => {
  it("shows a row's tool name, target, selected option outcome, created and updated", async () => {
    renderScreen(<LearnedPermissionsTab />)

    expect(await screen.findByText("Bash")).toBeDefined()
    expect(screen.getByText("Learn")).toBeDefined()
    expect(screen.getByText("Allow")).toBeDefined()
    expect(screen.getByText("Write")).toBeDefined()
    expect(screen.getByText("AI")).toBeDefined()
    expect(screen.getByText("Reject")).toBeDefined()
    expect(screen.getByText("2 rows")).toBeDefined()

    // The Created and Updated columns, in that order: each cell is a `<time>`
    // pinned to the row's own stamp — see `components/when.tsx`.
    const row = screen.getByRole("row", { name: "Bash (Learn)" })
    const stamps = row.querySelectorAll("time")
    expect(stamps).toHaveLength(2)
    expect(stamps[0]?.getAttribute("datetime")).toBe(ALLOWED_ROW.created_at)
    expect(stamps[1]?.getAttribute("datetime")).toBe(ALLOWED_ROW.updated_at)
  })

  it("narrows the list to the repository picked in the filter", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("combobox", { name: "Filter by repository" }))
    await user.click(await screen.findByRole("option", { name: REPO_A.path }))

    await waitFor(() => expect(screen.queryByText("Write")).toBeNull())
    expect(screen.getByText("Bash")).toBeDefined()
  })

  it("shows an empty state once the filter matches nothing, with a way to clear it", async () => {
    rows = [DENIED_ROW]
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Write")

    await user.click(screen.getByRole("combobox", { name: "Filter by repository" }))
    await user.click(await screen.findByRole("option", { name: REPO_A.path }))

    expect(await screen.findByText("No rows for this repository")).toBeDefined()

    await user.click(screen.getByRole("button", { name: "Clear filter" }))

    expect(await screen.findByText("Write")).toBeDefined()
  })

  it("has no add or edit control", async () => {
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    expect(screen.queryByRole("button", { name: /add/i })).toBeNull()
    expect(screen.queryByRole("button", { name: /edit/i })).toBeNull()
  })

  it("pins the actions cell to the trailing edge", async () => {
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const actionsCell = screen.getByRole("button", { name: "Remove Bash" }).closest("td")
    expect(actionsCell).not.toBeNull()
    expect(actionsCell?.className).toContain("sticky")
    expect(actionsCell?.className).toContain("right-0")
  })

  it("opens a row's detail with the keyboard", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const row = screen.getByRole("row", { name: "Bash (Learn)" })
    row.focus()
    await user.keyboard("{Enter}")

    expect(await screen.findByText(/"command": "ls"/)).toBeDefined()
  })
})

describe("the repository filter", () => {
  it("shows the folder name on the trigger and each option, with the full path underneath and on the trigger", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const trigger = screen.getByRole("combobox", { name: "Filter by repository" })
    await user.click(trigger)
    const option = await screen.findByRole("option", { name: REPO_A.path })
    expect(within(option).getByText("ariadne")).toBeDefined()
    expect(within(option).getByText(REPO_A.path)).toBeDefined()

    await user.click(option)

    expect(trigger.title).toBe(REPO_A.path)
    expect(within(trigger).getByText("ariadne")).toBeDefined()
  })

  it("shows a readable label where the URL's repository id is not in the list", async () => {
    const missingId = "01JMISSING000000000000000"
    renderScreen(<LearnedPermissionsTab />, { route: `/permissions?repository=${missingId}` })
    await screen.findByText("No rows for this repository")

    const trigger = screen.getByRole("combobox", { name: "Filter by repository" })
    expect(within(trigger).getByText(shortId(missingId))).toBeDefined()
    expect(trigger.title).toBe(missingId)
  })
})

describe("removing a row", () => {
  it("asks for confirmation, then sends the delete", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Remove Bash" }))
    expect(screen.getByText("Bash")).toBeDefined()
    await user.click(await screen.findByRole("button", { name: "Remove row" }))

    await waitFor(() => expect(screen.queryByText("Bash")).toBeNull())
    expect(requests.some((request) => request.method === "DELETE")).toBe(true)
  })

  it("names the repository folder and the request, not only the tool name", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Remove Bash" }))

    expect(await screen.findByText(/in ariadne\?/)).toBeDefined()
    expect(screen.getByText(/“ls”/)).toBeDefined()
  })

  it("toasts the daemon's own message on a refusal", async () => {
    deleteFailure = {
      status: 404,
      code: "learned_permission_not_found",
      message: "already removed",
    }
    const user = userEvent.setup()
    renderScreen(
      <>
        <Toaster />
        <LearnedPermissionsTab />
      </>,
    )
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Remove Bash" }))
    await user.click(await screen.findByRole("button", { name: "Remove row" }))

    expect(await screen.findByText(/already removed/)).toBeDefined()
  })
})

describe("the detail view", () => {
  it("shows every field, with the tool call, the options and a null output as JSON", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await user.click(await screen.findByText("Bash"))

    const panel = within(await screen.findByRole("dialog"))
    expect(panel.getByText(/"command": "ls"/)).toBeDefined()
    expect(panel.getByText(/"optionId": "yes"/)).toBeDefined()
    expect(panel.getByText("no model output")).toBeDefined()
    expect(panel.getByText(ALLOWED_ROW.id)).toBeDefined()
    expect(panel.getByText(REPO_A.id)).toBeDefined()
    expect(panel.getByText("Learn")).toBeDefined()
  })

  it("shows the model's output as JSON where it was called", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await user.click(await screen.findByText("Write"))

    expect(await screen.findByText(/"label": "deny"/)).toBeDefined()
    expect(screen.queryByText("no model output")).toBeNull()
  })

  it("refreshes the row on an `updated` event", async () => {
    const { queryClient } = renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    // The daemon's own row moves first, as it does in production — the event
    // only ever follows a write the GET would already answer the same way.
    const updated = { ...ALLOWED_ROW, selected_option: "no" }
    rows = rows.map((row) => (row.id === updated.id ? updated : row))

    act(() => {
      dispatchDomainEvent(queryClient, { event: "learned_permission_updated", data: updated })
    })

    expect(await screen.findByText("Deny")).toBeDefined()
  })
})

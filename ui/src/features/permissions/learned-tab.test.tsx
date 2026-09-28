// @vitest-environment jsdom

/**
 * The Learned tab against a stubbed daemon.
 *
 * Two rows, from two repositories, one from each source: what is worth
 * pinning is that the table joins a row's `repository_id` against the
 * registry for its path, that the `repository` filter narrows the list the
 * daemon's own way, that Add, Edit and Delete each send the contract's
 * request and toast a refusal rather than swallow it, and that the detail view
 * tells a console row's stored request from a manual row's absent one.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { LearnedPermissionDto, RepositoryDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
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

const CONSOLE_ROW: LearnedPermissionDto = aLearnedPermission({
  id: "01JLEARN0000000000000CON",
  repository_id: REPO_A.id,
  tool_name: "Bash",
  kind: "execute",
  source: "console",
  tool_call: { title: "Bash", kind: "execute", rawInput: { command: "ls" } },
  options: { choices: ["allow", "deny"] },
  selected_option: "allow",
  session_id: "01JSESS0000000000000000001",
  task_id: "01JTASK0000000000000000001",
  label: "allow",
  danger: 0.12,
  allow_threshold: 0.2,
  deny_threshold: 0.8,
})

const MANUAL_ROW: LearnedPermissionDto = aLearnedPermission({
  id: "01JLEARN0000000000000MAN",
  repository_id: REPO_B.id,
  tool_name: "Write",
  kind: "edit",
  source: "manual",
})

const FILE_ROW: LearnedPermissionDto = aLearnedPermission({
  id: "01JLEARN0000000000000FIL",
  repository_id: REPO_A.id,
  tool_name: "Read",
  kind: "read",
  source: "console",
  tool_call: {
    title: "Read",
    kind: "read",
    rawInput: { file_path: "/home/me/dev/ariadne/src/main.rs" },
  },
})

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []
let rows: LearnedPermissionDto[] = []
let createFailure: { status: number; code: string; message: string } | null = null
let updateFailure: { status: number; code: string; message: string } | null = null
let deleteFailure: { status: number; code: string; message: string } | null = null

function stubDaemon() {
  requests = []
  rows = [CONSOLE_ROW, MANUAL_ROW]
  createFailure = null
  updateFailure = null
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
      if (request.method === "POST") {
        if (createFailure) {
          const { status, code, message } = createFailure
          return errorResponse(status, code, message)
        }
        const created = aLearnedPermission({ id: "01JLEARN0000000000000NEW", ...body })
        rows = [...rows, created]
        return jsonResponse(created, 201)
      }
    }

    const match = /^\/v1\/permissions\/learned\/(.+)$/.exec(url.pathname)
    if (match) {
      const id = match[1]
      const row = rows.find((one) => one.id === id)
      if (request.method === "GET") {
        return row
          ? jsonResponse(row)
          : errorResponse(404, "learned_permission_not_found", "no such approval")
      }
      if (request.method === "PUT") {
        if (updateFailure) {
          const { status, code, message } = updateFailure
          return errorResponse(status, code, message)
        }
        const updated = { ...row, ...body } as LearnedPermissionDto
        rows = rows.map((one) => (one.id === id ? updated : one))
        return jsonResponse(updated)
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
  it("shows each row's repository by its folder name, with the full path as a title", async () => {
    renderScreen(<LearnedPermissionsTab />)

    expect(await screen.findByText("Bash")).toBeDefined()
    expect(screen.getByText("ariadne")).toBeDefined()
    expect(screen.getByTitle(REPO_A.path)).toBeDefined()
    expect(screen.getByText("Write")).toBeDefined()
    expect(screen.getByText("sandbox")).toBeDefined()
    expect(screen.getByTitle(REPO_B.path)).toBeDefined()
    expect(screen.getByText("Console")).toBeDefined()
    expect(screen.getByText("Manual")).toBeDefined()
    expect(screen.getByText("2 approvals")).toBeDefined()
  })

  it("shows the AI label and danger where the model scored the row", async () => {
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    expect(screen.getByText("Allow")).toBeDefined()
    expect(screen.getByText("· 0.12")).toBeDefined()
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
    rows = [MANUAL_ROW]
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Write")

    await user.click(screen.getByRole("combobox", { name: "Filter by repository" }))
    await user.click(await screen.findByRole("option", { name: REPO_A.path }))

    expect(await screen.findByText("No approvals for this repository")).toBeDefined()

    await user.click(screen.getByRole("button", { name: "Clear filter" }))

    expect(await screen.findByText("Write")).toBeDefined()
  })

  it("shows the tool name and the kind cut with an ellipsis, each with a title of the full value", async () => {
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const tool = screen.getByTitle("Bash")
    expect(tool.className).toContain("truncate")
    const kind = screen.getByTitle("execute")
    expect(kind.className).toContain("truncate")
  })

  it("shows the Request column text: the command of a shell call, the path of a file call", async () => {
    rows = [CONSOLE_ROW, FILE_ROW]
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    expect(screen.getByTitle("ls")).toBeDefined()
    expect(screen.getByTitle("/home/me/dev/ariadne/src/main.rs")).toBeDefined()
  })

  it("pins the actions cell to the trailing edge", async () => {
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const actionsCell = screen.getByRole("button", { name: "Edit Bash" }).closest("td")
    expect(actionsCell).not.toBeNull()
    expect(actionsCell?.className).toContain("sticky")
    expect(actionsCell?.className).toContain("right-0")
  })

  it("opens a row's detail with the keyboard", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    const row = screen.getByRole("row", { name: "Bash in ariadne" })
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
    await screen.findByText("No approvals for this repository")

    const trigger = screen.getByRole("combobox", { name: "Filter by repository" })
    expect(within(trigger).getByText(shortId(missingId))).toBeDefined()
    expect(trigger.title).toBe(missingId)
  })
})

describe("adding an approval", () => {
  it("sends the repository, the tool name and the kind gathered in the form", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Add approval" }))
    const dialog = await screen.findByRole("dialog")
    await user.click(within(dialog).getByRole("combobox", { name: "Repository" }))
    const option = await screen.findByRole("option", { name: REPO_B.path })
    expect(within(option).getByText("sandbox")).toBeDefined()
    expect(within(option).getByText(REPO_B.path)).toBeDefined()
    await user.click(option)
    expect(within(dialog).getByRole("combobox", { name: "Repository" }).title).toBe(REPO_B.path)
    await user.type(within(dialog).getByLabelText("Tool name"), "Read")
    await user.type(within(dialog).getByLabelText("Kind"), "read")
    await user.click(within(dialog).getByRole("button", { name: "Add approval" }))

    await waitFor(() => {
      const created = requests.find((request) => request.method === "POST")
      expect(created?.body).toEqual({ repository_id: REPO_B.id, tool_name: "Read", kind: "read" })
    })
  })

  it("toasts the daemon's own message on a refusal", async () => {
    createFailure = {
      status: 409,
      code: "learned_permission_exists",
      message: "this repository already approves Bash execute",
    }
    const user = userEvent.setup()
    renderScreen(
      <>
        <Toaster />
        <LearnedPermissionsTab />
      </>,
    )
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Add approval" }))
    const dialog = await screen.findByRole("dialog")
    await user.click(within(dialog).getByRole("combobox", { name: "Repository" }))
    await user.click(await screen.findByRole("option", { name: REPO_A.path }))
    await user.type(within(dialog).getByLabelText("Tool name"), "Bash")
    await user.type(within(dialog).getByLabelText("Kind"), "execute")
    await user.click(within(dialog).getByRole("button", { name: "Add approval" }))

    expect(await screen.findByText(/already approves Bash execute/)).toBeDefined()
  })
})

describe("editing an approval", () => {
  it("opens with the row's own values and sends the tool name and the kind", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Edit Bash" }))
    const dialog = await screen.findByRole("dialog")
    const toolName = within(dialog).getByLabelText("Tool name") as HTMLInputElement
    expect(toolName.value).toBe("Bash")
    await user.clear(toolName)
    await user.type(toolName, "Shell")
    await user.click(within(dialog).getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      const updated = requests.filter((request) => request.method === "PUT").at(-1)
      expect(updated?.body).toEqual({ tool_name: "Shell", kind: "execute" })
    })
  })

  it("toasts the daemon's own message on a refusal", async () => {
    updateFailure = {
      status: 422,
      code: "invalid_request",
      message: "the tool name cannot be empty",
    }
    const user = userEvent.setup()
    renderScreen(
      <>
        <Toaster />
        <LearnedPermissionsTab />
      </>,
    )
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Edit Bash" }))
    const dialog = await screen.findByRole("dialog")
    await user.click(within(dialog).getByRole("button", { name: "Save changes" }))

    expect(await screen.findByText(/the tool name cannot be empty/)).toBeDefined()
  })
})

describe("removing an approval", () => {
  it("asks for confirmation, then sends the delete", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await screen.findByText("Bash")

    await user.click(screen.getByRole("button", { name: "Remove Bash" }))
    expect(screen.getByText("Bash")).toBeDefined()
    await user.click(await screen.findByRole("button", { name: "Remove approval" }))

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
    await user.click(await screen.findByRole("button", { name: "Remove approval" }))

    expect(await screen.findByText(/already removed/)).toBeDefined()
  })
})

describe("the detail view", () => {
  it("shows the stored request of a console row, with the AI verdict and both thresholds", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await user.click(await screen.findByText("Bash"))

    expect(await screen.findByText(/"command": "ls"/)).toBeDefined()
    expect(screen.getByText(/"choices"/)).toBeDefined()
    expect(screen.getByText("allow")).toBeDefined()
    expect(screen.getByText(CONSOLE_ROW.id)).toBeDefined()
    expect(screen.getByText(REPO_A.id)).toBeDefined()
    expect(screen.getByRole("link", { name: /^session/ })).toBeDefined()
    expect(screen.getByRole("link", { name: /^task/ })).toBeDefined()
    expect(screen.getByText("0.12")).toBeDefined()
    expect(screen.getByText("0.20")).toBeDefined()
    expect(screen.getByText("0.80")).toBeDefined()
  })

  it("says a manual row recorded no request", async () => {
    const user = userEvent.setup()
    renderScreen(<LearnedPermissionsTab />)
    await user.click(await screen.findByText("Write"))

    expect(await screen.findByText(/no request was recorded/)).toBeDefined()
    expect(screen.queryByText(/"command"/)).toBeNull()
  })
})

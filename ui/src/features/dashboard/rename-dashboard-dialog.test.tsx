// @vitest-environment jsdom

/**
 * Renaming a dashboard: a create under the new name, then a delete of the
 * old one — in that order, so a refusal leaves the old dashboard in place.
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { aDashboard } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { RenameDashboardDialog } from "./rename-dashboard-dialog"

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

function stubDaemon(answer: (request: Request) => Response) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const raw = await request.text()
    requests.push({
      method: request.method,
      path: new URL(request.url).pathname,
      body: raw.length > 0 ? JSON.parse(raw) : null,
    })
    return answer(request)
  })
}

const CUSTOM = aDashboard({
  name: "custom",
  builtin: false,
  document: {
    kind: "Dashboard",
    metadata: { name: "custom" },
    spec: { display: { name: "On call" }, duration: "1d" },
  },
})

function renderDialog() {
  const onRenamed = vi.fn()
  const onOpenChange = vi.fn()
  renderScreen(
    <RenameDashboardDialog
      dashboard={CUSTOM}
      open
      onOpenChange={onOpenChange}
      onRenamed={onRenamed}
    />,
  )
  return { onRenamed, onOpenChange }
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "Rename On call" })
}

beforeEach(() => {
  requests = []
  stubDaemon((request) =>
    request.method === "POST"
      ? jsonResponse(aDashboard({ name: "on-call", builtin: false }), 201)
      : new Response(null, { status: 204 }),
  )
})

describe("renaming", () => {
  it("titles the dialog with the dashboard's display name", () => {
    renderDialog()

    expect((within(dialog()).getByLabelText("Name") as HTMLInputElement).value).toBe("custom")
  })

  it("creates the dashboard's document under the new name, then deletes the old one", async () => {
    const user = userEvent.setup()
    const { onOpenChange, onRenamed } = renderDialog()

    fireEvent.change(within(dialog()).getByLabelText("Name"), { target: { value: "" } })
    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("button", { name: "Rename" }))

    await waitFor(() => expect(requests).toHaveLength(2))
    expect(requests[0]).toMatchObject({
      method: "POST",
      path: "/v1/dashboards",
      body: {
        name: "on-call",
        document: { kind: "Dashboard", metadata: { name: "on-call" }, spec: { duration: "1d" } },
      },
    })
    expect(requests[1]).toMatchObject({ method: "DELETE", path: "/v1/dashboards/custom" })
    expect(onOpenChange).toHaveBeenCalledWith(false)
    await waitFor(() => expect(onRenamed).toHaveBeenCalled())
  })

  it("refuses a name of only spaces, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    fireEvent.change(within(dialog()).getByLabelText("Name"), { target: { value: "" } })
    await user.type(within(dialog()).getByLabelText("Name"), "   ")
    await user.click(within(dialog()).getByRole("button", { name: "Rename" }))

    expect(await screen.findByText("A dashboard needs a name.")).toBeDefined()
    expect(requests).toEqual([])
  })

  it("puts a name already taken on the name field, and never deletes the old dashboard", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(409, "conflict", "dashboard on-call is taken"))
    const { onOpenChange } = renderDialog()

    fireEvent.change(within(dialog()).getByLabelText("Name"), { target: { value: "" } })
    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("button", { name: "Rename" }))

    expect(await screen.findByText('A dashboard named "on-call" already exists.')).toBeDefined()
    expect(requests.some((one) => one.method === "DELETE")).toBe(false)
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })
})

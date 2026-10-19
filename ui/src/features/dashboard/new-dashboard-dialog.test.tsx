// @vitest-environment jsdom

/**
 * A dashboard of your own: a name, and a choice of what it starts from.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { aDashboard } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { NewDashboardDialog } from "./new-dashboard-dialog"

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

function stubDaemon(answer: () => Response) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const raw = await request.text()
    requests.push({
      method: request.method,
      path: new URL(request.url).pathname,
      body: raw.length > 0 ? JSON.parse(raw) : null,
    })
    return answer()
  })
}

function renderDialog(current = aDashboard({ name: "stats", builtin: true })) {
  const onCreated = vi.fn()
  const onOpenChange = vi.fn()
  renderScreen(
    <NewDashboardDialog open onOpenChange={onOpenChange} current={current} onCreated={onCreated} />,
  )
  return { onCreated, onOpenChange }
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "New dashboard" })
}

beforeEach(() => {
  requests = []
  stubDaemon(() => jsonResponse(aDashboard({ name: "on-call", builtin: false }), 201))
})

describe("starting point", () => {
  it("offers Empty and Copy of the current dashboard's display name when one is given", () => {
    renderDialog(
      aDashboard({
        name: "stats",
        document: {
          kind: "Dashboard",
          metadata: { name: "stats" },
          spec: { display: { name: "Default" } },
        },
      }),
    )

    expect(within(dialog()).getByRole("radio", { name: "Empty" })).toBeDefined()
    expect(within(dialog()).getByRole("radio", { name: "Copy of Default" })).toBeDefined()
  })

  it("offers no Copy choice without a current dashboard to copy from", () => {
    const onCreated = vi.fn()
    renderScreen(<NewDashboardDialog open onOpenChange={vi.fn()} onCreated={onCreated} />)

    expect(within(dialog()).queryByRole("radio", { name: /Copy of/ })).toBeNull()
  })
})

describe("creating", () => {
  it("refuses a dashboard with no name, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByText("A dashboard needs a name.")).toBeDefined()
    expect(requests).toEqual([])
  })

  it("refuses a name of only spaces, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "   ")
    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByText("A dashboard needs a name.")).toBeDefined()
    expect(requests).toEqual([])
  })

  it("creates a valid, empty Perses dashboard when Empty is picked", async () => {
    const user = userEvent.setup()
    const { onCreated, onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    await waitFor(() => {
      expect(requests.find((one) => one.method === "POST")?.body).toEqual({
        name: "on-call",
        document: { kind: "Dashboard", metadata: { name: "on-call" }, spec: {} },
      })
    })
    expect(onOpenChange).toHaveBeenCalledWith(false)
    await waitFor(() => expect(onCreated).toHaveBeenCalled())
  })

  it("replaces the copied dashboard's display name with its new name", async () => {
    const user = userEvent.setup()
    const current = aDashboard({
      name: "stats",
      document: {
        kind: "Dashboard",
        metadata: { name: "stats" },
        spec: {
          display: { name: "Default" },
          duration: "7d",
          panels: { usage: { kind: "Panel" } },
        },
      },
    })
    renderDialog(current)

    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("radio", { name: "Copy of Default" }))
    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    await waitFor(() => {
      expect(requests.find((one) => one.method === "POST")?.body).toEqual({
        name: "on-call",
        document: {
          kind: "Dashboard",
          metadata: { name: "on-call" },
          spec: {
            display: { name: "on-call" },
            duration: "7d",
            panels: { usage: { kind: "Panel" } },
          },
        },
      })
    })
  })

  it("puts a name already taken on the name field, and keeps the dialog open", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(409, "conflict", "dashboard on-call is taken"))
    const { onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByText('A dashboard named "on-call" already exists.')).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it("shows any other refusal on the dialog itself", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(422, "invalid_request", "kind must be Dashboard"))
    renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "on-call")
    await user.click(within(dialog()).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByText(/kind must be Dashboard/)).toBeDefined()
  })
})

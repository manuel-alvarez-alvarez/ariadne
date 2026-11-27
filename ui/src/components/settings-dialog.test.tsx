// @vitest-environment jsdom

/**
 * The settings dialog's webhook tunnel against a stubbed daemon (027).
 *
 * The daemon URL beside it is a local store write with nothing to stub; the
 * tunnel is the daemon's own switch, written the moment it is flipped, and its
 * state is read back off what the daemon answers.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { DEFAULT_BASE_URL, type ForgeTunnelDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { useSettingsStore } from "@/stores/settings"
import { useStreamStore } from "@/stores/stream"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SettingsDialog } from "./settings-dialog"

function aTunnel(overrides: Partial<ForgeTunnelDto> = {}): ForgeTunnelDto {
  return {
    enabled: true,
    state: "up",
    url: "https://smee.io/Tl13s4JZbHs6Ww60eXq7Ab",
    listen: "127.0.0.1:49152",
    since: "2026-10-08T10:00:00.000Z",
    error: null,
    ...overrides,
  }
}

/** What `GET /v1/forge/tunnel` answers; a `PUT` moves its switch. */
let tunnel: ForgeTunnelDto

/** The bodies of every `PUT /v1/forge/tunnel`. */
let tunnelWrites: unknown[] = []

beforeEach(() => {
  useStreamStore.getState().reset()
  tunnel = aTunnel()
  tunnelWrites = []
  useSettingsStore.setState({ baseUrl: DEFAULT_BASE_URL, diagnosticsEnabled: true })
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    if (request.method === "PUT") {
      const body = (await request.json()) as { enabled: boolean }
      tunnelWrites.push(body)
      tunnel = body.enabled ? aTunnel() : aTunnel({ enabled: false, state: "off", url: null })
    }
    return jsonResponse(tunnel)
  })
})

describe("SettingsDialog", () => {
  it("has no Hosts section", () => {
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)
    expect(screen.queryByRole("table")).toBeNull()
    expect(screen.queryByText("Hosts")).toBeNull()
    expect(screen.queryByLabelText("Worker name")).toBeNull()
  })

  it("sends app diagnostics by default and lets the user disable them", async () => {
    const user = userEvent.setup()
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

    const toggle = screen.getByRole("switch", { name: "Send app diagnostics to the daemon" })
    expect(toggle.getAttribute("aria-checked")).toBe("true")
    await user.click(toggle)
    expect(useSettingsStore.getState().diagnosticsEnabled).toBe(false)
  })

  it("saves the daemon URL from the button beside the field, and closes", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderScreen(
      <>
        <Toaster />
        <SettingsDialog open onOpenChange={onOpenChange} />
      </>,
    )

    await waitFor(() => expect(document.activeElement?.textContent).toBe("Daemon setup help"))
    const field = screen.getByLabelText("Daemon URL")
    await user.clear(field)
    await user.type(field, "http://localhost:9999")
    await user.click(screen.getByRole("button", { name: "Save address" }))

    expect(await screen.findByText("Daemon URL updated")).toBeDefined()
    expect(
      within(screen.getByRole("region", { name: "Connection recovery" })).getByText(
        "http://localhost:9999",
      ),
    ).toBeDefined()
    expect(useSettingsStore.getState().baseUrl).toBe("http://localhost:9999")
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("shows the field error and saves nothing for an invalid URL", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderScreen(<SettingsDialog open onOpenChange={onOpenChange} />)

    await waitFor(() => expect(document.activeElement?.textContent).toBe("Daemon setup help"))
    const field = screen.getByLabelText("Daemon URL")
    await user.clear(field)
    await user.type(field, "not a url")
    await user.click(screen.getByRole("button", { name: "Save address" }))

    expect(await screen.findByText("Not a valid URL, e.g. http://127.0.0.1:7676")).toBeDefined()
    expect(useSettingsStore.getState().baseUrl).toBe(DEFAULT_BASE_URL)
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("keeps the footer to Reset to default and Close, with no Save button there", () => {
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

    const footer = screen.getByRole("button", { name: "Reset to default" }).closest("div")
    expect(footer).not.toBeNull()
    expect(footer?.querySelector("button[type=submit]")).toBeNull()
    expect(
      footer &&
        Array.from(footer.querySelectorAll("button")).map(
          (b) => b.getAttribute("aria-label") ?? b.textContent,
        ),
    ).toEqual(["Reset to default", "Close"])
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull()
  })

  it("says what the tunnel is for, shows its state, and its switch turns it off and on", async () => {
    const user = userEvent.setup()
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

    expect(screen.getByText(/forwards to the daemon/)).toBeDefined()
    expect(await screen.findByText("Tunnel up")).toBeDefined()
    expect(screen.getByText("https://smee.io/Tl13s4JZbHs6Ww60eXq7Ab")).toBeDefined()
    const toggle = screen.getByRole("switch", { name: "Webhook tunnel" })
    expect(toggle.getAttribute("aria-checked")).toBe("true")

    await user.click(toggle)
    expect(await screen.findByText("Tunnel off")).toBeDefined()
    expect(
      screen.getByRole("switch", { name: "Webhook tunnel" }).getAttribute("aria-checked"),
    ).toBe("false")

    await user.click(screen.getByRole("switch", { name: "Webhook tunnel" }))
    expect(await screen.findByText("Tunnel up")).toBeDefined()
    expect(tunnelWrites).toEqual([{ enabled: false }, { enabled: true }])
  })

  it("says why a tunnel that is on is down", async () => {
    tunnel = aTunnel({ state: "down", url: null, error: "connection refused" })
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

    expect(await screen.findByText("Tunnel down")).toBeDefined()
    expect(screen.getByText("connection refused")).toBeDefined()
  })
  it("shows the saved address and live connection state with recovery guidance", () => {
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)
    expect(screen.getByText(/Configured address:/)).toBeDefined()
    expect(screen.getByText(/Connection: Connecting/)).toBeDefined()
    expect(screen.getByRole("button", { name: "Retry connection" }).textContent).toBe("")
    expect(screen.getByText(/Start the daemon/)).toBeDefined()
  })

  it("announces stream changes without moving focus and gives neutral failure guidance", async () => {
    const user = userEvent.setup()
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)
    await waitFor(() => expect(document.activeElement?.textContent).toBe("Daemon setup help"))
    const field = screen.getByLabelText("Daemon URL")
    await user.click(field)
    act(() => useStreamStore.getState().setStatus("reconnecting"))
    expect(screen.getByRole("status").textContent).toContain("Disconnected")
    expect(screen.getByText(/The connection failed/)).toBeDefined()
    expect(document.activeElement).toBe(field)
    act(() => useStreamStore.getState().markOpen())
    expect(screen.getByRole("status").textContent).toContain("Connected")
    expect(screen.queryByText(/The connection failed/)).toBeNull()
    expect(document.activeElement).toBe(field)
  })
})

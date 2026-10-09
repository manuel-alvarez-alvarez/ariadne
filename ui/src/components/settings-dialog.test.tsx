// @vitest-environment jsdom

/**
 * The settings dialog's webhook tunnel against a stubbed daemon (027).
 *
 * The daemon URL beside it is a local store write with nothing to stub; the
 * tunnel is the daemon's own switch, written the moment it is flipped, and its
 * state is read back off what the daemon answers.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { ForgeTunnelDto } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SettingsDialog } from "./settings-dialog"

function aTunnel(overrides: Partial<ForgeTunnelDto> = {}): ForgeTunnelDto {
  return {
    enabled: true,
    state: "up",
    url: "https://amber-104233.loca.lt",
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
  tunnel = aTunnel()
  tunnelWrites = []
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
  it("says what the tunnel is for, shows its state, and its switch turns it off and on", async () => {
    const user = userEvent.setup()
    renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

    expect(screen.getByText(/forwards to the daemon/)).toBeDefined()
    expect(await screen.findByText("Tunnel up")).toBeDefined()
    expect(screen.getByText("https://amber-104233.loca.lt")).toBeDefined()
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
})

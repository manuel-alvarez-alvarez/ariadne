import { describe, expect, it } from "vitest"

import { getDashboard, saveDashboard } from "./dashboard-client"
import { daemonFetch } from "./test/setup"

const DAEMON_URL = "http://127.0.0.1:7676"
const DOCUMENT = { kind: "Dashboard", metadata: { name: "stats" }, spec: {} }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  })
}

describe("getDashboard", () => {
  it("reads the document off GET /v1/dashboards/{name}", async () => {
    daemonFetch.mockImplementation(async (input: Request | string | URL) => {
      const url = new URL(String(input))
      expect(url.pathname).toBe("/v1/dashboards/stats")
      return jsonResponse({
        name: "stats",
        builtin: true,
        document_is_default: true,
        document: DOCUMENT,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      })
    })

    expect(await getDashboard(DAEMON_URL, "stats")).toEqual(DOCUMENT)
  })

  it("throws the daemon's own message on a refusal", async () => {
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ error: { code: "not_found", message: "no such dashboard" } }, 404),
    )

    await expect(getDashboard(DAEMON_URL, "missing")).rejects.toThrow("no such dashboard")
  })
})

describe("saveDashboard", () => {
  it("PUTs the document as JSON and returns what the daemon kept", async () => {
    let seen: { method: string; url: string; body: unknown } | undefined
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      seen = {
        method: String(init?.method),
        url: String(input),
        body: JSON.parse(String(init?.body)),
      }
      return jsonResponse({
        name: "stats",
        builtin: true,
        document_is_default: false,
        document: DOCUMENT,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-02T00:00:00Z",
      })
    })

    const saved = await saveDashboard(DAEMON_URL, "stats", DOCUMENT)

    expect(seen?.method).toBe("PUT")
    expect(new URL(seen?.url ?? "").pathname).toBe("/v1/dashboards/stats")
    expect(seen?.body).toEqual({ document: DOCUMENT })
    expect(saved).toEqual(DOCUMENT)
  })
})

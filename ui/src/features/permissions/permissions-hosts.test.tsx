// @vitest-environment jsdom

/** The permissions screens over a settings daemon and a named host. */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import { anAiPermissionsStatus } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { PermissionsPage } from "./permissions-page"

afterEach(() => forgetHosts())

const synced = (overrides: Partial<typeof BOX.sync>) => ({
  ...BOX,
  sync: { ...BOX.sync, ...overrides },
})

it("shows each host's own device and writes a device change to that host alone", async () => {
  const seen = stubHosts({
    local: { "/v1/permissions/ai": anAiPermissionsStatus({ device: "mlx" }) },
    box: {
      "/v1/permissions/ai": (_url: URL, request: Request) =>
        request.method === "PUT"
          ? jsonResponse(anAiPermissionsStatus({ device: "cpu" }))
          : jsonResponse(anAiPermissionsStatus({ device: "cuda" })),
    },
  })
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  await screen.findByRole("heading", { name: "AI on box" })
  const devices = screen.getAllByRole("combobox", { name: "Device" })
  expect(devices.map((device) => device.textContent?.replace(/\W/g, ""))).toEqual(["mlx", "cuda"])
  // Settings stay local's: one flavour select, one switch.
  expect(screen.getAllByRole("combobox", { name: "Flavour" })).toHaveLength(1)

  await userEvent.click(devices[1] as HTMLElement)
  await userEvent.click(await screen.findByRole("option", { name: /^mlx/ }))

  await waitFor(() => {
    const write = seen.find((request) => request.method === "PUT")
    expect(write?.origin).toBe(new URL(BOX.endpoint ?? "").origin)
    expect(JSON.parse(write?.body ?? "{}")).toEqual({ device: "mlx" })
  })
  expect(seen.filter((request) => request.method === "PUT")).toHaveLength(1)
})

it("shows a sync line per host: in sync, pending, or failed with its error", async () => {
  stubHosts({ local: { "/v1/permissions/learned": { items: [] }, "/v1/repositories": [] } }, [
    LOCAL,
    synced({ state: "pending" }),
    { ...synced({ state: "failed", error: "ssh closed" }), name: "far" },
  ])
  renderScreen(<PermissionsPage />)

  expect((await screen.findByTestId("sync-box")).textContent).toContain("pending")
  expect(screen.getByTestId("sync-far").textContent).toContain("failed: ssh closed")
})

it("counts the learned rows a host skipped on the Learned tab only", async () => {
  stubHosts(
    {
      local: {
        "/v1/permissions/learned": { items: [] },
        "/v1/repositories": [],
        "/v1/permissions/ai": anAiPermissionsStatus(),
      },
      box: { "/v1/permissions/ai": anAiPermissionsStatus() },
    },
    [LOCAL, synced({ skipped_permissions: 3 })],
  )
  const view = renderScreen(<PermissionsPage />)
  expect((await screen.findByTestId("sync-box")).textContent).toContain(
    "3 learned permissions skipped",
  )

  view.rerender(<PermissionsPage />)
  await userEvent.click(screen.getByRole("tab", { name: "AI" }))
  expect(screen.getByTestId("sync-box").textContent).not.toContain("skipped")
})

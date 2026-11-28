// @vitest-environment jsdom

/** The colour of a host, and every place that draws a host name in it. */

import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"
import { ErrorState, hostTitle, RefreshWarning } from "@/components/error-state"
import { HostColorSwatch } from "@/components/host-color-swatch"
import { HostDot } from "@/components/host-dot"
import { HostLabel } from "@/components/host-label"
import { HostPicker } from "@/components/host-picker"
import { HostSync } from "@/components/host-sync"
import { dispatchDomainEvent } from "@/events/dispatch"
import { HostsPage } from "@/features/hosts/hosts-page"
import { hostColor } from "@/lib/host-color"
import { HOST_COLORS } from "@/lib/host-colors"
import { renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

afterEach(() => forgetHosts())

const hueOf = (element: HTMLElement) => element.getAttribute("data-host-color")

it("gives a host its override, else grey for local, else a stable colour of the eight", () => {
  expect(hostColor("box", "pink")).toBe("pink")
  expect(hostColor("local", "teal")).toBe("teal")
  expect(hostColor("local")).toBe("grey")
  expect(hostColor("local", null)).toBe("grey")
  const names = ["box", "far", "gpu-1", "gpu-2", "build", "mac-mini", "nas", "ci", "edge", "dev"]
  const values = HOST_COLORS.map((color) => color.value)
  for (const name of names) {
    expect(values).toContain(hostColor(name))
    expect(hostColor(name)).toBe(hostColor(name))
  }
  // FNV-1a("box") % 8 is 2, the third colour, worked out apart from the code.
  expect(hostColor("box")).toBe("amber")
  expect(new Set(names.map((name) => hostColor(name))).size).toBeGreaterThan(3)
})

it("lists the eight colours in a fixed order with labels", () => {
  expect(HOST_COLORS.map((color) => color.value)).toEqual([
    "red",
    "orange",
    "amber",
    "green",
    "teal",
    "blue",
    "violet",
    "pink",
  ])
  expect(HOST_COLORS.map((color) => color.label)).toEqual([
    "Red",
    "Orange",
    "Amber",
    "Green",
    "Teal",
    "Blue",
    "Violet",
    "Pink",
  ])
})

it("draws a swatch of a colour of the list", () => {
  const { container } = render(<HostColorSwatch color="violet" />)
  const swatch = container.querySelector("[data-host-color]")
  expect(swatch?.getAttribute("data-host-color")).toBe("violet")
  expect(swatch?.className).toContain("bg-host-violet")
})

it("draws a HostLabel pill and a dot in the host colour, local in grey", async () => {
  stubHosts({}, [LOCAL, { ...BOX, color: "green" }])
  renderScreen(
    <>
      <HostLabel host="box" />
      <HostLabel host="local" />
      <span data-testid="dot">
        <HostDot host="box" />
      </span>
    </>,
  )
  await waitFor(() => expect(hueOf(screen.getByText("box"))).toBe("green"))
  expect(screen.getByText("box").className).toContain("bg-host-green-soft")
  expect(screen.getByText("box").className).toContain("text-host-green-fg")
  expect(hueOf(screen.getByText("local"))).toBe("grey")
  expect(screen.getByTestId("dot").firstElementChild?.getAttribute("data-host-color")).toBe("green")
})

it("repaints every place a host name is drawn when host_updated changes its colour", async () => {
  const user = userEvent.setup()
  stubHosts({ local: { "/v1/forge/tunnel": { enabled: false, state: "off" } } }, [
    LOCAL,
    { ...BOX, color: "red" },
  ])
  const { queryClient } = renderScreen(
    <>
      <HostPicker />
      <HostsPage />
      <HostSync />
      <HostLabel host="box" />
    </>,
  )
  const row = async () =>
    within(await screen.findByRole("table"))
      .getByText("box")
      .closest("[data-host-color]")
  await waitFor(async () => expect((await row())?.getAttribute("data-host-color")).toBe("red"))
  expect(
    within(screen.getByLabelText("Sync")).getByText("box").getAttribute("data-host-color"),
  ).toBe("red")

  await user.click(screen.getByRole("button", { name: "Host" }))
  const menu = await screen.findByRole("menu")
  expect(hueOf(within(menu).getByText("box"))).toBe("red")
  await user.keyboard("{Escape}")
  expect(screen.getByRole("button", { name: "Host" }).querySelector("[data-host-color]")).toBeNull()

  act(() => {
    dispatchDomainEvent(queryClient, "local", {
      event: "host_updated",
      data: { ...BOX, color: "blue" },
    })
  })
  await waitFor(() => expect(document.querySelectorAll('[data-host-color="red"]').length).toBe(0))
  expect(document.querySelectorAll('[data-host-color="blue"]').length).toBeGreaterThanOrEqual(3)
})

it("draws the host name of each error, warning and message in its colour, and repaints it live", async () => {
  stubHosts({}, [LOCAL, { ...BOX, color: "red" }])
  const { queryClient } = renderScreen(
    <>
      <ErrorState title={hostTitle("Could not load goals", "box", 2)} error={new Error("boom")} />
      <RefreshWarning
        what={
          <>
            tasks on <HostLabel host="box" />
          </>
        }
        error={new Error("boom")}
        onRetry={() => {}}
      />
    </>,
  )
  const names = () => screen.getAllByText("box")
  await waitFor(() => expect(names().map(hueOf)).toEqual(["red", "red"]))
  act(() => {
    dispatchDomainEvent(queryClient, "local", {
      event: "host_updated",
      data: { ...BOX, color: "green" },
    })
  })
  await waitFor(() => expect(names().map(hueOf)).toEqual(["green", "green"]))
})

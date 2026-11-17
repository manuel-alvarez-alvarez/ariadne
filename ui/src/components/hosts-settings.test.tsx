// @vitest-environment jsdom

/**
 * The settings dialog's Hosts section and the header's host picker, over the
 * settings daemon's `GET /v1/hosts`.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"

import { dispatchDomainEvent } from "@/events/dispatch"
import { useHostFilter } from "@/hooks/use-host-filter"
import { useSettingsStore } from "@/stores/settings"
import { renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { HostPicker } from "./host-picker"
import { SettingsDialog } from "./settings-dialog"

const FAR = {
  ...BOX,
  name: "far",
  kind: "ssh" as const,
  endpoint: null,
  status: "down" as const,
  error: "ssh forward not started",
}

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "" })
})

afterEach(() => {
  forgetHosts()
})

/** The cells of the Hosts table's row for one host, in column order. */
function hostRow(name: string): string[] {
  const table = screen.getByRole("table")
  const row = within(table)
    .getAllByRole("row")
    .find((one) => within(one).queryAllByRole("cell")[0]?.textContent === name)
  if (!row) throw new Error(`no row for ${name}`)
  return within(row)
    .getAllByRole("cell")
    .map((cell) => cell.textContent ?? "")
}

it("lists every host with its name, kind, status, endpoint and error, live through host_updated", async () => {
  const hosts = [LOCAL, BOX, FAR]
  stubHosts({ local: { "/v1/forge/tunnel": { enabled: false, state: "off" } } }, hosts)
  const { queryClient } = renderScreen(<SettingsDialog open onOpenChange={() => {}} />)

  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(4))
  expect(hostRow("local")).toEqual(["local", "local", "ready", "http://127.0.0.1:7676", "-"])
  expect(hostRow("box")).toEqual(["box", "direct", "ready", "http://box.test:7676", "-"])
  expect(hostRow("far")).toEqual(["far", "ssh", "down", "-", "ssh forward not started"])
  expect(screen.getByText(/tables of the/).textContent).toContain("config.toml")

  // The daemon's own row moves first, as it does in production — the event
  // only ever follows a change the GET would already answer the same way.
  const up = { ...FAR, status: "ready" as const, endpoint: "http://127.0.0.1:7701", error: null }
  hosts[2] = up
  act(() => {
    dispatchDomainEvent(queryClient, "local", { event: "host_updated", data: up })
  })

  await waitFor(() =>
    expect(hostRow("far")).toEqual(["far", "ssh", "ready", "http://127.0.0.1:7701", "-"]),
  )
})

/** What `useHostFilter` says, beside the picker. */
function Selected() {
  return <p data-testid="selected">{useHostFilter().join(",")}</p>
}

it("picks All, local or a named host with its status, and keeps the pick", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, BOX, FAR])
  renderScreen(
    <>
      <HostPicker />
      <Selected />
    </>,
  )
  // All reads every host the app can reach; the one that is down is left out.
  await waitFor(() => expect(screen.getByTestId("selected").textContent).toBe("local,box"))

  await user.click(screen.getByRole("button", { name: "Host" }))
  const options = await screen.findAllByRole("menuitemradio")
  expect(options.map((option) => option.textContent)).toEqual([
    "All hosts",
    "local",
    "boxready",
    "fardownssh forward not started",
  ])
  await user.click(options[2] as HTMLElement)

  await waitFor(() => expect(screen.getByTestId("selected").textContent).toBe("box"))
  expect(useSettingsStore.getState().hostFilter).toBe("box")
  expect(JSON.parse(localStorage.getItem("ariadne.settings") ?? "{}").state.hostFilter).toBe("box")
})

// @vitest-environment jsdom

/**
 * The settings dialog's Hosts section and the header's host picker, over the
 * settings daemon's `GET /v1/hosts`.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"
import { useHosts } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { dispatchDomainEvent } from "@/events/dispatch"
import { useHostFilter } from "@/hooks/use-host-filter"
import { useSettingsStore } from "@/stores/settings"
import { errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, LOCAL_ORIGIN, stubHosts } from "@/test/hosts"
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
  expect(hostRow("local")).toEqual(["local", "local", "ready", "http://127.0.0.1:7676", "-", ""])
  expect(hostRow("box")).toEqual(["box", "direct", "ready", "http://box.test:7676", "-", ""])
  expect(hostRow("far")).toEqual(["far", "ssh", "down", "-", "ssh forward not started", ""])
  expect(screen.getByText(/tables of the/).textContent).toContain("config.toml")

  // The daemon's own row moves first, as it does in production — the event
  // only ever follows a change the GET would already answer the same way.
  const up = { ...FAR, status: "ready" as const, endpoint: "http://127.0.0.1:7701", error: null }
  hosts[2] = up
  act(() => {
    dispatchDomainEvent(queryClient, "local", { event: "host_updated", data: up })
  })

  await waitFor(() =>
    expect(hostRow("far")).toEqual(["far", "ssh", "ready", "http://127.0.0.1:7701", "-", ""]),
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
    "farnot reachablessh forward not started",
  ])
  await user.click(options[2] as HTMLElement)

  await waitFor(() => expect(screen.getByTestId("selected").textContent).toBe("box"))
  expect(useSettingsStore.getState().hostFilter).toBe("box")
  expect(JSON.parse(localStorage.getItem("ariadne.settings") ?? "{}").state.hostFilter).toBe("box")
})

/** Says when the hosts list has answered. */
function Answered() {
  return <p data-testid="answered">{useHosts().isPending ? "" : "yes"}</p>
}

it("shows no host picker while the daemon lists local alone", async () => {
  stubHosts({}, [LOCAL])
  renderScreen(
    <>
      <HostPicker />
      <Answered />
    </>,
  )
  await waitFor(() => expect(screen.getByTestId("answered").textContent).toBe("yes"))
  expect(screen.queryByRole("button", { name: "Host" })).toBeNull()
})

it("shows the host picker for a named host that is down", async () => {
  stubHosts({}, [LOCAL, FAR])
  renderScreen(<HostPicker />)
  expect(await screen.findByRole("button", { name: "Host" })).toBeTruthy()
})

it("shows a host with a socket path as not reachable, never as ready", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, { ...FAR, status: "ready", endpoint: "/run/ariadned.sock", error: null }])
  renderScreen(<HostPicker />)

  await user.click(await screen.findByRole("button", { name: "Host" }))
  const options = await screen.findAllByRole("menuitemradio")
  expect(options[2]?.textContent).toBe(
    "farnot reachable/run/ariadned.sock is not an http:// or https:// endpoint",
  )
})

it("shows the status of a picked named host in the trigger", async () => {
  useSettingsStore.setState({ hostFilter: "far" })
  stubHosts({}, [LOCAL, BOX, FAR])
  renderScreen(<HostPicker />)

  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Host" }).textContent).toBe("farnot reachable"),
  )
})

it("reads a saved pick the daemon no longer lists as All hosts", async () => {
  useSettingsStore.setState({ hostFilter: "gone" })
  stubHosts({}, [LOCAL, BOX])
  renderScreen(
    <>
      <HostPicker />
      <Selected />
    </>,
  )

  await waitFor(() => expect(screen.getByTestId("selected").textContent).toBe("local,box"))
  expect(screen.getByRole("button", { name: "Host" }).textContent).toBe("All hosts")
})

const RECONNECT = (name: string) => `/v1/hosts/${name}/reconnect`

it("reconnects a named host from each picker row, in each status, and picks nothing", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    {
      local: {
        [RECONNECT("box")]: () => jsonResponse(BOX),
        [RECONNECT("far")]: () => jsonResponse(FAR),
      },
    },
    [LOCAL, BOX, FAR],
  )
  renderScreen(
    <>
      <HostPicker />
      <Selected />
    </>,
  )
  await waitFor(() => expect(screen.getByTestId("selected").textContent).toBe("local,box"))
  await user.click(screen.getByRole("button", { name: "Host" }))
  await screen.findAllByRole("menuitemradio")

  expect(screen.queryByRole("button", { name: "Reconnect local" })).toBeNull()
  await user.click(screen.getByRole("button", { name: "Reconnect far" }))
  await user.click(screen.getByRole("button", { name: "Reconnect box" }))

  await waitFor(() =>
    expect(seen.filter((one) => one.method === "POST").map((one) => one.url.pathname)).toEqual([
      RECONNECT("far"),
      RECONNECT("box"),
    ]),
  )
  expect(seen.find((one) => one.method === "POST")?.origin).toBe(LOCAL_ORIGIN)
  expect(useSettingsStore.getState().hostFilter).toBe("")
  expect(screen.getByTestId("selected").textContent).toBe("local,box")
})

it("reconnects a host from its Hosts row, disabled while it runs, and shows the new status", async () => {
  const user = userEvent.setup()
  const hosts = [LOCAL, BOX, FAR]
  const seen = stubHosts(
    {
      local: {
        [RECONNECT("far")]: () => undefined,
      },
    },
    hosts,
  )
  const { queryClient } = renderScreen(<SettingsDialog open onOpenChange={() => {}} />)
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(4))

  expect(screen.queryByRole("button", { name: "Reconnect local" })).toBeNull()
  const button = screen.getByRole("button", { name: "Reconnect far" })
  await user.click(button)
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(true))
  expect(seen.filter((one) => one.method === "POST")).toHaveLength(1)

  const up = { ...FAR, status: "ready" as const, endpoint: "http://127.0.0.1:7701", error: null }
  hosts[2] = up
  act(() => {
    dispatchDomainEvent(queryClient, "local", { event: "host_updated", data: up })
  })
  await waitFor(() => expect(hostRow("far")[2]).toBe("ready"))
})

it("shows the error of a reconnect that fails", async () => {
  const user = userEvent.setup()
  stubHosts(
    {
      local: {
        [RECONNECT("box")]: () => errorResponse(404, "host_not_found", "no host named box"),
      },
    },
    [LOCAL, BOX],
  )
  renderScreen(
    <>
      <Toaster />
      <SettingsDialog open onOpenChange={() => {}} />
    </>,
  )
  await user.click(await screen.findByRole("button", { name: "Reconnect box" }))

  expect(await screen.findByText(/no host named box/)).toBeTruthy()
  expect(
    (screen.getByRole("button", { name: "Reconnect box" }) as HTMLButtonElement).disabled,
  ).toBe(false)
})

it("shows no reconnect button while the daemon lists local alone", async () => {
  stubHosts({}, [LOCAL])
  renderScreen(
    <>
      <HostPicker />
      <SettingsDialog open onOpenChange={() => {}} />
    </>,
  )
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(2))
  expect(screen.queryByRole("button", { name: /Reconnect/ })).toBeNull()
})

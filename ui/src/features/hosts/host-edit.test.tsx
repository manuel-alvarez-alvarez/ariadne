// @vitest-environment jsdom

/**
 * Adding, editing and removing a host on the Hosts screen, over the settings
 * daemon's `POST`, `PATCH` and `DELETE /v1/hosts`.
 */

import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"
import type { HostDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { dispatchDomainEvent } from "@/events/dispatch"
import { HOST_COLORS } from "@/lib/host-colors"
import { useSettingsStore } from "@/stores/settings"
import { errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, LOCAL_ORIGIN, stubHosts } from "@/test/hosts"
import { HostsPage } from "./hosts-page"

const NEW_BOX = { ...BOX, name: "new-box", endpoint: "ssh://me@new-box", kind: "ssh" as const }
const SSH_BOX = { ...BOX, name: "far", endpoint: "ssh://me@far", remote: "/run/far.sock" }

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "" })
})

afterEach(() => {
  cleanup()
  forgetHosts()
})

function mount() {
  return renderScreen(
    <>
      <Toaster />
      <HostsPage />
    </>,
  )
}

/** `/v1/hosts`: the list on GET, and `onPost` for the POST. */
function hostsRoute(hosts: HostDto[], onPost: () => Response | Promise<Response>) {
  return (_url: URL, request: Request) =>
    request.method === "POST" ? (onPost() as Response) : jsonResponse(hosts)
}

function bodies(seen: { method: string; body: string }[], method: string): unknown[] {
  return seen.filter((one) => one.method === method).map((one) => JSON.parse(one.body))
}

function type(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } })
}

it("adds a host with name, endpoint, remote and colour, and shows its row through host_updated", async () => {
  const user = userEvent.setup()
  const hosts = [LOCAL, BOX]
  const seen = stubHosts(
    {
      local: {
        "/v1/hosts": hostsRoute(hosts, () => jsonResponse({ ...NEW_BOX, color: "pink" }, 201)),
      },
    },
    hosts,
  )
  const { queryClient } = mount()
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(3))

  await user.click(screen.getByRole("button", { name: "Add host" }))
  const dialog = await screen.findByRole("dialog")
  type("Name", "new-box")
  type("Endpoint", "ssh://me@new-box")
  type("Remote", "/run/new.sock")
  await user.click(within(dialog).getByRole("radio", { name: "Pink" }))
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))

  await waitFor(() => expect(bodies(seen, "POST")).toHaveLength(1))
  expect(bodies(seen, "POST")[0]).toEqual({
    name: "new-box",
    endpoint: "ssh://me@new-box",
    remote: "/run/new.sock",
    color: "pink",
  })
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  act(() => {
    dispatchDomainEvent(queryClient, "local", { event: "host_updated", data: NEW_BOX })
  })
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(4))
})

it("offers Default and the eight colours as swatches", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, BOX])
  mount()
  await user.click(await screen.findByRole("button", { name: "Add host" }))
  const dialog = await screen.findByRole("dialog")
  const labels = within(dialog)
    .getAllByRole("radio")
    .map((radio) => radio.closest("label")?.textContent)
  expect(labels).toEqual(["Default", ...HOST_COLORS.map((color) => color.label)])
  expect(dialog.querySelectorAll("[data-host-color]")).toHaveLength(8)
})

it("sends only the changed fields of a named host, an emptied remote as null and Default as null", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    { local: { "/v1/hosts/far": () => jsonResponse({ ...SSH_BOX, remote: null }) } },
    [LOCAL, { ...SSH_BOX, color: "teal" }],
  )
  mount()
  await user.click(await screen.findByRole("button", { name: "Edit far" }))
  const dialog = await screen.findByRole("dialog")
  expect((screen.getByLabelText("Name") as HTMLInputElement).readOnly).toBe(true)

  // Nothing changed yet: the colour alone moves, to Default.
  await user.click(within(dialog).getByRole("radio", { name: "Default" }))
  await user.click(within(dialog).getByRole("button", { name: "Save changes" }))
  await waitFor(() => expect(bodies(seen, "PATCH")).toHaveLength(1))
  expect(bodies(seen, "PATCH")[0]).toEqual({ color: null })

  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  await user.click(screen.getByRole("button", { name: "Edit far" }))
  const again = await screen.findByRole("dialog")
  type("Remote", "")
  type("Endpoint", "ssh://me@elsewhere")
  await user.click(within(again).getByRole("button", { name: "Save changes" }))
  await waitFor(() => expect(bodies(seen, "PATCH")).toHaveLength(2))
  expect(bodies(seen, "PATCH")[1]).toEqual({ endpoint: "ssh://me@elsewhere", remote: null })
})

it("edits the colour of local alone", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    { local: { "/v1/hosts/local": () => jsonResponse({ ...LOCAL, color: "blue" }) } },
    [LOCAL, BOX],
  )
  mount()
  await user.click(await screen.findByRole("button", { name: "Edit local" }))
  const dialog = await screen.findByRole("dialog")
  expect(screen.queryByLabelText("Name")).toBeNull()
  expect(screen.queryByLabelText("Endpoint")).toBeNull()
  expect(screen.queryByLabelText("Remote")).toBeNull()
  await user.click(within(dialog).getByRole("radio", { name: "Blue" }))
  await user.click(within(dialog).getByRole("button", { name: "Save changes" }))
  await waitFor(() => expect(bodies(seen, "PATCH")).toEqual([{ color: "blue" }]))
  expect(seen.find((one) => one.method === "PATCH")?.url.pathname).toBe("/v1/hosts/local")
})

it("shows a refusal at the field it names, and any other in the dialog", async () => {
  const user = userEvent.setup()
  let answer = errorResponse(
    400,
    "bad_request",
    "hosts.x.endpoint must be ssh://[user@]host[:port]",
  )
  stubHosts({ local: { "/v1/hosts": hostsRoute([LOCAL], () => answer) } }, [LOCAL])
  mount()
  await user.click(await screen.findByRole("button", { name: "Add host" }))
  const dialog = await screen.findByRole("dialog")
  type("Name", "x")
  type("Endpoint", "nope")
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))
  const endpointField = screen.getByLabelText("Endpoint")
  await waitFor(() => expect(endpointField.getAttribute("aria-invalid")).toBe("true"))
  expect(within(dialog).getByText(/must be ssh/)).toBeTruthy()

  answer = errorResponse(409, "conflict", "host name exists")
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))
  await waitFor(() =>
    expect(screen.getByLabelText("Name").getAttribute("aria-invalid")).toBe("true"),
  )
  expect(within(dialog).getByText(/host name exists/)).toBeTruthy()

  answer = errorResponse(409, "conflict", "the hosts file changed")
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))
  expect(await within(dialog).findByText(/the hosts file changed/)).toBeTruthy()
  expect(screen.getByLabelText("Name").getAttribute("aria-invalid")).toBeNull()

  answer = errorResponse(500, "internal", "the database is locked")
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))
  expect(await within(dialog).findByText(/the database is locked/)).toBeTruthy()
  expect(screen.getByLabelText("Name").getAttribute("aria-invalid")).toBeNull()
})

it("disables the submit button while its request runs", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    {
      local: {
        "/v1/hosts": hostsRoute([LOCAL], () => new Promise<Response>(() => {})),
      },
    },
    [LOCAL],
  )
  mount()
  await user.click(await screen.findByRole("button", { name: "Add host" }))
  const dialog = await screen.findByRole("dialog")
  type("Name", "x")
  type("Endpoint", "http://x:1")
  const submit = within(dialog).getByRole("button", { name: "Add host" }) as HTMLButtonElement
  await user.click(submit)
  await waitFor(() => expect(submit.disabled).toBe(true))
  expect(seen.filter((one) => one.method === "POST")).toHaveLength(1)
})

it("removes a named host after a confirmation that says the far daemon keeps running", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    { local: { "/v1/hosts/box": () => new Response(null, { status: 204 }) } },
    [LOCAL, BOX],
  )
  const { queryClient } = mount()
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(3))
  expect(screen.queryByRole("button", { name: "Remove local" })).toBeNull()

  await user.click(screen.getByRole("button", { name: "Remove box" }))
  const dialog = await screen.findByRole("dialog")
  expect(within(dialog).getByText(/daemon on it keeps running/)).toBeTruthy()
  expect(seen.some((one) => one.method === "DELETE")).toBe(false)
  await user.click(within(dialog).getByRole("button", { name: "Remove host" }))

  await waitFor(() => expect(seen.filter((one) => one.method === "DELETE")).toHaveLength(1))
  expect(seen.find((one) => one.method === "DELETE")?.origin).toBe(LOCAL_ORIGIN)
  act(() => {
    dispatchDomainEvent(queryClient, "local", { event: "host_deleted", data: BOX })
  })
  await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(2))
  expect(screen.queryByRole("dialog")).toBeNull()
})

it("shows the error of a failed removal in a toast", async () => {
  const user = userEvent.setup()
  stubHosts(
    { local: { "/v1/hosts/box": () => errorResponse(404, "unknown_host", "unknown host") } },
    [LOCAL, BOX],
  )
  mount()
  await user.click(await screen.findByRole("button", { name: "Remove box" }))
  const dialog = await screen.findByRole("dialog")
  await user.click(within(dialog).getByRole("button", { name: "Remove host" }))
  expect(await screen.findByText("Could not remove box")).toBeTruthy()
})

it("sends color null when a new host keeps the Default colour", async () => {
  const user = userEvent.setup()
  const seen = stubHosts(
    { local: { "/v1/hosts": hostsRoute([LOCAL], () => jsonResponse(NEW_BOX, 201)) } },
    [LOCAL],
  )
  mount()
  await user.click(await screen.findByRole("button", { name: "Add host" }))
  const dialog = await screen.findByRole("dialog")
  type("Name", "new-box")
  type("Endpoint", "ssh://me@new-box")
  await user.click(within(dialog).getByRole("button", { name: "Add host" }))
  await waitFor(() => expect(bodies(seen, "POST")).toHaveLength(1))
  expect(bodies(seen, "POST")[0]).toEqual({
    name: "new-box",
    endpoint: "ssh://me@new-box",
    color: null,
  })
})

// @vitest-environment jsdom

/**
 * The row actions of the Hosts screen: Update, Continue setup and Discard
 * setup, over `GET /v1/host-setups`.
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { act } from "react"
import { afterEach, expect, it, vi } from "vitest"
import type { components } from "@/api/schema"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { HostsPage } from "./hosts-page"

type HostSetup = components["schemas"]["HostSetupDto"]

afterEach(() => {
  forgetHosts()
})

const STEP = {
  name: "probe",
  title: "Check the machine",
  complete: true,
  error: null,
  group: null,
  manual: null,
}

const STOPPED: HostSetup = {
  name: "builder",
  operation: "setup",
  ssh: "ssh://me@builder",
  state: "failed",
  steps: [
    STEP,
    {
      ...STEP,
      name: "service",
      title: "Start the Ariadne service",
      complete: false,
      error: "no lingering",
      manual: { reason: "Turn on lingering.", command: 'sudo loginctl enable-linger "$USER"' },
    },
  ],
  tools: null,
}

const SAVED: HostSetup = {
  ...STOPPED,
  name: "box",
  state: "complete",
  steps: [STEP],
  tools: { agents: ["codex-acp"], github: true },
}

const UPDATE_DTO = { name: "box", state: "running", steps: [{ ...STEP, complete: false }] }

function stub(setups: HostSetup[], extra: Record<string, unknown> = {}) {
  return stubHosts(
    {
      local: {
        "/v1/host-setups": setups,
        "/v1/forge/tunnel": { enabled: false, state: "off" },
        "/v1/acp-agents": [
          { id: "codex-acp", status: "ready" },
          { id: "claude-acp", status: "ready" },
        ],
        ...extra,
      },
    },
    [LOCAL, BOX],
  )
}

function row(name: string): HTMLElement {
  const found = screen
    .getAllByRole("row")
    .find((one) => within(one).queryAllByRole("cell")[0]?.textContent === name)
  if (!found) throw new Error(`no row for ${name}`)
  return found
}

it("shows a row for each saved setup that is not complete, with its state and Continue and Discard actions", async () => {
  stub([STOPPED, SAVED])
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("builder")).toBeTruthy())
  const builder = row("builder")
  expect(within(builder).getByText("Setup failed")).toBeTruthy()
  expect(within(builder).getByRole("button", { name: "Continue setup" })).toBeTruthy()
  expect(within(builder).getByRole("button", { name: "Discard setup" })).toBeTruthy()
  expect(within(row("box")).queryByRole("button", { name: "Continue setup" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Update worker" })).toBeNull()
})

it("offers no Discard setup while the setup runs", async () => {
  stub([{ ...STOPPED, state: "running" }])
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("builder")).toBeTruthy())
  expect(within(row("builder")).queryByRole("button", { name: "Discard setup" })).toBeNull()
})

it("opens the Install step of the setup from Continue setup", async () => {
  stub([STOPPED], { "/v1/hosts/builder/setup": { ...STOPPED } })
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("builder")).toBeTruthy())
  await userEvent
    .setup()
    .click(within(row("builder")).getByRole("button", { name: "Continue setup" }))

  const dialog = await screen.findByRole("dialog", { name: "Add machine" })
  expect(await within(dialog).findByText("Start the Ariadne service")).toBeTruthy()
  expect(within(dialog).getByText("Turn on lingering.")).toBeTruthy()
})

it("discards a stopped setup after a confirmation that says the remote files stay", async () => {
  const seen = stub([STOPPED], {
    "/v1/hosts/builder/setup": (_url: URL, request: Request) =>
      request.method === "DELETE" ? new Response(null, { status: 204 }) : undefined,
  })
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("builder")).toBeTruthy())
  const user = userEvent.setup()
  await user.click(within(row("builder")).getByRole("button", { name: "Discard setup" }))
  const dialog = await screen.findByRole("dialog", { name: "Discard the setup of builder?" })
  expect(within(dialog).getByText(/Files on the remote machine stay/)).toBeTruthy()
  await user.click(within(dialog).getByRole("button", { name: "Discard setup" }))

  await waitFor(() =>
    expect(
      seen.some((one) => one.method === "DELETE" && one.url.pathname === "/v1/hosts/builder/setup"),
    ).toBe(true),
  )
})

it("updates a machine with no name or SSH input, the programs of this machine and the saved tools selected", async () => {
  const seen = stub([SAVED], {
    "/v1/hosts/box/update": (_url: URL, request: Request) =>
      request.method === "POST" ? jsonResponse(UPDATE_DTO) : jsonResponse(UPDATE_DTO),
  })
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("box")).toBeTruthy())
  const user = userEvent.setup()
  await user.click(within(row("box")).getByRole("button", { name: "Update" }))

  const dialog = await screen.findByRole("dialog", { name: "Update box" })
  expect(within(dialog).queryByLabelText(/name|ssh/i)).toBeNull()
  expect(
    (
      within(dialog).getByRole("checkbox", {
        name: "Update the Ariadne programs",
      }) as HTMLInputElement
    ).getAttribute("aria-checked"),
  ).toBe("true")
  expect(within(dialog).getByRole("checkbox", { name: "Codex" }).getAttribute("aria-checked")).toBe(
    "true",
  )
  expect(
    within(dialog).getByRole("checkbox", { name: "Claude" }).getAttribute("aria-checked"),
  ).toBe("false")
  expect(
    within(dialog).getByRole("checkbox", { name: "GitHub CLI" }).getAttribute("aria-checked"),
  ).toBe("true")
  expect(within(dialog).getByText(/waits for running tasks/)).toBeTruthy()
  expect(dialog.textContent).not.toMatch(/worker|source daemon machine|settings daemon/i)

  await user.click(within(dialog).getByRole("button", { name: "Update" }))
  await waitFor(() =>
    expect(
      seen.some((one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update"),
    ).toBe(true),
  )
  const posted = seen.find(
    (one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update",
  )
  expect(JSON.parse(posted?.body ?? "{}")).toEqual({
    source: { kind: "daemon" },
    tools: { agents: ["codex-acp"], github: true },
  })
  expect(await within(dialog).findByText("Check the machine")).toBeTruthy()
})

it("sends tools alone when the programs are not selected, and says why it waits when nothing is", async () => {
  const seen = stub([SAVED], { "/v1/hosts/box/update": UPDATE_DTO })
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("box")).toBeTruthy())
  fireEvent.click(within(row("box")).getByRole("button", { name: "Update" }))
  const dialog = await screen.findByRole("dialog", { name: "Update box" })
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Update the Ariadne programs" }))
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Codex" }))
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "GitHub CLI" }))
  expect(within(dialog).getByText("Select the programs or a tool.")).toBeTruthy()
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Claude" }))
  await userEvent.setup().click(within(dialog).getByRole("button", { name: "Update" }))

  await waitFor(() =>
    expect(
      seen.some((one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update"),
    ).toBe(true),
  )
  const posted = seen.find(
    (one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update",
  )
  expect(JSON.parse(posted?.body ?? "{}")).toEqual({
    tools: { agents: ["claude-acp"], github: false },
  })
})

it("lets a failed update change its selections and try again", async () => {
  const failed = {
    ...UPDATE_DTO,
    state: "failed",
    steps: [{ ...STEP, complete: false, error: "health check failed" }],
  }
  const seen = stub([{ ...SAVED, operation: "update", state: "failed" }], {
    "/v1/hosts/box/update": failed,
  })
  renderScreen(<HostsPage />)

  await waitFor(() => expect(row("box")).toBeTruthy())
  const user = userEvent.setup()
  await user.click(within(row("box")).getByRole("button", { name: "Update" }))
  const dialog = await screen.findByRole("dialog", { name: "Update box" })
  expect(await within(dialog).findByText("health check failed")).toBeTruthy()
  await user.click(within(dialog).getByRole("button", { name: "Back" }))
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "GitHub CLI" }))
  await user.click(within(dialog).getByRole("button", { name: "Update" }))

  await waitFor(() =>
    expect(
      seen.some((one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update"),
    ).toBe(true),
  )
  const posted = seen.find(
    (one) => one.method === "POST" && one.url.pathname === "/v1/hosts/box/update",
  )
  expect(JSON.parse(posted?.body ?? "{}")).toMatchObject({
    tools: { agents: ["codex-acp"], github: false },
  })
})

it("shows a stopped update of an existing host and a stopped setup as rows, continues an update in its dialog, and discards no update", async () => {
  stub(
    [
      {
        ...SAVED,
        operation: "update",
        state: "failed",
        steps: [{ ...STEP, complete: false, error: "no health" }],
      },
      STOPPED,
    ],
    { "/v1/hosts/box/update": { ...UPDATE_DTO, state: "failed" } },
  )
  renderScreen(<HostsPage />)

  await waitFor(() => expect(screen.getByText("Update failed")).toBeTruthy())
  expect(screen.getByText("Setup failed")).toBeTruthy()
  const discards = screen.getAllByRole("button", { name: "Discard setup" })
  expect(discards).toHaveLength(1)
  expect(screen.getAllByRole("button", { name: "Continue setup" })).toHaveLength(1)
  fireEvent.click(screen.getByRole("button", { name: "Continue update" }))
  expect(await screen.findByRole("dialog", { name: "Update box" })).toBeTruthy()
})

it("reads the saved setups again every second while an operation runs, and stops when it ends", async () => {
  let reads = 0
  stub([], {
    "/v1/host-setups": () => {
      reads += 1
      return jsonResponse([{ ...STOPPED, state: reads < 3 ? "running" : "failed" }])
    },
  })
  // Only the interval pair is faked: it is the one timer that paces the poll.
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] })
  try {
    renderScreen(<HostsPage />)
    expect(await screen.findByText("Setup running")).toBeTruthy()
    await act(() => vi.advanceTimersByTimeAsync(2100))
    expect(await screen.findByText("Setup failed")).toBeTruthy()
    const settled = reads
    await act(() => vi.advanceTimersByTimeAsync(3000))
    expect(reads).toBe(settled)
  } finally {
    vi.useRealTimers()
  }
})

// @vitest-environment jsdom

/**
 * The Permissions screen against a stubbed daemon.
 *
 * The AI tab: one card, one settings row. What is worth pinning is that every
 * fact of `GET /v1/permissions/ai` reaches the screen, that each control sends
 * only the field it changed the moment it changed, that the switch is what the
 * Python check gates, and that a refusal is toasted with the daemon's own
 * words rather than swallowed.
 *
 * The Learned tab has its own test file (`learned-tab.test.tsx`); what belongs
 * here is only which tab the screen opens on and how the URL follows a switch
 * between them.
 */

import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { AiPermissionsStatusDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { anAiPermissionsStatus } from "@/test/fixtures"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"
import { PermissionsPage } from "./permissions-page"

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []
/** What the stub answers a write with: the row as it stands after that write. */
let current: AiPermissionsStatusDto = anAiPermissionsStatus()

/** The last write that went out, whatever it was. */
function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/**
 * The daemon: `GET /v1/permissions/ai` always answers the current row, so
 * the screen loads. A write merges its body into that row and answers the
 * result — or is refused, as `failure` says, which only ever applies to a
 * write: the screen has to be on the card before a write can be tried against
 * it.
 *
 * The Learned tab has its own test file and its own stub; this one only has to
 * answer its two reads with nothing, so switching onto that tab from here does
 * not throw.
 */
function stubDaemon(failure?: { status: number; code: string; message: string }) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? (JSON.parse(raw) as Record<string, unknown>) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/repositories") return new Response(JSON.stringify([]), { status: 200 })
    if (pathname === "/v1/permissions/learned") {
      return new Response(JSON.stringify({ items: [] }), { status: 200 })
    }

    if (failure && request.method !== "GET") {
      const { status, code, message } = failure
      return errorResponse(status, code, message)
    }
    if (request.method !== "GET") {
      current = { ...current, ...body }
    }
    return new Response(JSON.stringify(current), {
      status: request.method === "POST" ? 202 : 200,
      headers: { "content-type": "application/json" },
    })
  })
}

beforeEach(() => {
  requests = []
  current = anAiPermissionsStatus()
  stubDaemon()
})

it("renders the switch, model pickers, thresholds, Refresh and facts without a refresh time", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  expect(
    await screen.findByRole("switch", { name: "Enable the AI permission model" }),
  ).toBeDefined()
  const allowThreshold = screen.getByRole("spinbutton", {
    name: "Allow threshold",
  }) as HTMLInputElement
  const denyThreshold = screen.getByRole("spinbutton", {
    name: "Deny threshold",
  }) as HTMLInputElement
  expect(allowThreshold.value).toBe("0.2000")
  expect(denyThreshold.value).toBe("0.8000")
  expect(screen.getByRole("combobox", { name: "Flavour" })).toBeDefined()
  expect(screen.getByRole("combobox", { name: "Device" })).toBeDefined()
  expect(screen.queryByLabelText("Daily refresh")).toBeNull()
  expect(screen.getByRole("button", { name: "Refresh" })).toBeDefined()
  expect(screen.queryByText("State")).toBeNull()

  expect(screen.queryByText("Checkpoints")).toBeNull()
  expect(screen.queryByText("Prompts")).toBeNull()
  expect(screen.queryByRole("button", { name: "Restore defaults" })).toBeNull()
})

it("shows the Thresholds, Model and Status and hardware section headings", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  expect(await screen.findByRole("heading", { name: "Thresholds" })).toBeDefined()
  expect(screen.getByRole("heading", { name: "Model" })).toBeDefined()
  expect(screen.getByRole("heading", { name: "Status and hardware" })).toBeDefined()
})

it("joins Test a request and Refresh in one button group in the card header", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  const testButton = await screen.findByRole("button", { name: "Test a request" })
  const refreshButton = screen.getByRole("button", { name: "Refresh" })
  const group = testButton.closest('[data-slot="button-group"]')

  expect(group).not.toBeNull()
  expect(group?.contains(refreshButton)).toBe(true)
  expect(testButton.closest("header")?.contains(group)).toBe(true)
})

it("opens the test dialog from the header's Test a request button", async () => {
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  await user.click(await screen.findByRole("button", { name: "Test a request" }))

  expect(await screen.findByRole("dialog")).toBeDefined()
})

it("puts the title, the badge, the switch and both icon buttons in one header row", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  const header = (await screen.findByRole("heading", { name: "AI" })).closest("header")
  expect(header).not.toBeNull()
  expect(header?.contains(screen.getByText("Disabled"))).toBe(true)
  expect(
    header?.contains(screen.getByRole("switch", { name: "Enable the AI permission model" })),
  ).toBe(true)
  expect(header?.contains(screen.getByRole("button", { name: "Test a request" }))).toBe(true)
  expect(header?.contains(screen.getByRole("button", { name: "Refresh" }))).toBe(true)
})

it("drops the old description line under the title", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  await screen.findByRole("switch", { name: "Enable the AI permission model" })
  expect(screen.queryByText(/A model that runs on this machine/)).toBeNull()
})

describe("the state badge", () => {
  it("shows Disabled for the disabled state", async () => {
    current = anAiPermissionsStatus({ state: "disabled" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(await screen.findByText("Disabled")).toBeDefined()
  })

  it("shows Installing for the installing state", async () => {
    current = anAiPermissionsStatus({ state: "installing" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(await screen.findByText("Installing")).toBeDefined()
  })

  it("shows Ready for the ready state", async () => {
    current = anAiPermissionsStatus({ state: "ready" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(await screen.findByText("Ready")).toBeDefined()
  })

  it("shows Failed for the failed state", async () => {
    current = anAiPermissionsStatus({ state: "failed" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(await screen.findByText("Failed")).toBeDefined()
  })
})

it("shows every fact the daemon answered with, behind Details", async () => {
  current = anAiPermissionsStatus({
    state: "ready",
    installed_release: "kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
    latest_release: "kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
    weights_present: true,
    endpoint: "http://127.0.0.1:8901",
    last_refresh_at: "2026-01-01T00:00:00Z",
    last_error: null,
  })
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  expect(await screen.findByText("Ready")).toBeDefined()
  expect(screen.queryByText("http://127.0.0.1:8901")).toBeNull()

  await user.click(await screen.findByRole("button", { name: "Details" }))

  expect(
    screen.getAllByText("kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"),
  ).toHaveLength(2)
  expect(screen.getByText("Yes")).toBeDefined()
  expect(screen.getByText("http://127.0.0.1:8901")).toBeDefined()
})

it("shows the last error as an alert at the top of the card", async () => {
  current = anAiPermissionsStatus({
    state: "failed",
    last_error: "the package could not be installed",
  })
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  const alert = await screen.findByRole("alert")
  expect(alert.textContent).toContain("the package could not be installed")
})

describe("the enabled switch", () => {
  it("sends enabled: true the moment it is turned on", async () => {
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("switch", { name: "Enable the AI permission model" }))

    await waitFor(() => expect(lastWrite()?.body).toEqual({ enabled: true }))
  })

  it("is disabled with the version found, where Python is too old", async () => {
    current = anAiPermissionsStatus({
      python: { path: "/usr/bin/python3", version: "3.9.1", ok: false },
    })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    const switchEl = await screen.findByRole("switch", { name: "Enable the AI permission model" })
    expect(switchEl.getAttribute("data-disabled")).not.toBeNull()
    expect(await screen.findByText(/The model needs Python 3.12 or 3.13/)).toBeDefined()
    expect(screen.getByText(/found 3.9.1/)).toBeDefined()
  })

  it("is disabled and says not found, where no interpreter was found at all", async () => {
    current = anAiPermissionsStatus({ python: { path: null, version: null, ok: false } })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(await screen.findByText(/not found/)).toBeDefined()
  })
})

it("sends the allow threshold typed, once the field is left", async () => {
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  const allowThreshold = await screen.findByRole("spinbutton", { name: "Allow threshold" })
  await user.clear(allowThreshold)
  await user.type(allowThreshold, "0.3")
  await user.tab()

  await waitFor(() => expect(lastWrite()?.body).toEqual({ allow_threshold: 0.3 }))
})

it("sends the deny threshold typed, once the field is left", async () => {
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  const denyThreshold = await screen.findByRole("spinbutton", { name: "Deny threshold" })
  await user.clear(denyThreshold)
  await user.type(denyThreshold, "0.9")
  await user.tab()

  await waitFor(() => expect(lastWrite()?.body).toEqual({ deny_threshold: 0.9 }))
})

it("toasts the daemon's own message on a refused threshold, and puts the value back", async () => {
  stubDaemon({
    status: 422,
    code: "invalid_request",
    message: "the allow threshold must stay under the deny threshold",
  })
  renderScreen(
    <>
      <Toaster />
      <PermissionsPage />
    </>,
    { route: "/permissions?tab=ai" },
  )

  const allowThreshold = (await screen.findByRole("spinbutton", {
    name: "Allow threshold",
  })) as HTMLInputElement
  // user-event rewrites the value of a number input to String(Number(v)) after its first focus.
  fireEvent.change(allowThreshold, { target: { value: "0.95" } })
  fireEvent.blur(allowThreshold)

  expect(
    await screen.findByText(/the allow threshold must stay under the deny threshold/),
  ).toBeDefined()
  await waitFor(() => expect(allowThreshold.value).toBe("0.2000"))
})

describe("the model pickers", () => {
  it("puts Flavour and Device in one inline row", async () => {
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    const flavourField = (await screen.findByRole("combobox", { name: "Flavour" })).closest(
      '[data-slot="field"]',
    )
    const deviceField = screen
      .getByRole("combobox", { name: "Device" })
      .closest('[data-slot="field"]')
    expect(flavourField).not.toBeNull()
    expect(flavourField?.parentElement).toBe(deviceField?.parentElement)
  })

  it("disables an unsupported flavour and shows its reason", async () => {
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("combobox", { name: "Flavour" }))

    expect(
      (await screen.findByRole("option", { name: /27b — needs 64 GB memory/ })).getAttribute(
        "data-disabled",
      ),
    ).not.toBeNull()
  })

  it("shows the resource reason before an unavailable platform reason", async () => {
    const user = userEvent.setup()
    const status = anAiPermissionsStatus()
    current = {
      ...status,
      flavours: status.flavours.map((option) =>
        option.flavour === "27b"
          ? {
              ...option,
              devices: [
                {
                  device: "mlx",
                  can_run: false,
                  reason: "MLX needs macOS on Apple Silicon",
                  slow: false,
                },
                {
                  device: "cuda",
                  can_run: false,
                  reason: "needs 24 GB VRAM, found 8 GB",
                  slow: false,
                },
                {
                  device: "cpu",
                  can_run: false,
                  reason: "needs 64 GB memory, found 32 GB",
                  slow: false,
                },
              ],
            }
          : option,
      ),
    }
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("combobox", { name: "Flavour" }))

    expect(
      await screen.findByRole("option", { name: /27b — needs 64 GB memory, found 32 GB/ }),
    ).toBeDefined()
  })

  it("disables an unavailable device and shows slow on CPU", async () => {
    const user = userEvent.setup()
    current = anAiPermissionsStatus({ flavour: "0.8b", device: "mlx" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("combobox", { name: "Device" }))

    expect(
      (await screen.findByRole("option", { name: /cuda — CUDA is unavailable/ })).getAttribute(
        "data-disabled",
      ),
    ).not.toBeNull()
    expect(await screen.findByRole("option", { name: /cpu — slow on CPU/ })).toBeDefined()
  })

  it("sends a flavour alone, then sends its flavour and device", async () => {
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("combobox", { name: "Flavour" }))
    await user.click(await screen.findByRole("option", { name: "0.8b" }))
    await waitFor(() => expect(lastWrite()?.body).toEqual({ flavour: "0.8b" }))

    await user.click(screen.getByRole("combobox", { name: "Device" }))
    await user.click(screen.getByRole("option", { name: /cpu — slow on CPU/ }))
    await waitFor(() => expect(lastWrite()?.body).toEqual({ flavour: "0.8b", device: "cpu" }))
  })

  it("disables both pickers while installing", async () => {
    current = anAiPermissionsStatus({ state: "installing" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    expect(
      (await screen.findByRole("combobox", { name: "Flavour" })).hasAttribute("disabled"),
    ).toBe(true)
    expect(screen.getByRole("combobox", { name: "Device" }).hasAttribute("disabled")).toBe(true)
  })

  it("shows the flavour refusal message", async () => {
    const user = userEvent.setup()
    stubDaemon({ status: 422, code: "flavour_unsupported", message: "needs 24 GB VRAM" })
    renderScreen(
      <>
        <Toaster />
        <PermissionsPage />
      </>,
      { route: "/permissions?tab=ai" },
    )

    await user.click(await screen.findByRole("combobox", { name: "Flavour" }))
    await user.click(await screen.findByRole("option", { name: "0.8b" }))

    expect(await screen.findByText(/needs 24 GB VRAM/)).toBeDefined()
  })
})

it("shows the flavour, device, state, memory and GPU in one status line", async () => {
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })
  expect(
    await screen.findByText("4b on mlx · Disabled · 64 GB · Apple M4 Max (48 GB)"),
  ).toBeDefined()
})

it("shows no GPU where the hardware probe found none", async () => {
  current = anAiPermissionsStatus({
    hardware: { os: "linux", arch: "x86_64", memory_bytes: 1536 * 1024 ** 3, gpu: null },
  })
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })
  expect(await screen.findByText("4b on mlx · Disabled · 1.5 TB · No GPU")).toBeDefined()
})

describe("Refresh", () => {
  it("posts to the refresh endpoint", async () => {
    current = anAiPermissionsStatus({ enabled: true, state: "ready" })
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    await user.click(await screen.findByRole("button", { name: "Refresh" }))

    await waitFor(() =>
      expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/permissions/ai/refresh" }),
    )
  })

  it("is disabled while the model is off", async () => {
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    const button = (await screen.findByRole("button", { name: "Refresh" })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
  })

  it("is disabled while an install is already running", async () => {
    current = anAiPermissionsStatus({ enabled: true, state: "installing" })
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

    const button = (await screen.findByRole("button", { name: "Refresh" })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
  })
})

it("toasts the daemon's own message on a refusal", async () => {
  current = anAiPermissionsStatus({ enabled: true, state: "ready" })
  stubDaemon({ status: 409, code: "ai_busy", message: "an install is already running" })
  const user = userEvent.setup()
  renderScreen(
    <>
      <Toaster />
      <PermissionsPage />
    </>,
    { route: "/permissions?tab=ai" },
  )

  await user.click(await screen.findByRole("button", { name: "Refresh" }))

  expect(await screen.findByText(/an install is already running/)).toBeDefined()
})

describe("tabs", () => {
  it("opens the Learned tab when the URL says nothing", async () => {
    renderScreen(<PermissionsPage />, { route: "/permissions" })

    expect(await screen.findByRole("tab", { name: "Learned", selected: true })).toBeDefined()
  })

  it("opens the Learned tab the URL asks for", async () => {
    renderScreen(<PermissionsPage />, { route: "/permissions?tab=learned" })

    expect(await screen.findByRole("tab", { name: "Learned", selected: true })).toBeDefined()
  })

  it("puts the picked tab on the URL", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<PermissionsPage />, { route: "/permissions" })

    await user.click(await screen.findByRole("tab", { name: "AI" }))

    expect(location.url).toBe("/permissions?tab=ai")
  })
})

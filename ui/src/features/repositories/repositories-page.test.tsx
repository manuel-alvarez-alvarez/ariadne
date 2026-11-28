// @vitest-environment jsdom

/**
 * The repositories screen against a stubbed daemon.
 *
 * The table is three columns straight off the DTO, and the one thing about it
 * worth asserting is that the path leaves with the user: it is shown cut short
 * to fit its column, and it is the *whole* path that has to reach the
 * clipboard. How short is the other half of that, and it is a layout the rows
 * of this table were 130px tall without. The rest of what is asserted here is the two places the daemon
 * can say no and the screen has to keep the user somewhere useful: an empty
 * list, which is the normal state of a fresh install, and the 409 that says a
 * goal or a task still holds the repository being removed. That refusal has to
 * stay on screen, and the button that caused it has to stop being clickable,
 * because nothing about clicking it again would change the answer.
 *
 * jsdom is asked for by this file alone (the docblock above): every other test
 * in the app is pure and has no business paying for a DOM.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

import type { ForgeTunnelDto, HostDto, RepositoryDto } from "@/api"
import { aForge, aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL } from "@/test/hosts"
import { RepositoriesPage } from "./repositories-page"

const ARIADNE: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
})

// The other row: another checkout, on another branch, with no description,
// whose agents learn their permissions.
const SANDBOX: RepositoryDto = aRepository({
  id: "01JREPO00000000000000SND",
  path: "/home/me/dev/sandbox",
  base_branch: "trunk",
  description: null,
  permission_mode: "learn",
  default_workflow: "release",
})

/** `DELETE /v1/repositories/{id}` answers this instead of 204, when set. */
let deleteFailure: { status: number; code: string; message: string } | null = null

/** What `GET /v1/forge/tunnel` answers: the tunnel is up unless a test says otherwise. */
let tunnel: ForgeTunnelDto

/** What `GET /v1/hosts` answers: `local` alone unless a test names another. */
let hosts: HostDto[]

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

function stubDaemon(repositories: RepositoryDto[]) {
  deleteFailure = null
  tunnel = aTunnel()
  hosts = [LOCAL]
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/forge/tunnel") return jsonResponse(tunnel)
    if (pathname === "/v1/hosts") return jsonResponse(hosts)
    if (request.method === "DELETE") {
      if (!deleteFailure) return new Response(null, { status: 204 })
      const { status, code, message } = deleteFailure
      return errorResponse(status, code, message)
    }
    return jsonResponse(repositories)
  })
}

/** The header's "Register repository", which the empty state repeats below. */
function registerButton(): HTMLElement {
  const [header] = screen.getAllByRole("button", { name: "Register repository" })
  if (!header) throw new Error("the screen offers no way to register a repository")
  return header
}

beforeEach(() => {
  stubDaemon([ARIADNE, SANDBOX])
})

// Testing Library only unmounts by itself under `globals: true`, which this
// project does not use — without this every screen stays in the document.

describe("RepositoriesPage", () => {
  it("lists what the daemon holds, and says so where a description is missing", async () => {
    renderScreen(<RepositoriesPage />)

    // The paths are truncated in the middle, so each one is on screen as two
    // halves under the title that carries it whole.
    expect(await screen.findByTitle(ARIADNE.path)).toBeDefined()
    expect(screen.getByText("main")).toBeDefined()
    expect(screen.getByText("The orchestrator itself.")).toBeDefined()
    expect(screen.getByTitle(SANDBOX.path)).toBeDefined()
    expect(screen.getByText("no description")).toBeDefined()
    expect(screen.getByText("Auto")).toBeDefined()
    expect(screen.getByText("Learn")).toBeDefined()
    expect(screen.queryByText("2 repositories")).toBeNull()
  })

  it("shows each repository workflow and opens its workflow", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<RepositoriesPage />)

    expect(await screen.findByRole("columnheader", { name: "Workflow" })).toBeDefined()
    expect(await screen.findByRole("link", { name: ARIADNE.default_workflow })).toBeDefined()

    await user.click(screen.getByRole("link", { name: SANDBOX.default_workflow }))

    expect(location.url).toBe("/workflows?workflow=release")
    expect(screen.queryByRole("dialog")).toBeNull()
  })

  it("shows smee green while the hook is live, and red with the error when it is not", async () => {
    const user = userEvent.setup()
    stubDaemon([
      {
        ...ARIADNE,
        forge: aForge({
          enabled: true,
          webhook: {
            state: "live",
            url: "https://smee.io/Tl13s4JZbHs6Ww60eXq7Ab?route=/webhooks/github/repo",
            error: null,
            last_delivery_at: "2026-10-07T10:00:00Z",
            fetch_error: null,
          },
        }),
      },
      {
        ...SANDBOX,
        forge: aForge({
          enabled: true,
          name: "sandbox",
          webhook: {
            state: "failed",
            url: null,
            error: "HTTP 403: needs admin rights",
            last_delivery_at: null,
            fetch_error: null,
          },
        }),
      },
    ])
    renderScreen(<RepositoriesPage />)

    const working = await screen.findByRole("img", { name: "smee: working" })
    expect(working.textContent).toBe("smee")
    expect(working.className).toContain("bg-status-done-soft")
    expect(screen.queryByText("2026-10-07T10:00:00Z")).toBeNull()
    await user.hover(working)
    expect(
      await screen.findByText("https://smee.io/Tl13s4JZbHs6Ww60eXq7Ab?route=/webhooks/github/repo"),
    ).toBeDefined()

    const failing = screen.getByRole("img", { name: "smee: not working" })
    expect(failing.className).toContain("bg-status-danger-soft")
    await user.hover(failing)
    expect(await screen.findByText("Webhook: HTTP 403: needs admin rights")).toBeDefined()
  })

  it("ends the webhook skeleton when the tunnel cannot be read", async () => {
    stubDaemon([{ ...ARIADNE, forge: aForge({ enabled: true }) }])
    const answer = daemonFetch.getMockImplementation()
    daemonFetch.mockImplementation(async (input, init) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      if (new URL(request.url).pathname === "/v1/forge/tunnel") {
        return errorResponse(500, "internal", "no tunnel")
      }
      return (answer as NonNullable<typeof answer>)(input, init)
    })
    renderScreen(<RepositoriesPage />)

    expect(await screen.findByTitle("The tunnel could not be read")).toBeDefined()
    expect(screen.queryByLabelText("Loading webhook mode")).toBeNull()
  })

  it("shows polling at once for a repository whose webhook is off, whatever the tunnel does", async () => {
    stubDaemon([{ ...ARIADNE, forge: aForge({ enabled: true, webhook_enabled: false }) }])
    const answer = daemonFetch.getMockImplementation()
    daemonFetch.mockImplementation(async (input, init) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      if (new URL(request.url).pathname === "/v1/forge/tunnel") {
        return errorResponse(500, "internal", "no tunnel")
      }
      return (answer as NonNullable<typeof answer>)(input, init)
    })
    renderScreen(<RepositoriesPage />)

    expect(await screen.findByRole("img", { name: "polling: working" })).toBeDefined()
    expect(screen.queryByTitle("The tunnel could not be read")).toBeNull()
  })

  it("shows polling with the tunnel off: green with how to turn it on, red with the fetch error", async () => {
    const user = userEvent.setup()
    stubDaemon([
      { ...ARIADNE, forge: aForge({ enabled: true }) },
      {
        ...SANDBOX,
        forge: aForge({
          enabled: true,
          name: "sandbox",
          webhook: {
            state: "polling",
            url: null,
            error: "tunnel off",
            last_delivery_at: null,
            fetch_error: "gh: HTTP 502",
          },
        }),
      },
    ])
    tunnel = aTunnel({ enabled: false, state: "off", url: null })
    renderScreen(<RepositoriesPage />)

    const working = await screen.findByRole("img", { name: "polling: working" })
    expect(working.textContent).toBe("polling")
    await user.hover(working)
    expect(await screen.findByText(/turn on the webhook tunnel in Settings/i)).toBeDefined()

    await user.hover(screen.getByRole("img", { name: "polling: not working" }))
    expect(await screen.findByText("gh: HTTP 502")).toBeDefined()
    expect(screen.queryByRole("img", { name: /smee/ })).toBeNull()
  })

  it("shows green polling with the webhook off, and says so with the interval", async () => {
    const user = userEvent.setup()
    stubDaemon([
      {
        ...ARIADNE,
        forge: aForge({ enabled: true, webhook_enabled: false, poll_interval_minutes: 15 }),
      },
    ])
    // The tunnel is up, yet this repository registers no hook.
    renderScreen(<RepositoriesPage />)

    const pill = await screen.findByRole("img", { name: "polling: working" })
    expect(pill.className).toContain("bg-status-done-soft")
    await user.hover(pill)
    expect(await screen.findByText("The webhook is off for this repository.")).toBeDefined()
    expect(await screen.findByText(/every 15 minutes/)).toBeDefined()
  })

  it("shows amber polling when the forge refused the hook for rights, and says how to fix it", async () => {
    const user = userEvent.setup()
    stubDaemon([
      {
        ...ARIADNE,
        forge: aForge({
          enabled: true,
          host: "github.example.com",
          webhook: {
            state: "polling",
            url: null,
            error:
              'gh: Not Found (HTTP 404)\nThis API operation needs the "admin:repo_hook" scope.',
            last_delivery_at: null,
            fetch_error: null,
            rights_refused: true,
          },
        }),
      },
    ])
    // The tunnel is up, yet the forge refused the hook.
    renderScreen(<RepositoriesPage />)

    const pill = await screen.findByRole("img", { name: "polling: no webhook rights" })
    expect(pill.textContent).toBe("polling")
    expect(pill.className).toContain("bg-status-warn-soft")
    expect(screen.queryByRole("img", { name: /smee/ })).toBeNull()
    await user.hover(pill)
    expect(await screen.findByText(/admin rights on the repository/)).toBeDefined()
    expect(await screen.findByText(/admin:repo_hook scope on the gh token/)).toBeDefined()
    expect(
      await screen.findByText("gh auth refresh -h github.example.com -s admin:repo_hook"),
    ).toBeDefined()
    expect(await screen.findByText(/Turn off the webhook for this repository/)).toBeDefined()
  })

  it("shows the forge each remote is on over two lines, and whether it is enabled", async () => {
    stubDaemon([
      { ...ARIADNE, forge: aForge({ enabled: true, login: "octocat" }) },
      { ...SANDBOX, forge: aForge({ name: "sandbox" }) },
      aRepository({ id: "01JREPO00000000000000LOC", path: "/home/me/dev/local" }),
    ])
    renderScreen(<RepositoriesPage />)

    expect(await screen.findByRole("columnheader", { name: "Forge" })).toBeDefined()
    const enabled = await screen.findByTitle("github.com/acme/widgets")
    expect([...enabled.children].map((line) => line.textContent)).toEqual([
      "GitHub",
      "acme/widgets",
    ])
    const off = screen.getByTitle("github.com/acme/sandbox")
    expect([...off.children].map((line) => line.textContent)).toEqual([
      "GitHub · off",
      "acme/sandbox",
    ])
    const none = (await screen.findByTitle("/home/me/dev/local")).closest("tr")
    expect(none?.textContent).toContain("none")
  })

  it("says nothing about how work ends, which is the task's own", async () => {
    renderScreen(<RepositoriesPage />)
    await screen.findByTitle(ARIADNE.path)

    // The column that used to carry it is gone with the field behind it.
    expect(screen.queryByText("Merge strategy")).toBeNull()
    expect(screen.queryByText("Direct")).toBeNull()
    expect(screen.queryByText("Pull request")).toBeNull()
  })

  it("caps the path so the description has room to be a sentence", async () => {
    renderScreen(<RepositoriesPage />)

    // The path is the widest value in the row and the one that refuses to
    // shrink on its own — its last segment is what the middle ellipsis keeps —
    // so below `lg` it is capped outright. Uncapped, it took the width the
    // description needed and left it wrapping a word to a line.
    const path = (await screen.findByTitle(ARIADNE.path)).closest("td")
    expect(path?.className).toContain("max-w-36")
    expect(path?.className).toContain("lg:max-w-96")

    const description = screen.getByText("The orchestrator itself.").closest("td")
    expect(description?.className).toContain("min-w-48")
  })

  it("copies the whole path, which is longer than what the column shows", async () => {
    const user = userEvent.setup()
    renderScreen(<RepositoriesPage />)

    const [path] = await screen.findAllByRole("button", { name: "Copy repository path" })
    if (!path) throw new Error("the first row offers no way to copy its path")
    await user.click(path)

    expect(await navigator.clipboard.readText()).toBe(ARIADNE.path)

    const [branch] = screen.getAllByRole("button", { name: "Copy base branch" })
    if (!branch) throw new Error("the first row offers no way to copy its base branch")
    await user.click(branch)

    expect(await navigator.clipboard.readText()).toBe(ARIADNE.base_branch)
  })

  it("offers the way out of an empty list, which is where a fresh install starts", async () => {
    const user = userEvent.setup()
    stubDaemon([])
    renderScreen(<RepositoriesPage />)

    // The same words the new-goal dialog uses for the same state.
    expect(await screen.findByText("No repositories registered")).toBeDefined()

    // Two buttons say it — the header's and the empty state's own.
    await user.click(registerButton())

    expect(await screen.findByRole("dialog")).toBeDefined()
    expect(screen.getByLabelText("Path")).toBeDefined()
  })

  it("opens the edit dialog on the repository of the row it was clicked in", async () => {
    const user = userEvent.setup()
    renderScreen(<RepositoriesPage />)

    await user.click(await screen.findByRole("button", { name: `Edit ${SANDBOX.path}` }))

    const path = (await screen.findByLabelText("Path")) as HTMLInputElement
    expect(path.value).toBe(SANDBOX.path)
    expect((screen.getByLabelText("Base branch") as HTMLInputElement).value).toBe("trunk")
  })
})

describe("removing a repository", () => {
  it("keeps the daemon's refusal on screen and stops offering the click", async () => {
    const user = userEvent.setup()
    renderScreen(<RepositoriesPage />)
    deleteFailure = {
      status: 409,
      code: "conflict",
      message: `repository ${ARIADNE.id} is still used by 2 goals`,
    }

    await user.click(await screen.findByRole("button", { name: `Remove ${ARIADNE.path}` }))
    await user.click(await screen.findByRole("button", { name: "Remove repository" }))

    expect(await screen.findByText("This repository is still in use")).toBeDefined()
    expect(screen.getByText(/still used by 2 goals/)).toBeDefined()
    // The dialog stays up, and re-confirming would only ask the same question.
    expect(screen.getByRole("button", { name: "Remove repository" }).hasAttribute("disabled")).toBe(
      true,
    )
  })

  it("closes on a 204, which is the whole of a successful delete", async () => {
    const user = userEvent.setup()
    renderScreen(<RepositoriesPage />)

    await user.click(await screen.findByRole("button", { name: `Remove ${SANDBOX.path}` }))
    await user.click(await screen.findByRole("button", { name: "Remove repository" }))

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Remove repository" })).toBeNull()
    })
  })
})

describe("hosts", () => {
  afterEach(() => forgetHosts())

  it("says which hosts hold a checkout of each repository and which lack one", async () => {
    stubDaemon([ARIADNE, { ...SANDBOX, forge: aForge({ name: "sandbox" }) }])
    hosts = [
      LOCAL,
      {
        ...BOX,
        sync: {
          state: "failed",
          error: "connection reset",
          missing_repositories: [{ id: SANDBOX.id, forge: null }],
          skipped_permissions: 0,
        },
      },
    ]
    renderScreen(<RepositoriesPage />)

    expect(await screen.findByText("failed: connection reset")).toBeTruthy()
    const [ariadne, sandbox] = await screen.findAllByRole("list", { name: "Checkouts" })
    expect(ariadne?.textContent).toBe("box · checkout")
    expect(sandbox?.textContent).toBe("box · no checkout")
  })

  it("says a repository with no forge has none to match a checkout by, not that it has no checkout", async () => {
    hosts = [
      LOCAL,
      {
        ...BOX,
        sync: {
          state: "in_sync",
          error: null,
          missing_repositories: [{ id: SANDBOX.id, forge: null }],
          skipped_permissions: 0,
        },
      },
    ]
    renderScreen(<RepositoriesPage />)

    const [ariadne, sandbox] = await screen.findAllByRole("list", { name: "Checkouts" })
    expect(ariadne?.textContent).toBe("box · checkout")
    expect(sandbox?.textContent).toBe("box · no forge to match by")
  })

  it("shows a host as pending, not as holding a checkout, until its first sync completes", async () => {
    hosts = [
      LOCAL,
      {
        ...BOX,
        sync: { state: "pending", error: null, missing_repositories: [], skipped_permissions: 0 },
      },
    ]
    renderScreen(<RepositoriesPage />)

    const [ariadne, sandbox] = await screen.findAllByRole("list", { name: "Checkouts" })
    expect(ariadne?.textContent).toBe("box · pending")
    expect(sandbox?.textContent).toBe("box · pending")
  })

  it("reads a pending sync of a host the app cannot reach as waiting for host", async () => {
    hosts = [
      LOCAL,
      {
        ...BOX,
        name: "far",
        kind: "ssh",
        endpoint: null,
        status: "down",
        error: "no forward",
        sync: { state: "pending", error: null, missing_repositories: [], skipped_permissions: 0 },
      },
    ]
    renderScreen(<RepositoriesPage />)

    const [ariadne] = await screen.findAllByRole("list", { name: "Checkouts" })
    expect(ariadne?.textContent).toBe("far · waiting for host")
  })
})

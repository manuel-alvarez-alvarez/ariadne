// @vitest-environment jsdom
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { aForge, aRepository, aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { RepositoryFormDialog } from "./repository-form-dialog"

/** Routes `GET /v1/workflows` and `POST /v1/forge/detect` the same way every
 * create-form test needs them, plus whatever `extra` wants. */
function stubDaemon(
  detected: unknown,
  extra?: (
    pathname: string,
    request: Request,
  ) => Response | Promise<Response | undefined> | undefined,
) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build-test" })])
    if (pathname === "/v1/forge/detect" && request.method === "POST") return jsonResponse(detected)
    const extraResponse = await extra?.(pathname, request)
    if (extraResponse) return extraResponse
    return new Response("not found", { status: 404 })
  })
}

/** Registering with no workflow picked is refused before it reaches the daemon. */
it("refuses to register without a workflow picked", async () => {
  const user = userEvent.setup()
  const post = vi.fn(async () => new Response("not found", { status: 404 }))
  stubDaemon(null, (pathname, request) => {
    if (pathname === "/v1/repositories" && request.method === "POST") return post()
    return undefined
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  await user.type(await screen.findByLabelText("Path"), "/home/me/dev/widgets")
  await user.click(screen.getByRole("button", { name: /register/i }))

  expect(await screen.findByText("Pick a workflow.")).toBeDefined()
  expect(post).not.toHaveBeenCalled()
})

/** Turning the switch off sends `supervise_posts: false` on save. */
it("saves the supervise posts switch", async () => {
  const user = userEvent.setup()
  const repository = aRepository({ supervise_posts: true, forge: aForge({ enabled: true }) })
  const put = vi.fn(async () => jsonResponse({ ...repository, supervise_posts: false }))
  let sentBody: unknown
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build-test" })])
    if (pathname === `/v1/repositories/${repository.id}` && request.method === "PUT") {
      sentBody = await request.clone().json()
      return put()
    }
    return new Response("not found", { status: 404 })
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={repository} />)

  const switchControl = await screen.findByRole("switch", { name: "Supervise posts" })
  expect(switchControl.getAttribute("aria-checked")).toBe("true")
  await user.click(switchControl)
  await user.click(screen.getByRole("button", { name: /save changes/i }))

  expect(put).toHaveBeenCalled()
  expect(sentBody).toMatchObject({ supervise_posts: false })
})

/** The create form probes `POST /v1/forge/detect` once the path loses focus. */
it("shows the forge switch, off, once a path with a forge is entered", async () => {
  const user = userEvent.setup()
  stubDaemon({
    kind: "github",
    host: "github.com",
    owner: "acme",
    name: "widgets",
    remote: "origin",
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  await user.type(await screen.findByLabelText("Path"), "/home/me/dev/widgets")
  await user.tab()

  const switchControl = await screen.findByRole("switch", { name: "GitHub integration" })
  expect(switchControl.getAttribute("aria-checked")).toBe("false")
  expect(screen.getByTestId("forge-remote").textContent).toContain("github.com/acme/widgets")
})

/** A checkout with no GitHub or GitLab remote shows no switch at all. */
it("shows no forge switch for a path with no forge", async () => {
  const user = userEvent.setup()
  stubDaemon(null)
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  await user.type(await screen.findByLabelText("Path"), "/home/me/dev/widgets")
  await user.tab()

  await screen.findByLabelText("Base branch") // Something settled before asserting an absence.
  expect(screen.queryByRole("switch", { name: /integration/i })).toBeNull()
})

/** Turning the detected switch on sends `forge: {enabled: true}` on register. */
it("sends forge enabled in the create request once the switch is turned on", async () => {
  const user = userEvent.setup()
  const post = vi.fn(async () =>
    jsonResponse(aRepository({ forge: aForge({ enabled: true }) }), 201),
  )
  let sentBody: unknown
  stubDaemon(
    { kind: "github", host: "github.com", owner: "acme", name: "widgets", remote: "origin" },
    async (pathname, request) => {
      if (pathname === "/v1/repositories" && request.method === "POST") {
        sentBody = await request.clone().json()
        return post()
      }
      return undefined
    },
  )
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  await user.type(await screen.findByLabelText("Path"), "/home/me/dev/widgets")
  await user.tab()
  await user.click(await screen.findByRole("switch", { name: "GitHub integration" }))
  await user.click(screen.getByLabelText("Default workflow"))
  await user.click(await screen.findByRole("option", { name: "build-test" }))
  await user.click(screen.getByRole("button", { name: /register/i }))

  expect(post).toHaveBeenCalled()
  expect(sentBody).toMatchObject({ forge: { enabled: true } })
})

/**
 * Supervise posts sits below the forge switch and only shows while it is on,
 * in both forms; turning the forge off hides it without losing what it held.
 */
it("hides supervise posts under an off forge switch, keeping its value", async () => {
  const user = userEvent.setup()
  const repository = aRepository({ supervise_posts: true, forge: aForge({ enabled: true }) })
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build-test" })])
    return new Response("not found", { status: 404 })
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={repository} />)

  const forgeSwitch = await screen.findByRole("switch", { name: "GitHub integration" })
  const superviseSwitch = screen.getByRole("switch", { name: "Supervise posts" })
  await user.click(superviseSwitch) // off, while still shown

  await user.click(forgeSwitch) // forge off: supervise posts disappears
  expect(screen.queryByRole("switch", { name: "Supervise posts" })).toBeNull()

  await user.click(forgeSwitch) // forge back on: its value held at off
  expect(screen.getByRole("switch", { name: "Supervise posts" }).getAttribute("aria-checked")).toBe(
    "false",
  )
})

/**
 * A re-probed path answers differently than the one the switch was turned
 * on for, so the stale "on" must not survive it — neither the switch itself,
 * which this path no longer has one for, nor supervise posts below it.
 */
it("resets the switch and hides supervise posts once a re-probed path has no forge", async () => {
  const user = userEvent.setup()
  const detected = {
    kind: "github",
    host: "github.com",
    owner: "acme",
    name: "widgets",
    remote: "origin",
  }
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build-test" })])
    if (pathname === "/v1/forge/detect" && request.method === "POST") {
      const body = (await request.clone().json()) as { path: string }
      return jsonResponse(body.path === "/home/me/dev/widgets" ? detected : null)
    }
    return new Response("not found", { status: 404 })
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  const pathInput = await screen.findByLabelText("Path")
  await user.type(pathInput, "/home/me/dev/widgets")
  await user.tab()
  const forgeSwitch = await screen.findByRole("switch", { name: "GitHub integration" })
  await user.click(forgeSwitch)
  expect(forgeSwitch.getAttribute("aria-checked")).toBe("true")
  expect(screen.getByRole("switch", { name: "Supervise posts" })).toBeDefined()

  await user.clear(pathInput)
  await user.type(pathInput, "/home/me/dev/other")
  await user.tab()

  await waitFor(() => expect(screen.queryByRole("switch", { name: /integration/i })).toBeNull())
  expect(screen.queryByRole("switch", { name: "Supervise posts" })).toBeNull()
})

/**
 * A path that turns relative after a forge was detected and switched on
 * answers no detection at all — the probe never asks the daemon about a
 * path that cannot be a checkout — so the stale switch must still clear.
 */
it("resets the switch once the path turns relative after a forge was detected", async () => {
  const user = userEvent.setup()
  stubDaemon({
    kind: "github",
    host: "github.com",
    owner: "acme",
    name: "widgets",
    remote: "origin",
  })
  renderScreen(<RepositoryFormDialog open onOpenChange={vi.fn()} repository={null} />)

  const pathInput = await screen.findByLabelText("Path")
  await user.type(pathInput, "/home/me/dev/widgets")
  await user.tab()
  const forgeSwitch = await screen.findByRole("switch", { name: "GitHub integration" })
  await user.click(forgeSwitch)

  await user.clear(pathInput)
  await user.type(pathInput, "relative/dir")
  await user.tab()

  await waitFor(() => expect(screen.queryByRole("switch", { name: /integration/i })).toBeNull())
  expect(screen.queryByRole("switch", { name: "Supervise posts" })).toBeNull()
})

// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { aRepository, aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { RepositoryFormDialog } from "./repository-form-dialog"

/** Registering with no workflow picked is refused before it reaches the daemon. */
it("refuses to register without a workflow picked", async () => {
  const user = userEvent.setup()
  const post = vi.fn(async () => new Response("not found", { status: 404 }))
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build" })])
    if (pathname === "/v1/repositories" && request.method === "POST") return post()
    return new Response("not found", { status: 404 })
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
  const repository = aRepository({ supervise_posts: true })
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

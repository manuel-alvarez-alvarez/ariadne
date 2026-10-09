// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { aWorkflow } from "@/test/fixtures"
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

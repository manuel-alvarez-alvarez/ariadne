// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkerSetup } from "./worker-setup"

function mockSetupFetch(handler: (request: Request) => Promise<Response>) {
  daemonFetch.mockImplementation(handler)
}

it("starts an explicit update with selected tools and shows maintenance progress", async () => {
  const requests: Request[] = []
  mockSetupFetch(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([])
    requests.push(request)
    return jsonResponse({
      name: "builder",
      state: "incomplete",
      steps: [{ name: "maintenance", complete: true, error: null }],
    })
  })
  renderScreen(<WorkerSetup />)
  expect(screen.queryByLabelText("SSH address")).toBeNull()
  fireEvent.click(screen.getByLabelText("Update agent tools"))
  for (const [label, value] of [
    ["Worker name", "builder"],
    ["CLI binary path on the source daemon machine", "/cli"],
    ["Daemon binary path on the source daemon machine", "/daemon"],
    ["Required agent IDs", "codex-acp"],
  ] as const)
    fireEvent.change(screen.getByLabelText(label), { target: { value } })
  await userEvent.setup().click(screen.getByRole("button", { name: "Start or retry update" }))
  await waitFor(() => expect(screen.getByText("maintenance: complete")).toBeTruthy())
  const posted = requests.find((request) => request.method === "POST")
  expect(posted?.url).toContain("/v1/hosts/builder/update")
  expect(await posted?.clone().json()).toMatchObject({
    tools: { agents: ["codex-acp"], github: false },
  })
  expect(screen.queryByText("Worker repositories")).toBeNull()
})

it("allows a tools-only update without binary paths or SSH inputs", async () => {
  const bodies: unknown[] = []
  mockSetupFetch(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([])
    if (request.method === "POST") bodies.push(await request.clone().json())
    return jsonResponse({ name: "builder", state: "complete", steps: [] })
  })
  renderScreen(<WorkerSetup />)
  fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "builder" } })
  fireEvent.click(screen.getByLabelText("Update Ariadne binaries"))
  fireEvent.click(screen.getByLabelText("Update agent tools"))
  fireEvent.change(screen.getByLabelText("Required agent IDs"), { target: { value: "codex-acp" } })
  await userEvent.setup().click(screen.getByRole("button", { name: "Start or retry update" }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ tools: { agents: ["codex-acp"], github: false } })
})

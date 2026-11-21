// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkerSetup } from "./worker-setup"

it("selects repositories with a home workspace default and an editable destination", async () => {
  const bodies: unknown[] = []
  daemonFetch.mockImplementation(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories")
      return jsonResponse([{ id: "source", path: "/source/project", forge: { name: "project" } }])
    if (request.method === "POST") bodies.push(await request.clone().json())
    return jsonResponse({ name: "builder", state: "incomplete", steps: [] })
  })
  renderScreen(<WorkerSetup />)
  const user = userEvent.setup()
  await user.type(screen.getByLabelText("Worker name"), "builder")
  await user.type(screen.getByLabelText("SSH address"), "ssh://box")
  await user.type(screen.getByLabelText("CLI binary path on the source daemon machine"), "/cli")
  await user.type(
    screen.getByLabelText("Daemon binary path on the source daemon machine"),
    "/daemon",
  )
  await user.click(await screen.findByLabelText("/source/project"))
  const destination = screen.getByLabelText("Destination for /source/project")
  expect((destination as HTMLInputElement).value).toBe("~/workspace/project")
  await user.clear(destination)
  await user.type(destination, "/srv/project")
  await user.click(screen.getByRole("button", { name: "Start or retry setup" }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toMatchObject({ repositories: { source: "/srv/project" } })
})

it("starts setup and shows retained steps and a failure", async () => {
  const requests: Request[] = []
  const fetch = vi.fn((request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories")
      return Promise.resolve(jsonResponse([]))
    requests.push(request)
    return Promise.resolve(
      jsonResponse({
        name: "builder",
        state: "incomplete",
        steps: [
          { name: "install", complete: true, error: null },
          { name: "service", complete: false, error: "Enable user services" },
        ],
      }),
    )
  })
  daemonFetch.mockImplementation(fetch)
  renderScreen(<WorkerSetup />)
  const user = userEvent.setup()
  await user.type(screen.getByLabelText("Worker name"), "builder")
  await user.type(screen.getByLabelText("SSH address"), "ssh://user@builder")
  await user.type(
    screen.getByLabelText("CLI binary path on the source daemon machine"),
    "/tmp/ariadne",
  )
  await user.type(
    screen.getByLabelText("Daemon binary path on the source daemon machine"),
    "/tmp/ariadned",
  )
  await user.click(screen.getByRole("button", { name: "Start or retry setup" }))
  await waitFor(() => expect(screen.getByText("Enable user services")).toBeTruthy())
  expect(screen.getByText("install: complete")).toBeTruthy()
  expect(screen.getByText("incomplete")).toBeTruthy()
  const posted = requests.find((request) => request.method === "POST")
  expect(posted?.url).toContain("/v1/hosts/builder/setup")
  expect(await posted?.clone().json()).toEqual({
    ssh: "ssh://user@builder",
    remote: null,
    source: { kind: "local", cli: "/tmp/ariadne", daemon: "/tmp/ariadned" },
  })
  await user.click(screen.getByRole("button", { name: "Start or retry setup" }))
  await waitFor(() =>
    expect(requests.filter((request) => request.method === "POST")).toHaveLength(2),
  )
  const retried = requests.filter((request) => request.method === "POST")[1]
  if (!retried) throw new Error("no retry request")
  expect(await retried.clone().json()).toEqual(await posted?.clone().json())
})

it("sends explicit tool and credential selections", async () => {
  const bodies: unknown[] = []
  daemonFetch.mockImplementation(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([])
    if (request.method === "POST") bodies.push(await request.clone().json())
    return jsonResponse({ name: "builder", state: "incomplete", steps: [] })
  })
  renderScreen(<WorkerSetup />)
  const user = userEvent.setup()
  await user.type(screen.getByLabelText("Worker name"), "builder")
  await user.type(screen.getByLabelText("SSH address"), "ssh://box")
  await user.type(screen.getByLabelText("CLI binary path on the source daemon machine"), "/cli")
  await user.type(
    screen.getByLabelText("Daemon binary path on the source daemon machine"),
    "/daemon",
  )
  await user.type(screen.getByLabelText("Required agent IDs"), "codex-acp")
  await user.click(screen.getByLabelText("Install GitHub CLI"))
  await user.click(screen.getByLabelText("Copy Codex file credentials"))
  await user.click(screen.getByRole("button", { name: "Start or retry setup" }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toMatchObject({
    tools: { agents: ["codex-acp"], github: true },
    credentials: ["codex"],
  })
})

it("starts an explicit update with selected tools and shows maintenance progress", async () => {
  const requests: Request[] = []
  daemonFetch.mockImplementation(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([])
    requests.push(request)
    return jsonResponse({
      name: "builder",
      state: "incomplete",
      steps: [{ name: "maintenance", complete: true, error: null }],
    })
  })
  renderScreen(<WorkerSetup update />)
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
  daemonFetch.mockImplementation(async (request: Request) => {
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([])
    if (request.method === "POST") bodies.push(await request.clone().json())
    return jsonResponse({ name: "builder", state: "complete", steps: [] })
  })
  renderScreen(<WorkerSetup update />)
  fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "builder" } })
  fireEvent.click(screen.getByLabelText("Update Ariadne binaries"))
  fireEvent.click(screen.getByLabelText("Update agent tools"))
  fireEvent.change(screen.getByLabelText("Required agent IDs"), { target: { value: "codex-acp" } })
  await userEvent.setup().click(screen.getByRole("button", { name: "Start or retry update" }))
  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(bodies[0]).toEqual({ tools: { agents: ["codex-acp"], github: false } })
})

// @vitest-environment jsdom
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkerSetup } from "./worker-setup"

it("starts setup and shows retained steps and a failure", async () => {
  const requests: Request[] = []
  const fetch = vi.fn((request: Request) => {
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

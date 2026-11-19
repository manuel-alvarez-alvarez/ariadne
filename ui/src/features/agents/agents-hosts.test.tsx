// @vitest-environment jsdom

/** The agents screen over a settings daemon and a named host. */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

import { useSettingsStore } from "@/stores/settings"
import { aModel, anAgentConfig } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { AgentsPage } from "./agents-page"

afterEach(() => {
  vi.restoreAllMocks()
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

const MODEL = "claude-agent-acp:claude-opus-5"

function answers(flag: string, enabled: boolean) {
  return {
    "/v1/agents": [anAgentConfig({ extra_flags: [flag], default_flags: [] })],
    "/v1/models": [aModel({ id: MODEL, enabled })],
    "/v1/acp-agents": [],
    "/v1/models/enabled": (_url: URL, request: Request) =>
      request.method === "PUT" ? jsonResponse(aModel({ id: MODEL, enabled: !enabled })) : undefined,
  }
}

it("groups an agent of two hosts under one tab, with a chip per host, and writes to the chip's host", async () => {
  const seen = stubHosts({ local: answers("--on-local", true), box: answers("--on-box", true) })
  renderScreen(<AgentsPage />)

  // One tab for the agent id, though both hosts have it.
  expect(await screen.findAllByRole("tab", { name: /claude-agent-acp/ })).toHaveLength(1)
  const chips = screen.getByRole("group", { name: "Host" })
  expect(chips.textContent).toBe("localbox")
  expect(await screen.findByText("--on-local")).toBeTruthy()

  await userEvent.click(screen.getByRole("button", { name: "box" }))
  expect(await screen.findByText("--on-box")).toBeTruthy()

  await userEvent.click(screen.getByRole("switch"))
  await waitFor(() => {
    const write = seen.find((request) => request.method === "PUT")
    expect(write?.origin).toBe("http://box.test:7676")
    expect(JSON.parse(write?.body ?? "{}")).toEqual({ id: MODEL, enabled: false })
  })
  expect(seen.filter((request) => request.method === "PUT")).toHaveLength(1)
})

it("reads only the picked host, with no chips", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({ local: answers("--on-local", true), box: answers("--on-box", true) })
  renderScreen(<AgentsPage />)

  expect(await screen.findByText("--on-box")).toBeTruthy()
  expect(screen.queryByRole("group", { name: "Host" })).toBeNull()
  expect(
    seen.some(
      (request) =>
        request.origin === "http://127.0.0.1:7676" && request.url.pathname === "/v1/agents",
    ),
  ).toBe(false)
})

it("keeps the chip of local when a named host goes down", async () => {
  stubHosts({ local: answers("--on-local", true) }, [
    LOCAL,
    { ...BOX, status: "down", error: "connection refused" },
  ])
  renderScreen(<AgentsPage />)

  expect(await screen.findByText("--on-local")).toBeTruthy()
  await waitFor(() => expect(screen.getByRole("group", { name: "Host" }).textContent).toBe("local"))
})

it("says which host's agents failed to load beside the agents of the host that answered, with a retry", async () => {
  let failing = true
  stubHosts({
    local: answers("--on-local", true),
    box: {
      ...answers("--on-box", true),
      "/v1/agents": () =>
        failing
          ? new Response(JSON.stringify({ error: { code: "boom", message: "box is broken" } }), {
              status: 500,
              headers: { "content-type": "application/json" },
            })
          : jsonResponse([anAgentConfig({ extra_flags: ["--on-box"], default_flags: [] })]),
    },
  })
  renderScreen(<AgentsPage />)

  expect(await screen.findByText("Could not load the agents on box")).toBeTruthy()
  expect(await screen.findByText("--on-local")).toBeTruthy()

  failing = false
  await userEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByRole("button", { name: "box" })).toBeTruthy()
  expect(screen.queryByText("Could not load the agents on box")).toBeNull()
})

const broken = () =>
  new Response(JSON.stringify({ error: { code: "boom", message: "it is broken" } }), {
    status: 500,
    headers: { "content-type": "application/json" },
  })

it("names the failed host beside a host that answered with no agents", async () => {
  stubHosts({
    local: { ...answers("--on-local", true), "/v1/agents": [] },
    box: { ...answers("--on-box", true), "/v1/agents": broken },
  })
  renderScreen(<AgentsPage />)

  expect(await screen.findByText("Could not load the agents on box")).toBeTruthy()
  expect(await screen.findByText("No agents")).toBeTruthy()
  expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy()
})

it("names every host whose agents failed, each with a retry, when none answered", async () => {
  stubHosts({
    local: { ...answers("--on-local", true), "/v1/agents": broken },
    box: { ...answers("--on-box", true), "/v1/agents": broken },
  })
  renderScreen(<AgentsPage />)

  expect(await screen.findByText("Could not load the agents on local")).toBeTruthy()
  expect(await screen.findByText("Could not load the agents on box")).toBeTruthy()
  expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(2)
})

const OTHER = "opencode"

function twoAgents(flag: string, models: unknown[]) {
  return {
    ...answers(flag, true),
    "/v1/agents": [
      anAgentConfig({ extra_flags: [flag], default_flags: [] }),
      anAgentConfig({ agent_id: OTHER, extra_flags: [`${flag}-${OTHER}`], default_flags: [] }),
    ],
    "/v1/models": models,
  }
}

const skeletons = () => document.querySelectorAll('[data-slot="skeleton"]').length

it("shows a loading table, not 'No models', while the picked host's models load", async () => {
  stubHosts({
    local: answers("--on-local", true),
    // The models of box never answer.
    box: { ...answers("--on-box", true), "/v1/models": () => new Promise<Response>(() => {}) },
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  expect(await screen.findByText("--on-box")).toBeTruthy()
  expect(skeletons()).toBeGreaterThan(0)
  expect(screen.queryByText(/No models/)).toBeNull()
})

it("says 'No models' once the picked host answered none", async () => {
  stubHosts({
    local: answers("--on-local", true),
    box: { ...answers("--on-box", true), "/v1/models": [] },
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  expect(await screen.findByText(/No models/)).toBeTruthy()
})

it("shows the error of the picked host's models in the table, with its name and Retry", async () => {
  let failing = true
  stubHosts({
    local: answers("--on-local", true),
    box: {
      ...answers("--on-box", true),
      "/v1/models": () => (failing ? broken() : jsonResponse([aModel({ id: MODEL })])),
    },
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  expect(
    await screen.findByText("Could not load the models on box: it is broken (boom)"),
  ).toBeTruthy()

  failing = false
  const retries = screen.getAllByRole("button", { name: "Retry" })
  await userEvent.click(retries[retries.length - 1] as HTMLElement)
  expect(await screen.findByRole("switch")).toBeTruthy()
  expect(screen.queryByText(/Could not load the models on box/)).toBeNull()
})

it("counts the models of the picked chip's host, and the models turned off there", async () => {
  stubHosts({
    local: answers("--on-local", true),
    box: {
      ...answers("--on-box", true),
      "/v1/models": [
        aModel({ id: "claude-agent-acp:a", enabled: false }),
        aModel({ id: "claude-agent-acp:b" }),
      ],
    },
  })
  renderScreen(<AgentsPage />)

  expect(await screen.findByLabelText("1 model")).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "box" }))
  expect(await screen.findByLabelText("2 models")).toBeTruthy()
  expect(screen.queryByLabelText("1 model")).toBeNull()

  await userEvent.hover(screen.getByRole("button", { name: "Refresh" }))
  expect(await screen.findAllByText(/1 model turned off on box/)).not.toHaveLength(0)
})

it("keeps the picked chip when the agent tab changes", async () => {
  stubHosts({
    local: twoAgents("--on-local", [aModel({ id: MODEL })]),
    box: twoAgents("--on-box", [aModel({ id: MODEL })]),
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  await userEvent.click(screen.getByRole("tab", { name: new RegExp(OTHER) }))
  expect(await screen.findByText(`--on-box-${OTHER}`)).toBeTruthy()
  expect(screen.getByRole("button", { name: "box", pressed: true })).toBeTruthy()
})

it("names the host in the flags dialog and in the toasts of a model change", async () => {
  const success = vi.spyOn(toast, "success").mockImplementation(() => "")
  stubHosts({ local: answers("--on-local", true), box: answers("--on-box", true) })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  await userEvent.click(screen.getByRole("button", { name: /Edit claude-agent-acp flags/ }))
  expect(await screen.findByRole("heading", { name: "claude-agent-acp flags on box" })).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }))

  await userEvent.click(screen.getByRole("switch"))
  await waitFor(() =>
    expect(success).toHaveBeenCalledWith("Model turned off on box", expect.anything()),
  )
})

it("opens with the chip of ?host= picked", async () => {
  stubHosts({ local: answers("--on-local", true), box: answers("--on-box", true) })
  renderScreen(<AgentsPage />, { route: "/agents?host=box" })

  expect(await screen.findByText("--on-box")).toBeTruthy()
  expect(screen.getByRole("button", { name: "box", pressed: true })).toBeTruthy()
})

it("names the host in the toast of a saved flag list", async () => {
  const success = vi.spyOn(toast, "success").mockImplementation(() => "")
  stubHosts({
    local: answers("--on-local", true),
    box: {
      ...answers("--on-box", true),
      "/v1/agents/claude-agent-acp": () =>
        jsonResponse(anAgentConfig({ extra_flags: ["--on-box"], default_flags: [] })),
    },
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  await userEvent.click(screen.getByRole("button", { name: /Edit claude-agent-acp flags/ }))
  await userEvent.click(await screen.findByRole("button", { name: "Save flags" }))
  await waitFor(() => expect(success).toHaveBeenCalledWith("Flags saved on box", expect.anything()))
})

it("names the host in the toast of a refused rank change", async () => {
  const error = vi.spyOn(toast, "error").mockImplementation(() => "")
  stubHosts({
    local: answers("--on-local", true),
    box: {
      ...answers("--on-box", true),
      "/v1/models/rank": () =>
        new Response(JSON.stringify({ error: { code: "boom", message: "refused" } }), {
          status: 500,
          headers: { "content-type": "application/json" },
        }),
    },
  })
  renderScreen(<AgentsPage />)

  await userEvent.click(await screen.findByRole("button", { name: "box" }))
  await userEvent.click(await screen.findByRole("combobox", { name: `${MODEL} rank` }))
  await userEvent.click(await screen.findByRole("option", { name: /^Fast/ }))
  await waitFor(() =>
    expect(error).toHaveBeenCalledWith("Could not change the rank on box", expect.anything()),
  )
})

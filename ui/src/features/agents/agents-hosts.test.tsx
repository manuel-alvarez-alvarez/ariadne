// @vitest-environment jsdom

/** The agents screen over a settings daemon and a named host. */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import { useSettingsStore } from "@/stores/settings"
import { aModel, anAgentConfig } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { AgentsPage } from "./agents-page"

afterEach(() => {
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

  expect(await screen.findByText("Could not load the agents of box")).toBeTruthy()
  expect(await screen.findByText("--on-local")).toBeTruthy()

  failing = false
  await userEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByRole("button", { name: "box" })).toBeTruthy()
  expect(screen.queryByText("Could not load the agents of box")).toBeNull()
})

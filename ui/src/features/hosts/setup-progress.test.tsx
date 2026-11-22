// @vitest-environment jsdom

/**
 * The Install step of "Add machine", over the settings daemon's
 * `GET /v1/hosts/{name}/setup` and `POST /v1/hosts/{name}/setup/resume`.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"
import type { components } from "@/api/schema"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { SetupProgress } from "./setup-progress"

type Setup = components["schemas"]["SetupDto"]

afterEach(() => {
  forgetHosts()
})

const LINGER = 'sudo loginctl enable-linger "$USER"'

const STOPPED: Setup = {
  name: "builder",
  state: "incomplete",
  steps: [
    {
      name: "probe",
      title: "Check the new machine",
      complete: true,
      error: null,
      group: "ariadne_installs",
      manual: null,
    },
    {
      name: "service",
      title: "Start the Ariadne service",
      complete: false,
      error: "systemd user services stop when you sign out",
      group: "manual",
      manual: {
        reason: "Ask an administrator to turn on lingering for this account, then resume setup.",
        command: LINGER,
      },
    },
  ],
}

const DONE: Setup = {
  ...STOPPED,
  state: "complete",
  steps: STOPPED.steps.map((step) => ({ ...step, complete: true, error: null })),
}

it("shows each step by its title, and a manual action with its reason, its command and a copy control", async () => {
  stubHosts({ local: { "/v1/hosts/builder/setup": STOPPED } }, [LOCAL])
  renderScreen(<SetupProgress name="builder" />)

  const steps = await screen.findByRole("list", { name: "Setup steps" })
  expect(
    within(steps)
      .getAllByRole("listitem")
      .map((step) => step.textContent),
  ).toEqual([
    "Check the new machine: Done",
    expect.stringMatching(/^Start the Ariadne service: Failed/),
  ])
  expect(screen.getByText("systemd user services stop when you sign out")).toBeTruthy()
  expect(
    screen.getByText(
      "Ask an administrator to turn on lingering for this account, then resume setup.",
    ),
  ).toBeTruthy()
  expect(screen.getByText(LINGER)).toBeTruthy()
  expect(screen.getByRole("button", { name: "Copy command" })).toBeTruthy()
  expect(screen.getByRole("status").textContent).toBe(
    "Setup stopped. Do the work below, then check again.",
  )
})

it("resumes the setup from Check again and shows the progress it answers with", async () => {
  const seen = stubHosts(
    {
      local: {
        "/v1/hosts/builder/setup": STOPPED,
        "/v1/hosts/builder/setup/resume": () => jsonResponse(DONE),
      },
    },
    [LOCAL],
  )
  renderScreen(<SetupProgress name="builder" />)

  const check = await screen.findByRole("button", { name: "Check again" })
  expect(document.getElementById(check.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
    "Start the Ariadne service",
  )
  await userEvent.setup().click(check)

  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(
      "Setup is complete. The new machine is in the hosts list.",
    ),
  )
  expect(
    seen.filter(
      (one) => one.method === "POST" && one.url.pathname === "/v1/hosts/builder/setup/resume",
    ),
  ).toHaveLength(1)
  expect(screen.queryByRole("button", { name: "Check again" })).toBeNull()
})

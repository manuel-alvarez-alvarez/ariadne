// @vitest-environment jsdom

/**
 * The Install step of "Add machine", over the settings daemon's
 * `GET /v1/hosts/{name}/setup`, `POST /v1/hosts/{name}/setup/resume` and
 * `POST /v1/hosts/{name}/setup/steps/{step}/skip`.
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
      skipped: false,
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
      skipped: false,
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

const GITHUB_MANUAL = {
  reason: "Install GitHub CLI on the new machine, then resume setup.",
  command: null,
}

type Step = Setup["steps"][number]

const PROBE: Step = {
  name: "probe",
  title: "Check the new machine",
  complete: true,
  error: null,
  group: "ariadne_installs",
  manual: null,
  skipped: false,
}

const GITHUB: Step = {
  name: "tool:github",
  title: "Install GitHub CLI",
  complete: false,
  error: "missing gh",
  group: "manual",
  manual: GITHUB_MANUAL,
  skipped: false,
}

const TOOL_FAILED: Setup = {
  name: "builder",
  state: "incomplete",
  steps: [
    PROBE,
    {
      name: "tool:git",
      title: "Install Git",
      complete: true,
      error: null,
      group: "already_present",
      manual: null,
      skipped: false,
    },
    GITHUB,
    {
      ...PROBE,
      name: "service",
      title: "Start the Ariadne service",
      complete: false,
    },
  ],
}

const TOOL_SKIPPED: Setup = {
  ...TOOL_FAILED,
  steps: TOOL_FAILED.steps.map((step) =>
    step.name === "tool:github" ? { ...step, skipped: true, group: "skipped" } : step,
  ),
}

it("offers Skip beside the manual work of a failed tool, shows it skipped, and continues the setup past it", async () => {
  const seen = stubHosts(
    {
      local: {
        "/v1/hosts/builder/setup": TOOL_FAILED,
        "/v1/hosts/builder/setup/steps/tool%3Agithub/skip": () => jsonResponse(TOOL_SKIPPED),
        "/v1/hosts/builder/setup/resume": () => jsonResponse(DONE),
      },
    },
    [LOCAL],
  )
  renderScreen(<SetupProgress name="builder" />)

  const skip = await screen.findByRole("button", { name: "Skip" })
  expect(screen.getAllByRole("button", { name: "Skip" })).toHaveLength(1)
  expect(document.getElementById(skip.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
    "Install GitHub CLI",
  )
  expect(screen.queryByRole("button", { name: "Continue setup" })).toBeNull()
  const user = userEvent.setup()
  await user.click(skip)

  const steps = await screen.findByRole("list", { name: "Setup steps" })
  await waitFor(() =>
    expect(within(steps).getAllByRole("listitem")[2]?.textContent).toMatch(
      /^Install GitHub CLI: Skipped/,
    ),
  )
  expect(screen.getByText(GITHUB_MANUAL.reason)).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Skip" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Check again" })).toBeNull()
  await user.click(screen.getByRole("button", { name: "Continue setup" }))

  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(
      "Setup is complete. The new machine is in the hosts list.",
    ),
  )
  const posts = seen.filter((one) => one.method === "POST").map((one) => one.url.pathname)
  expect(posts).toEqual([
    "/v1/hosts/builder/setup/steps/tool%3Agithub/skip",
    "/v1/hosts/builder/setup/resume",
  ])
})

it("offers no Skip for a failed step of Ariadne itself or of Git", async () => {
  stubHosts(
    {
      local: {
        "/v1/hosts/builder/setup": {
          ...STOPPED,
          steps: [...STOPPED.steps, { ...GITHUB, name: "tool:git", title: "Install Git" }],
        },
      },
    },
    [LOCAL],
  )
  renderScreen(<SetupProgress name="builder" />)

  expect(await screen.findAllByRole("button", { name: "Check again" })).toHaveLength(2)
  expect(screen.queryByRole("button", { name: "Skip" })).toBeNull()
})

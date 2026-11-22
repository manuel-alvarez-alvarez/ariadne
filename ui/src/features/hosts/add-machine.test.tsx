// @vitest-environment jsdom

/**
 * "Add machine" on the Hosts screen: the method step, and the four steps that
 * install Ariadne through SSH, over the settings daemon's setup routes.
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"
import type { components } from "@/api/schema"
import { useSettingsStore } from "@/stores/settings"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, LOCAL, type SeenRequest, stubHosts } from "@/test/hosts"
import { HostsPage } from "./hosts-page"

type Inspection = components["schemas"]["InspectionDto"]
type Review = components["schemas"]["SetupReviewDto"]

const INSPECTION: Inspection = {
  target: "x86_64-unknown-linux-gnu",
  checks: [],
  supported_agents: ["codex-acp", "claude-acp", "goose"],
  supported_credentials: ["codex", "github"],
  binaries: {
    method: "release",
    version: "1.4.0",
    tag: "v1.4.0",
    commit: null,
    uncommitted: false,
  },
  agents: [
    {
      id: "codex-acp",
      name: "Codex",
      on_this_machine: true,
      group: "ariadne_installs",
      manual: null,
      sign_in: "codex",
    },
    {
      id: "claude-acp",
      name: "Claude",
      on_this_machine: true,
      group: "already_present",
      manual: null,
      sign_in: "claude",
    },
    {
      id: "goose",
      name: "Goose",
      on_this_machine: false,
      group: "manual",
      manual: { reason: "Ariadne cannot install Goose.", command: "brew install goose" },
      sign_in: null,
    },
  ],
  github: { group: "ariadne_installs", manual: null },
  sign_ins: [
    { id: "codex", name: "Codex", copy_supported: true, command: "codex login" },
    { id: "github", name: "GitHub", copy_supported: true, command: "gh auth login" },
    { id: "claude", name: "Claude", copy_supported: false, command: "claude auth login" },
    { id: "ssh", name: "SSH keys", copy_supported: false, command: null },
  ],
}

const REVIEW: Review = {
  actions: [],
  inspection: INSPECTION,
  prerequisites: [],
  preserved: [],
  plan: [
    {
      id: "tool:codex-acp",
      title: "Install Codex",
      group: "ariadne_installs",
      detail: "Install Codex with npm.",
      manual: null,
    },
    {
      id: "tool:claude-acp",
      title: "Install Claude",
      group: "already_present",
      detail: "Claude is already on the new machine.",
      manual: null,
    },
    {
      id: "authentication:claude-acp",
      title: "Check the Claude sign-in",
      group: "manual",
      detail: "Claude is not signed in on the new machine.",
      manual: { reason: "Sign in to Claude on the new machine.", command: "claude auth login" },
    },
  ],
}

const PROJECT = { id: "r1", path: "/src/project", forge: { name: "project" } }

const FORBIDDEN = /worker|source daemon machine|settings daemon/i

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "" })
})

afterEach(() => {
  forgetHosts()
})

/** The settings daemon, with one registered repository and the setup routes of `builder`. */
function stubSetup() {
  return stubHosts(
    {
      local: {
        "/v1/repositories": [PROJECT],
        "/v1/hosts/builder/setup/inspect": () => jsonResponse(INSPECTION),
        "/v1/hosts/builder/setup/review": () => jsonResponse(REVIEW),
        "/v1/hosts/builder/setup": () =>
          jsonResponse({
            name: "builder",
            state: "running",
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
                name: "install",
                title: "Install the Ariadne programs",
                complete: false,
                error: null,
                group: "ariadne_installs",
                manual: null,
              },
            ],
          }),
      },
    },
    [LOCAL],
  )
}

function posts(seen: SeenRequest[], path: string): unknown[] {
  return seen
    .filter((one) => one.method === "POST" && one.url.pathname === path)
    .map((one) => JSON.parse(one.body || "{}"))
}

function nextButton(dialog: HTMLElement): HTMLButtonElement {
  return within(dialog).getByRole("button", { name: "Next" }) as HTMLButtonElement
}

/**
 * Opens the dialog at the Connect step of an installation. The steps before
 * the claim of a test go by `fireEvent`.
 */
async function openInstall(): Promise<HTMLElement> {
  fireEvent.click(screen.getByRole("button", { name: "Add machine" }))
  const dialog = await screen.findByRole("dialog")
  fireEvent.click(nextButton(dialog))
  await within(dialog).findByLabelText("SSH address")
  return dialog
}

/** Fills Connect and goes to Select. */
async function toSelect(): Promise<HTMLElement> {
  const dialog = await openInstall()
  fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "builder" } })
  fireEvent.change(within(dialog).getByLabelText("SSH address"), {
    target: { value: "ssh://me@builder" },
  })
  fireEvent.click(nextButton(dialog))
  await within(dialog).findByText("Agents and tools")
  return dialog
}

it("replaces Add host and Set up worker with one Add machine action that asks for the method", async () => {
  stubSetup()
  renderScreen(<HostsPage />)
  expect(screen.queryByRole("button", { name: "Add host" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Set up worker" })).toBeNull()

  await userEvent.setup().click(screen.getByRole("button", { name: "Add machine" }))
  const dialog = await screen.findByRole("dialog")
  const install = within(dialog).getByRole("radio", {
    name: "Install Ariadne on a machine through SSH",
  })
  const connect = within(dialog).getByRole("radio", {
    name: "Connect to a daemon that already runs",
  })
  const when = (radio: HTMLElement) =>
    document.getElementById(radio.getAttribute("aria-describedby") ?? "")?.textContent
  expect(when(install)).toBe("Use this when Ariadne is not on the new machine yet.")
  expect(when(connect)).toBe("Use this when the new machine already runs ariadned.")
  expect(dialog.textContent).not.toMatch(FORBIDDEN)
})

it("asks Connect for the name and the SSH address alone, says why Next waits, and inspects", async () => {
  const seen = stubSetup()
  renderScreen(<HostsPage />)
  const user = userEvent.setup()
  const dialog = await openInstall()

  const steps = within(dialog).getByRole("list", { name: "Steps" })
  expect(
    within(steps)
      .getAllByRole("listitem")
      .map((step) => step.textContent),
  ).toEqual(["1Connect", "2Select", "3Review", "4Install"])
  expect(within(steps).getByText("Connect").closest("li")?.getAttribute("aria-current")).toBe(
    "step",
  )
  expect(
    within(dialog)
      .getAllByRole("textbox")
      .map((box) => box.id),
  ).toHaveLength(2)
  expect(
    within(dialog).getByText(
      "Ariadne uses the SSH keys of this machine. It never asks for a password.",
    ),
  ).toBeTruthy()
  expect(nextButton(dialog).disabled).toBe(true)
  expect(within(dialog).getByText("Enter a name.")).toBeTruthy()
  fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "builder" } })
  expect(within(dialog).getByText("Enter the SSH address.")).toBeTruthy()

  await user.type(within(dialog).getByLabelText("SSH address"), "ssh://me@builder")
  expect(nextButton(dialog).disabled).toBe(false)
  await user.click(nextButton(dialog))

  await within(dialog).findByText("Agents and tools")
  expect(posts(seen, "/v1/hosts/builder/setup/inspect")).toEqual([{ ssh: "ssh://me@builder" }])
  expect(dialog.textContent).not.toMatch(FORBIDDEN)
})

it("shows the programs as one line and selects available agents, GitHub CLI and every repository", async () => {
  stubSetup()
  renderScreen(<HostsPage />)
  const dialog = await toSelect()

  expect(within(dialog).getByText("Release · version 1.4.0 · tag v1.4.0")).toBeTruthy()
  const checked = (name: string) =>
    within(dialog).getByRole("checkbox", { name }).getAttribute("aria-checked")
  expect(checked("Codex")).toBe("true")
  expect(checked("Claude")).toBe("true")
  expect(checked("Goose")).toBe("false")
  expect(checked("GitHub CLI")).toBe("true")
  expect(checked("project")).toBe("true")

  const line = (name: string) =>
    within(dialog).getByRole("checkbox", { name }).getAttribute("aria-describedby") ?? ""
  expect(document.getElementById(line("Codex"))?.textContent).toBe("Ariadne installs it.")
  expect(document.getElementById(line("Claude"))?.textContent).toBe("Already on the new machine.")
  expect(document.getElementById(line("Goose"))?.textContent).toContain("You install it by hand:")
  expect(document.getElementById(line("Goose"))?.textContent).toContain("brew install goose")

  const destination = (await within(dialog).findByLabelText(
    "Destination for project",
  )) as HTMLInputElement
  expect(destination.value).toBe("~/workspace/project")
  expect(dialog.textContent).not.toMatch(FORBIDDEN)
})

it("asks one sign-in choice per needed sign-in, and copies only where inspection permits it", async () => {
  const seen = stubSetup()
  renderScreen(<HostsPage />)
  const user = userEvent.setup()
  const dialog = await toSelect()

  const groups = within(dialog).getAllByRole("radiogroup")
  const names = groups.map((group) => group.getAttribute("aria-labelledby") ?? "")
  expect(names.map((id) => document.getElementById(id)?.textContent)).toEqual([
    "Codex",
    "GitHub",
    "Claude",
  ])
  const [codex, , claude] = groups as [HTMLElement, HTMLElement, HTMLElement]
  expect(
    within(codex)
      .getByRole("radio", { name: "Sign in on the new machine" })
      .getAttribute("aria-checked"),
  ).toBe("true")
  expect(within(codex).getByText("codex login")).toBeTruthy()
  const copyClaude = within(claude).getByRole("radio", { name: "Copy from this machine" })
  expect(copyClaude.getAttribute("aria-disabled")).toBe("true")
  expect(within(claude).getByText("Ariadne cannot copy the Claude sign-in.")).toBeTruthy()

  await user.click(within(codex).getByRole("radio", { name: "Copy from this machine" }))
  await user.click(within(dialog).getByRole("checkbox", { name: "Goose" }))
  await user.click(nextButton(dialog))

  await within(dialog).findByText("Ariadne installs")
  expect(posts(seen, "/v1/hosts/builder/setup/review")).toEqual([
    {
      ssh: "ssh://me@builder",
      tools: { agents: ["codex-acp", "claude-acp", "goose"], github: true },
      credentials: ["codex"],
      repositories: { r1: "~/workspace/project" },
    },
  ])
})

it("sends a source only when the Advanced inputs name the programs, and says why Next waits", async () => {
  const seen = stubSetup()
  renderScreen(<HostsPage />)
  const user = userEvent.setup()
  const dialog = await toSelect()

  await user.click(within(dialog).getByText("Advanced"))
  fireEvent.change(within(dialog).getByLabelText("CLI program path"), {
    target: { value: "/opt/ariadne" },
  })
  expect(nextButton(dialog).disabled).toBe(true)
  expect(within(dialog).getByText("Fill both program paths, or leave both empty.")).toBeTruthy()
  fireEvent.change(within(dialog).getByLabelText("Daemon program path"), {
    target: { value: "/opt/ariadned" },
  })
  fireEvent.change(within(dialog).getByLabelText("Remote socket"), {
    target: { value: "/run/ariadne.sock" },
  })
  await user.click(nextButton(dialog))

  await within(dialog).findByText("Ariadne installs")
  expect(posts(seen, "/v1/hosts/builder/setup/review")[0]).toMatchObject({
    remote: "/run/ariadne.sock",
    source: { kind: "local", cli: "/opt/ariadne", daemon: "/opt/ariadned" },
  })
})

it("reviews the plan in three groups, keeps the inputs on Back, and reviews a changed selection again", async () => {
  const seen = stubSetup()
  renderScreen(<HostsPage />)
  const user = userEvent.setup()
  const dialog = await toSelect()
  fireEvent.change(within(dialog).getByLabelText("Destination for project"), {
    target: { value: "~/code/project" },
  })
  await user.click(nextButton(dialog))

  const installs = await within(dialog).findByRole("region", { name: "Ariadne installs" })
  expect(within(installs).getByText("Install Codex")).toBeTruthy()
  const present = within(dialog).getByRole("region", { name: "Already on the machine" })
  expect(within(present).getByText("Install Claude")).toBeTruthy()
  const manual = within(dialog).getByRole("region", { name: "You install by hand" })
  expect(within(manual).getByText("Sign in to Claude on the new machine.")).toBeTruthy()
  expect(within(manual).getByText("claude auth login")).toBeTruthy()
  expect(within(dialog).getByText("Setup copies no sign-in from this machine.")).toBeTruthy()
  expect(dialog.textContent).not.toMatch(FORBIDDEN)

  await user.click(within(dialog).getByRole("button", { name: "Back" }))
  const destination = within(dialog).getByLabelText("Destination for project") as HTMLInputElement
  expect(destination.value).toBe("~/code/project")
  await user.click(within(dialog).getByRole("checkbox", { name: "GitHub CLI" }))
  await user.click(nextButton(dialog))
  await within(dialog).findByRole("region", { name: "Ariadne installs" })
  await user.click(within(dialog).getByRole("button", { name: "Install" }))

  await within(dialog).findByText("Install the Ariadne programs")
  const reviews = posts(seen, "/v1/hosts/builder/setup/review")
  expect(reviews).toHaveLength(2)
  expect(posts(seen, "/v1/hosts/builder/setup")).toEqual([reviews[1]])
  expect(reviews[1]).toEqual({
    ssh: "ssh://me@builder",
    tools: { agents: ["codex-acp", "claude-acp"], github: false },
    repositories: { r1: "~/code/project" },
  })
})

it("closes during the installation with no question, and the setup continues in the daemon", async () => {
  const seen = stubSetup()
  renderScreen(<HostsPage />)
  const user = userEvent.setup()
  const dialog = await toSelect()
  fireEvent.click(nextButton(dialog))
  fireEvent.click(await within(dialog).findByRole("button", { name: "Install" }))
  await within(dialog).findByText("Install the Ariadne programs")
  const steps = within(dialog).getByRole("list", { name: "Steps" })
  expect(within(steps).getByText("Install").closest("li")?.getAttribute("aria-current")).toBe(
    "step",
  )

  await user.click(within(dialog).getAllByRole("button", { name: "Close" })[0] as HTMLElement)

  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  expect(screen.queryByText("Discard this machine?")).toBeNull()
  expect(seen.some((one) => one.method === "DELETE")).toBe(false)
})

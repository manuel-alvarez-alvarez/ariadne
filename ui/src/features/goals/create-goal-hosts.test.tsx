// @vitest-environment jsdom

/**
 * The goal dialog across hosts: it reads the host it creates on, the model
 * picker's host column picks that host, and a change of host reloads the
 * repositories, the workflows and the models of the new host.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { useSettingsStore } from "@/stores/settings"
import { aGoal, aModel, anEffort, aRepository, aWorkflow } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { contentOf } from "@/test/text-content"
import { CreateGoalDialog } from "./create-goal-dialog"

type ToastOptions = { description: string; action: { label: string; onClick: () => void } }
const toasted = vi.hoisted(() => vi.fn<(title: string, options?: ToastOptions) => void>())
vi.mock("sonner", () => ({ toast: { success: toasted, error: vi.fn(), dismiss: vi.fn() } }))

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ goalStatusFilter: "", hostFilter: "" })
})

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

it("opens the goal dialog on local under All", async () => {
  stubHosts({ box: {} })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const pin = await screen.findByRole("button", { name: "Orchestrator runs on" })
  await waitFor(() => expect(pin.textContent).toContain("local"))
  expect(screen.queryByRole("combobox", { name: "Host" })).toBeNull()
})

it("defaults the goal dialog to the picked host, and reads that host only", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({
    local: {
      "/v1/repositories": [aRepository({ id: "r-local", path: "/src/local-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
    },
    box: {
      "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
    },
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const pin = await screen.findByRole("button", { name: "Orchestrator runs on" })
  await waitFor(() => expect(pin.textContent).toContain("box"))
  await waitFor(() =>
    expect(reads(seen, "box")).toEqual(["/v1/models", "/v1/repositories", "/v1/workflows"]),
  )
  expect(reads(seen, "local")).toEqual([])
})

it("reloads the lists of the new host when the goal dialog's host changes", async () => {
  const user = userEvent.setup()
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({
    local: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
    box: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  await pickHost(user, "local")

  await waitFor(() =>
    expect(reads(seen, "local")).toEqual(["/v1/models", "/v1/repositories", "/v1/workflows"]),
  )
})

/** Pick `name` in the host column of the open goal dialog's pin picker. */
async function pickHost(user: ReturnType<typeof userEvent.setup>, name: string) {
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await user.click(await within(dialog).findByRole("button", { name: "Orchestrator runs on" }))
  const hostsColumn = await screen.findByRole("listbox", { name: "Hosts" })
  await user.click(within(hostsColumn).getByText(name))
  await user.keyboard("{Escape}")
}

/** Pick `model` in the open goal dialog's pin picker. */
async function pickModel(user: ReturnType<typeof userEvent.setup>, id: string) {
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await user.click(await within(dialog).findByRole("button", { name: "Orchestrator runs on" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText(id))
  await user.keyboard("{Escape}")
  return dialog
}

/** A model that can be run at `low` or `high`. */
const EFFORT_MODEL = aModel({ efforts: [anEffort({ id: "low" }), anEffort({ id: "high" })] })

it("clears the model and the effort when the goal dialog's host changes", async () => {
  const user = userEvent.setup()
  const answers = {
    "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
    "/v1/models": [EFFORT_MODEL],
    "/v1/workflows": [],
    "/v1/goals": (_url: URL, request: Request) =>
      request.method === "POST" ? jsonResponse(aGoal({ title: "On box" }), 201) : undefined,
  }
  stubHosts({ local: answers, box: answers })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  const pin = await within(dialog).findByRole("button", { name: "Orchestrator runs on" })

  // A model and an effort of it, on local.
  await user.click(pin)
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(EFFORT_MODEL.id),
  )
  await user.click(
    within(await screen.findByRole("listbox", { name: "Efforts" })).getByText("high", {
      selector: "[data-slot=option-label]",
    }),
  )
  await waitFor(() => expect(pin.textContent).toContain("high"))
  if (screen.queryByRole("listbox", { name: "Efforts" })) await user.keyboard("{Escape}")

  await pickHost(user, "box")

  await waitFor(() => expect(pin.textContent).not.toContain("claude-sonnet-5"))
  expect(pin.textContent).not.toContain("high")
  expect(within(dialog).getByRole("button", { name: "Create goal" }).hasAttribute("disabled")).toBe(
    true,
  )
})

it("sends no effort for a model picked again after the goal dialog's host changes", async () => {
  const user = userEvent.setup()
  const answers = {
    "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
    "/v1/models": [EFFORT_MODEL],
    "/v1/workflows": [],
    "/v1/goals": (_url: URL, request: Request) =>
      request.method === "POST" ? jsonResponse(aGoal({ title: "On box" }), 201) : undefined,
  }
  const seen = stubHosts({ local: answers, box: answers })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await pickHost(user, "box")
  const pin = await within(dialog).findByRole("button", { name: "Orchestrator runs on" })
  await user.click(pin)
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(EFFORT_MODEL.id),
  )
  await user.keyboard("{Escape}")
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "On box")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))
  await waitFor(() => expect(seen.some((one) => one.method === "POST")).toBe(true))
  const body = JSON.parse(seen.find((one) => one.method === "POST")?.body ?? "{}")
  expect(body).toMatchObject({ model: EFFORT_MODEL.id })
  expect(body).not.toHaveProperty("effort")
})

async function warnsAndCreates(state: "failed" | "pending", error: string | null) {
  const user = userEvent.setup()
  useSettingsStore.setState({ hostFilter: "box" })
  const behind = { ...BOX, sync: { ...BOX.sync, state, error } }
  let created = false
  stubHosts(
    {
      local: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
      box: {
        "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
        "/v1/models": [aModel()],
        "/v1/workflows": [],
        "/v1/goals": (_url: URL, request: Request) => {
          if (request.method !== "POST") return undefined
          created = true
          return jsonResponse(aGoal({ title: "On box" }), 201)
        },
      },
    },
    [LOCAL, behind],
  )
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const alert = await screen.findByRole("alert")
  expect(alert.textContent).toContain(state)
  if (error) expect(alert.textContent).toContain(error)

  const dialog = await pickModel(user, aModel().id)
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "On box")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))
  await waitFor(() => expect(created).toBe(true))
}

it("warns in the goal dialog of a host whose sync is failed, and still creates", () =>
  warnsAndCreates("failed", "push refused: disk full"))

it("warns in the goal dialog of a host whose sync is pending, and still creates", () =>
  warnsAndCreates("pending", null))

it("names the host in the toast of a goal created on a named host, and offers all hosts when the pick leaves it out", async () => {
  const user = userEvent.setup()
  toasted.mockClear()
  useSettingsStore.setState({ hostFilter: "local" })
  stubHosts({
    local: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
    box: {
      "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
      "/v1/goals": (_url: URL, request: Request) =>
        request.method === "POST" ? jsonResponse(aGoal({ title: "Fresh goal" }), 201) : undefined,
    },
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await pickHost(user, "box")
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await pickModel(user, aModel().id)
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "Fresh goal")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))

  await waitFor(() => expect(toasted).toHaveBeenCalled())
  const [title, options] = toasted.mock.calls[0] ?? []
  expect(title).toBe("Goal created")
  expect(contentOf(options?.description)).toBe("Fresh goal on box")
  expect(options?.action.label).toBe("Show all hosts")
  act(() => options?.action.onClick())
  expect(useSettingsStore.getState().hostFilter).toBe("")
})

it("clears the repositories and the workflow when the picker's host changes, and keeps the picker last", async () => {
  const user = userEvent.setup()
  const answers = {
    "/v1/repositories": [
      aRepository({ id: "r-one", path: "/src/one-repo", default_workflow: "wf-one" }),
    ],
    "/v1/models": [aModel()],
    "/v1/workflows": [aWorkflow({ name: "wf-one" })],
  }
  const seen = stubHosts({ local: answers, box: answers })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  const workflow = await within(dialog).findByRole("combobox", { name: "Workflow" })
  const repositories = await within(dialog).findByRole("combobox", { name: "Repositories" })
  await user.click(repositories)
  await user.click(await screen.findByText(/one-repo/))
  await user.keyboard("{Escape}")
  expect(await within(dialog).findByText(/one-repo/)).toBeDefined()
  await waitFor(() => expect(workflow.textContent).toContain("wf-one"))

  await pickHost(user, "box")

  await waitFor(() => expect(within(dialog).queryByText(/one-repo/)).toBeNull())
  const nextWorkflow = await within(dialog).findByRole("combobox", { name: "Workflow" })
  await waitFor(() => expect(nextWorkflow.textContent).not.toContain("wf-one"))
  expect(nextWorkflow.textContent).toContain("Repository default")
  await waitFor(() => expect(reads(seen, "box")).toContain("/v1/repositories"))
  const fields = [...dialog.querySelectorAll("label")].map((label) => label.textContent)
  expect(fields.at(-1)).toBe("Orchestrator runs on")
})

/** The paths a host was asked to read, each once, sorted. */
function reads(seen: ReturnType<typeof stubHosts>, host: "local" | "box"): string[] {
  const origin = host === "box" ? "http://box.test:7676" : "http://127.0.0.1:7676"
  return [
    ...new Set(
      seen
        .filter((request) => request.origin === origin && request.url.pathname !== "/v1/hosts")
        .filter((request) => request.url.pathname !== "/v1/acp-agents")
        .map((request) => request.url.pathname),
    ),
  ].sort()
}

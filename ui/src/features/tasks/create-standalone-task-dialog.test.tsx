// @vitest-environment jsdom
import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { aModel, anEffort, aRepository, aTask, aWorkflow } from "@/test/fixtures"
import { errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, LOCAL, stubHosts } from "@/test/hosts"
import { CreateStandaloneTaskDialog } from "./create-standalone-task-dialog"

const REPO = aRepository({ path: "/work/ariadne" })
const WORKFLOW = aWorkflow({
  name: "build-test",
  steps: [
    { id: "build", title: "Build", description: "", skills: ["coding"], ranks: [] },
    { id: "test", title: "Test", description: "", skills: ["testing"], ranks: [] },
  ],
})
const MODEL = aModel({
  id: "codex-acp:gpt-5.5",
  agent_id: "codex-acp",
  efforts: [anEffort({ id: "high", default: true })],
})

function stubDaemon(hosts = [LOCAL, BOX]) {
  return stubHosts(
    {
      local: {
        "/v1/repositories": [REPO],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
      },
    },
    hosts,
  )
}

it("offers No workflow first, and one staffing row with no column under it", async () => {
  stubDaemon()
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  const workflow = await screen.findByRole("combobox", { name: "Workflow" })
  expect(workflow.textContent).toContain("No workflow")
  await user.click(workflow)
  const options = await screen.findAllByRole("option")
  expect(options[0]?.textContent).toBe("No workflow")
  expect(options.map((option) => option.textContent)).toContain("build-test")
  await user.keyboard("{Escape}")

  expect(screen.getAllByRole("button", { name: /runs on$/ })).toHaveLength(1)
  expect(screen.getByRole("button", { name: "Agent runs on" })).toBeTruthy()
})

it("staffs one row for each column of the picked workflow", async () => {
  stubDaemon()
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  await user.click(await screen.findByRole("combobox", { name: "Workflow" }))
  await user.click(await screen.findByRole("option", { name: "build-test" }))

  expect(await screen.findByRole("button", { name: "Build runs on" })).toBeTruthy()
  expect(screen.getByRole("button", { name: "Test runs on" })).toBeTruthy()
  expect(screen.getAllByRole("button", { name: /runs on$/ })).toHaveLength(2)
})

it("has no dependency field", async () => {
  stubDaemon()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)
  await screen.findByRole("combobox", { name: "Repository" })
  expect(screen.queryByText("Depends on")).toBeNull()
})

it("offers the ready hosts, starting on this machine", async () => {
  stubDaemon([LOCAL, BOX, { ...BOX, name: "cold", status: "down" }])
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  const host = await screen.findByRole("combobox", { name: "Host" })
  expect(host.textContent).toContain("local")
  await user.click(host)
  const names = (await screen.findAllByRole("option")).map((option) => option.textContent)
  expect(names).toEqual(["local", "box"])
})

it("picks the repository by search in the field the goal dialog uses", async () => {
  const other = aRepository({ id: "other", path: "/work/website" })
  stubHosts(
    {
      local: {
        "/v1/repositories": [REPO, other],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
      },
    },
    [LOCAL, BOX],
  )
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  const field = await screen.findByRole("combobox", { name: "Repository" })
  expect(field.textContent).toContain("Select a repository…")
  await user.click(field)
  await user.type(await screen.findByPlaceholderText("Search repositories…"), "website")
  expect(screen.queryByRole("option", { name: /^\/work\/ariadne/ })).toBeNull()
  await user.click(screen.getByRole("option", { name: /^\/work\/website/ }))

  // One pick closes the search and leaves the path in the field.
  expect(screen.queryByPlaceholderText("Search repositories…")).toBeNull()
  expect(screen.getByText("/work/website")).toBeTruthy()
})

it("creates the task in the repository with no workflow, on this machine", async () => {
  const task = aTask({ id: "01JNOGOAL", goal_id: null, title: "Fix the typo" })
  const seen = stubHosts(
    {
      local: {
        "/v1/repositories": [REPO],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
        "/v1/tasks": (_url: URL, request: Request) =>
          request.method === "POST" ? jsonResponse(task, 201) : undefined,
      },
    },
    [LOCAL, BOX],
  )
  const onCreated = vi.fn()
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} onCreated={onCreated} />)

  await user.type(await screen.findByLabelText("Title"), "Fix the typo")
  await user.click(screen.getByRole("combobox", { name: "Repository" }))
  await user.click(await screen.findByRole("option", { name: /^\/work\/ariadne/ }))
  await user.click(screen.getByRole("button", { name: "Agent runs on" }))
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(MODEL.id),
  )
  await user.click(within(screen.getByRole("listbox", { name: "Efforts" })).getByText("high"))
  await user.click(screen.getByRole("button", { name: "Create task" }))

  await waitFor(() => expect(onCreated).toHaveBeenCalledWith(task))
  const post = seen.find((one) => one.method === "POST" && one.url.pathname === "/v1/tasks")
  expect(JSON.parse(post?.body ?? "{}")).toEqual({
    title: "Fix the typo",
    description: "",
    repo_id: REPO.id,
    agents: [{ skills: [], model: MODEL.id, effort: "high" }],
  })
})

it("asks for a repository before it creates anything", async () => {
  const seen = stubDaemon()
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  await user.type(await screen.findByLabelText("Title"), "No repository")
  await user.click(screen.getByRole("button", { name: "Agent runs on" }))
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(MODEL.id),
  )
  await user.click(within(screen.getByRole("listbox", { name: "Efforts" })).getByText("high"))
  await user.click(screen.getByRole("button", { name: "Create task" }))

  expect(await screen.findByText("Choose a repository.")).toBeTruthy()
  expect(seen.some((one) => one.method === "POST")).toBe(false)
})

it("shows the daemon's reason when it refuses the host", async () => {
  stubHosts(
    {
      local: {
        "/v1/repositories": [REPO],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
        "/v1/tasks": (_url: URL, request: Request) =>
          request.method === "POST"
            ? errorResponse(400, "bad_request", "a goal-less task requires the local host")
            : undefined,
      },
      box: { "/v1/models": [MODEL], "/v1/acp-agents": [] },
    },
    [LOCAL, BOX],
  )
  const onCreated = vi.fn()
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} onCreated={onCreated} />)

  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Far task" } })
  fireEvent.click(screen.getByRole("combobox", { name: "Repository" }))
  fireEvent.click(await screen.findByRole("option", { name: /^\/work\/ariadne/ }))
  await user.click(screen.getByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "box" }))
  fireEvent.click(screen.getByRole("button", { name: "Agent runs on" }))
  fireEvent.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(MODEL.id),
  )
  fireEvent.click(screen.getByRole("button", { name: "Create task" }))

  expect(await screen.findByText(/a goal-less task requires the local host/)).toBeTruthy()
  expect(onCreated).not.toHaveBeenCalled()
})

it("shows the host picker directly above the staffing rows", async () => {
  stubDaemon()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  const host = await screen.findByRole("combobox", { name: "Host" })
  const agent = screen.getByRole("button", { name: "Agent runs on" })
  expect(host.compareDocumentPosition(agent) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
})

it("offers the models of the picked host in each staffing row", async () => {
  const BOX_MODEL = aModel({
    id: "claude-acp:opus",
    agent_id: "claude-acp",
    efforts: [anEffort({ id: "medium", default: true })],
  })
  stubHosts(
    {
      local: {
        "/v1/repositories": [REPO],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
      },
      box: { "/v1/models": [BOX_MODEL], "/v1/acp-agents": [] },
    },
    [LOCAL, BOX],
  )
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  await user.click(await screen.findByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "box" }))
  fireEvent.click(screen.getByRole("button", { name: "Agent runs on" }))

  const models = within(await screen.findByRole("listbox", { name: "Models" }))
  expect(models.getByText(BOX_MODEL.id)).toBeTruthy()
  expect(models.queryByText(MODEL.id)).toBeNull()
})

it("clears the picked model and effort of each row when the host changes", async () => {
  stubHosts(
    {
      local: {
        "/v1/repositories": [REPO],
        "/v1/workflows": [WORKFLOW],
        "/v1/models": [MODEL],
        "/v1/skills": [],
        "/v1/acp-agents": [],
      },
      box: { "/v1/models": [MODEL], "/v1/acp-agents": [] },
    },
    [LOCAL, BOX],
  )
  const user = userEvent.setup()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={vi.fn()} />)

  // Setup only: pick a model and effort, then close the picker.
  const pin = await screen.findByRole("button", { name: "Agent runs on" })
  fireEvent.click(pin)
  fireEvent.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(MODEL.id),
  )
  fireEvent.click(within(screen.getByRole("listbox", { name: "Efforts" })).getByText("high"))
  fireEvent.click(pin)
  expect(pin.textContent).toContain("high")

  await user.click(screen.getByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "box" }))

  const row = screen.getByRole("button", { name: "Agent runs on" })
  expect(row.textContent).not.toContain(MODEL.id)
  expect(row.textContent).not.toContain("high")
  expect(screen.getByRole("button", { name: "Create task" }).hasAttribute("disabled")).toBe(true)
})

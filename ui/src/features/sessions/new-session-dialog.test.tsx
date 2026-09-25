// @vitest-environment jsdom

/**
 * The new-session dialog: what it sends to `POST /v1/sessions`, what it
 * refuses before sending anything, and that the registered repositories are
 * offered as the directory.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import type { ModelDto, RepositoryDto } from "@/api"
import { aModel, anEffort, aRepository, aSession } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"

import { NewSessionDialog } from "./new-session-dialog"

const REPOSITORY: RepositoryDto = aRepository({ path: "/Users/me/dev/ariadne" })

const CATALOG: ModelDto[] = [
  aModel({
    id: "codex-acp:gpt-5.3-codex",
    agent_id: "codex-acp",
    efforts: [anEffort({ id: "low" }), anEffort({ id: "high", default: true })],
  }),
]

const STARTED = aSession({
  id: "01JSESS00000000000STARTED",
  goal_id: null,
  task_id: null,
  seat: null,
  task_agent_id: null,
  model: "codex-acp:gpt-5.3-codex",
})

/** The bodies posted to `/v1/sessions`, oldest first. */
const posted: unknown[] = []

function stubDaemon(start: () => Response = () => jsonResponse(STARTED)) {
  posted.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const { pathname } = new URL(request.url)
    if (pathname === "/v1/sessions" && request.method === "POST") {
      posted.push(await request.json())
      return start()
    }
    if (pathname === "/v1/models") return jsonResponse(CATALOG)
    if (pathname === "/v1/repositories") return jsonResponse([REPOSITORY])
    return jsonResponse([])
  })
}

function renderDialog() {
  const onStarted = vi.fn()
  const onOpenChange = vi.fn()
  renderScreen(<NewSessionDialog open onOpenChange={onOpenChange} onStarted={onStarted} />)
  return { onStarted, onOpenChange }
}

async function chooseModel(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Runs on" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText("codex-acp:gpt-5.3-codex"))
  await user.keyboard("{Escape}")
}

function directory(): HTMLInputElement {
  return screen.getByLabelText("Working directory") as HTMLInputElement
}

it("starts a session on the chosen model, in the typed directory, and hands it back", async () => {
  stubDaemon()
  const user = userEvent.setup()
  const { onStarted, onOpenChange } = renderDialog()

  await chooseModel(user)
  await user.type(directory(), "/Users/me/dev/ariadne")
  await user.click(screen.getByRole("button", { name: "Start session" }))

  await waitFor(() => expect(onStarted).toHaveBeenCalledWith(STARTED))
  expect(posted).toEqual([
    { model: "codex-acp:gpt-5.3-codex", working_directory: "/Users/me/dev/ariadne" },
  ])
  expect(onOpenChange).toHaveBeenCalledWith(false)
})

it("offers the registered repositories as the directory", async () => {
  stubDaemon()
  renderDialog()

  const list = directory().getAttribute("list")
  expect(list).not.toBeNull()
  await waitFor(() =>
    expect(
      [...(document.getElementById(list ?? "")?.querySelectorAll("option") ?? [])].map(
        (option) => option.value,
      ),
    ).toEqual([REPOSITORY.path]),
  )
})

it("refuses a relative directory before asking the daemon", async () => {
  stubDaemon()
  const user = userEvent.setup()
  renderDialog()

  await chooseModel(user)
  await user.type(directory(), "dev/ariadne")
  await user.click(screen.getByRole("button", { name: "Start session" }))

  await screen.findByText("Give an absolute path, starting with /.")
  expect(posted).toHaveLength(0)
})

it("shows the daemon's refusal and stays open", async () => {
  stubDaemon(() =>
    errorResponse(
      400,
      "bad_request",
      "the working directory must be the absolute path of a directory that exists: /nope",
    ),
  )
  const user = userEvent.setup()
  const { onStarted } = renderDialog()

  await chooseModel(user)
  await user.type(directory(), "/nope")
  await user.click(screen.getByRole("button", { name: "Start session" }))

  await screen.findByText(/a directory that exists: \/nope/)
  expect(onStarted).not.toHaveBeenCalled()
})

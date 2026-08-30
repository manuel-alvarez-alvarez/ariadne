// @vitest-environment jsdom

/**
 * The repository dialog against a stubbed daemon, from both ends of the form.
 *
 * Almost nothing here is client-side validation, and that is the point: only
 * the daemon can open a checkout, resolve a branch and check it has commits,
 * so the dialog's job is to send the right body and to put the daemon's answer
 * where the user can act on it. Each test is one of the ways that must not
 * curdle — an omitted branch has to reach the daemon as *absent* (which is
 * what asks for the repo's current branch) rather than as an empty string,
 * clearing a description has to reach it as an empty one (which is what clears
 * it), and a 400 has to land on the field it is about instead of a banner that
 * says "bad request" over a form that looks fine.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { MergeStrategyDto, RepositoryDto } from "@/api"
import { aMergeStrategy, aRepository, LANDING_PROMPT_DEFAULTS } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { RepositoryFormDialog } from "./repository-form-dialog"

const REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
})

const MERGE_STRATEGIES: MergeStrategyDto[] = [
  aMergeStrategy({ merge_strategy: "direct" }),
  aMergeStrategy({ merge_strategy: "pull_request" }),
]

interface Recorded {
  method: string
  path: string
  body: {
    path?: string
    base_branch?: string | null
    description?: string | null
    landing_prompt?: string
  } | null
}

let requests: Recorded[] = []

/** The last write that went out, whatever it was. */
function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/**
 * The daemon: `GET /v1/merge-strategies` answers with the fixed catalog, and
 * every write echoes back — or is refused, as `failure` says.
 */
function stubDaemon(failure?: { status: number; code: string; message: string }) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/merge-strategies") return jsonResponse(MERGE_STRATEGIES)

    if (failure) {
      const { status, code, message } = failure
      return errorResponse(status, code, message)
    }
    return new Response(JSON.stringify({ ...REPOSITORY, ...body }), {
      status: request.method === "POST" ? 201 : 200,
      headers: { "content-type": "application/json" },
    })
  })
}

function renderDialog(
  repository: RepositoryDto | null,
  onOpenChange: (open: boolean) => void = () => {},
) {
  return renderScreen(
    <RepositoryFormDialog open onOpenChange={onOpenChange} repository={repository} />,
  )
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

describe("registering a repository", () => {
  it("sends an omitted branch as absent, which is what asks for the current one", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    // The default briefing has to have loaded before submit, or its untouched
    // "" would go out as a customized empty one instead of being omitted.
    await waitFor(() => {
      expect((screen.getByLabelText("Landing briefing") as HTMLTextAreaElement).value).toBe(
        LANDING_PROMPT_DEFAULTS.direct,
      )
    })
    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/repositories" })
    expect(lastWrite()?.body).toEqual({
      path: "/home/me/dev/new",
      base_branch: null,
      description: null,
      merge_strategy: "direct",
    })
  })

  it("registers a repository whose tasks are published for a human to merge", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByLabelText("Merge strategy"))
    // The option is named the way the repositories table names the same stored
    // value; what it does is the line under it.
    await user.click(await screen.findByRole("option", { name: /^Pull request/ }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toMatchObject({ merge_strategy: "pull_request" })
  })

  it("refuses a relative path itself, without asking the daemon", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "dev/relative")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    expect(await screen.findByText("The path must be absolute.")).toBeDefined()
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })

  it("puts a bad path back on the path field, in the daemon's own words", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 400,
      code: "bad_request",
      message: "/home/me/dev/new is not a git work tree",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    // On the field, not in the banner above the buttons.
    const message = await screen.findByText(/is not a git work tree/)
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Path")
  })

  it("puts an unknown branch on the branch field, which is the one to fix", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 400,
      code: "bad_request",
      message: "branch nope does not exist in /home/me/dev/new",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.type(screen.getByLabelText("Base branch"), "nope")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    const message = await screen.findByText(/branch nope does not exist/)
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Base branch")
  })

  it("shows the pair already being registered above the buttons, where no field is at fault", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 409,
      code: "conflict",
      message: "/home/me/dev/new on main is already registered",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("already registered")
  })

  it("puts a placeholder refusal on the landing briefing field, not the branch its message also names", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 400,
      code: "bad_request",
      message:
        "the landing template has no value for placeholder {silly}; the ones it can use are {task_title}, {branch}, {base_branch}, {repo_path}",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    const message = await screen.findByText(/has no value for placeholder/)
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Landing briefing")
  })
})

/**
 * The briefing field: prefilled from `GET /v1/merge-strategies`, following an
 * untouched strategy pick and holding still for a customized one, and the
 * reset button that puts it back.
 */
describe("the landing briefing", () => {
  it("prefills a new repository with the selected strategy's default, and starts the reset button disabled", async () => {
    renderDialog(null)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    await waitFor(() => {
      expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct)
    })
    expect(screen.getByRole("button", { name: "Reset to default" }).hasAttribute("disabled")).toBe(
      true,
    )
  })

  it("prefills an existing repository with its own stored text", async () => {
    renderDialog(REPOSITORY)

    expect((screen.getByLabelText("Landing briefing") as HTMLTextAreaElement).value).toBe(
      REPOSITORY.landing_prompt,
    )
  })

  it("swaps in the new strategy's default while the briefing is still untouched", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    await waitFor(() => expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct))

    await user.click(screen.getByLabelText("Merge strategy"))
    await user.click(await screen.findByRole("option", { name: /^Pull request/ }))

    await waitFor(() => {
      expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.pull_request)
    })
  })

  it("seeds the strategy picked before the strategies query answered, not the form's opening default", async () => {
    const user = userEvent.setup()
    let answer = () => {}
    const held = new Promise<void>((resolve) => {
      answer = resolve
    })
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (pathname === "/v1/merge-strategies") {
        await held
        return jsonResponse(MERGE_STRATEGIES)
      }
      return jsonResponse(REPOSITORY)
    })
    renderDialog(null)

    // Picked while the query is still in flight — nothing to seed from yet.
    await user.click(screen.getByLabelText("Merge strategy"))
    await user.click(await screen.findByRole("option", { name: /^Pull request/ }))
    const textarea = screen.getByLabelText("Landing briefing") as HTMLTextAreaElement
    expect(textarea.value).toBe("")

    answer()

    await waitFor(() => {
      expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.pull_request)
    })
  })

  it("keeps words typed before the strategies query answers, instead of overwriting them with the default", async () => {
    const user = userEvent.setup()
    let answer = () => {}
    const held = new Promise<void>((resolve) => {
      answer = resolve
    })
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (pathname === "/v1/merge-strategies") {
        await held
        return jsonResponse(MERGE_STRATEGIES)
      }
      return jsonResponse(REPOSITORY)
    })
    renderDialog(null)

    // Typed while the query is still in flight — nothing seeded yet.
    const textarea = screen.getByLabelText("Landing briefing") as HTMLTextAreaElement
    await user.type(textarea, "Always squash, never rebase.")

    answer()
    await waitFor(() => {
      const resetButton = screen.getByRole("button", { name: "Reset to default" })
      expect(resetButton.hasAttribute("disabled")).toBe(false)
    })

    expect(textarea.value).toBe("Always squash, never rebase.")
  })

  it("keeps a customized briefing when the strategy changes", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    await waitFor(() => expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct))
    await user.clear(textarea)
    await user.type(textarea, "Always squash, never rebase.")

    await user.click(screen.getByLabelText("Merge strategy"))
    await user.click(await screen.findByRole("option", { name: /^Pull request/ }))

    expect(textarea.value).toBe("Always squash, never rebase.")
  })

  it("resets the briefing to the selected strategy's default, and disables itself again once it matches", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    await waitFor(() => expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct))
    await user.clear(textarea)
    await user.type(textarea, "Always squash, never rebase.")
    const resetButton = screen.getByRole("button", { name: "Reset to default" })
    expect(resetButton.hasAttribute("disabled")).toBe(false)

    await user.click(resetButton)

    expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct)
    expect(resetButton.hasAttribute("disabled")).toBe(true)
  })

  it("omits the briefing on create while it is still the strategy's default", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await waitFor(() => {
      expect((screen.getByLabelText("Landing briefing") as HTMLTextAreaElement).value).toBe(
        LANDING_PROMPT_DEFAULTS.direct,
      )
    })
    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).not.toHaveProperty("landing_prompt")
  })

  it("sends the customized text on create", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    await waitFor(() => expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct))
    await user.clear(textarea)
    await user.type(textarea, "Always squash, never rebase.")

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toMatchObject({ landing_prompt: "Always squash, never rebase." })
  })

  it("sends an empty string on save once a customized briefing is reset to the default", async () => {
    const user = userEvent.setup()
    const customized: RepositoryDto = aRepository({
      id: "01JREPO00000000000000CUS",
      landing_prompt: "Always squash, never rebase.",
      landing_prompt_is_default: false,
    })
    renderDialog(customized)

    const textarea = (await screen.findByLabelText("Landing briefing")) as HTMLTextAreaElement
    expect(textarea.value).toBe("Always squash, never rebase.")
    await user.click(screen.getByRole("button", { name: "Reset to default" }))
    expect(textarea.value).toBe(LANDING_PROMPT_DEFAULTS.direct)

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toMatchObject({ landing_prompt: "" })
  })
})

describe("editing a repository", () => {
  it("starts from what is stored, and sends the branch back with the rest", async () => {
    const user = userEvent.setup()
    renderDialog(REPOSITORY)

    expect((screen.getByLabelText("Path") as HTMLInputElement).value).toBe(REPOSITORY.path)
    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      REPOSITORY.description,
    )

    await user.type(screen.getByLabelText("Description"), " Now with repositories.")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({
      method: "PUT",
      path: `/v1/repositories/${REPOSITORY.id}`,
    })
    expect(lastWrite()?.body).toEqual({
      path: REPOSITORY.path,
      base_branch: "main",
      merge_strategy: "direct",
      description: "The orchestrator itself. Now with repositories.",
      // Untouched, and REPOSITORY already runs on its strategy's own default,
      // so this is the daemon's spelling of "no override" rather than a copy
      // of the text.
      landing_prompt: "",
    })
  })

  it("clears a description with the empty string the daemon spells it as", async () => {
    const user = userEvent.setup()
    renderDialog(REPOSITORY)

    await user.clear(screen.getByLabelText("Description"))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.description).toBe("")
  })
})

/**
 * The form holds a path nobody enjoys typing twice, so an outside press is not
 * an instruction to delete it — but an untouched form is still a form the user
 * gets to walk away from without being asked anything.
 */
describe("dismissing the dialog", () => {
  it("closes an untouched form straight away", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(REPOSITORY, onOpenChange)

    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("asks before dropping what was typed, and keeps it when the answer is no", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect((screen.getByLabelText("Path") as HTMLInputElement).value).toBe("/home/me/dev/new")
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("closes a saved form on the spot, with no draft left to ask about", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false)
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/repositories" })
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("closes and drops the draft once the discard is confirmed", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await user.click(await screen.findByRole("button", { name: "Discard" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })
})

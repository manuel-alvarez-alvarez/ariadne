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

import type { RepositoryDto, WorkflowDto } from "@/api"
import { aForge, aRepository, aWorkflow } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { RepositoryFormDialog } from "./repository-form-dialog"

const REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
})

const WORKFLOWS: WorkflowDto[] = [
  aWorkflow(),
  aWorkflow({ name: "develop-review-pr", document: "workflow develop-review-pr" }),
]

interface Recorded {
  method: string
  path: string
  body: {
    path?: string
    base_branch?: string | null
    description?: string | null
    permission_mode?: string
    default_landing?: string
    default_workflow?: string | null
    forge?: Record<string, unknown>
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

    if (pathname === "/v1/workflows") return jsonResponse(WORKFLOWS)
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
      permission_mode: "auto",
      default_workflow: null,
    })
  })

  it("sends the permission mode picked for it", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("combobox", { name: "Permission requests" }))
    await user.click(await screen.findByRole("option", { name: /^Learn/ }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.permission_mode).toBe("learn")
  })

  it("sends the default workflow picked for it", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("combobox", { name: "Default workflow" }))
    await user.click(await screen.findByRole("option", { name: "develop-review-pr" }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.default_workflow).toBe("develop-review-pr")
    expect(lastWrite()?.body).not.toHaveProperty("default_landing")
  })

  it("offers AI among the permission modes, and sends it as ai", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("combobox", { name: "Permission requests" }))
    await user.click(await screen.findByRole("option", { name: /^AI/ }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.permission_mode).toBe("ai")
  })

  it("puts an ai_disabled refusal on the permission mode field, pointing at the Permissions screen", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    stubDaemon({
      status: 409,
      code: "ai_disabled",
      message: "the `ai` permission mode needs the AI permission model turned on first",
    })
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("combobox", { name: "Permission requests" }))
    await user.click(await screen.findByRole("option", { name: /^AI/ }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    // On the field, not the daemon's own CLI-flavoured words, and the dialog
    // is left open to fix it.
    const message = await screen.findByText(
      "Enable the AI permission model on the Permissions screen first",
    )
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Permission requests")
    expect(onOpenChange).not.toHaveBeenCalled()
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
})

/**
 * A repository is a checkout, a base branch, a permission mode and a default
 * workflow — not the landing, merge strategy or landing briefing fields that
 * used to sit here, nor anything about how any one task or goal actually ends.
 */
describe("what a repository is", () => {
  it("takes a path, a base branch, a description, a permission mode and a default workflow", async () => {
    renderDialog(null)

    expect(await screen.findByLabelText("Path")).toBeDefined()
    expect(screen.getByLabelText("Base branch")).toBeDefined()
    expect(screen.getByLabelText("Description")).toBeDefined()
    expect(screen.getByRole("combobox", { name: "Permission requests" })).toBeDefined()
    expect(screen.getByRole("combobox", { name: "Default workflow" })).toBeDefined()

    expect(screen.queryByRole("combobox", { name: "Default landing" })).toBeNull()
    expect(screen.queryByLabelText("Merge strategy")).toBeNull()
    expect(screen.queryByLabelText("Landing briefing")).toBeNull()
    expect(screen.queryByRole("button", { name: "Reset to default" })).toBeNull()
  })

  it("asks the daemon for nothing but the repositories and the workflows", async () => {
    renderDialog(null)
    await screen.findByLabelText("Path")

    expect(requests.every((one) => !one.path.includes("merge-strategies"))).toBe(true)
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
      description: "The orchestrator itself. Now with repositories.",
      permission_mode: "auto",
      default_workflow: "",
    })
  })

  it("starts from the stored default workflow, and sends a new one", async () => {
    const user = userEvent.setup()
    renderDialog({ ...REPOSITORY, default_workflow: "develop-review-merge" })

    const picker = screen.getByRole("combobox", { name: "Default workflow" })
    expect(picker.textContent).toContain("develop-review-merge")

    await user.click(picker)
    await user.click(await screen.findByRole("option", { name: "develop-review-pr" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.default_workflow).toBe("develop-review-pr")
  })

  it("clears the default workflow with the empty string the daemon spells it as", async () => {
    const user = userEvent.setup()
    renderDialog({ ...REPOSITORY, default_workflow: "develop-review-merge" })

    await user.click(screen.getByRole("combobox", { name: "Default workflow" }))
    await user.click(await screen.findByRole("option", { name: "No workflow" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.default_workflow).toBe("")
  })

  it("starts from the stored permission mode, and sends a new one", async () => {
    const user = userEvent.setup()
    renderDialog({ ...REPOSITORY, permission_mode: "learn" })

    const picker = screen.getByRole("combobox", { name: "Permission requests" })
    expect(picker.textContent).toContain("Learn")

    await user.click(picker)
    await user.click(await screen.findByRole("option", { name: /^Ask/ }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.permission_mode).toBe("ask")
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

/**
 * The integration with the forge the daemon detected on the remote (spec
 * 025): one switch, shown only where there is a forge, and sent back only
 * where the user moved it.
 */
describe("the forge integration", () => {
  const ON_GITHUB: RepositoryDto = { ...REPOSITORY, forge: aForge() }

  it("shows the detected remote, and enables the forge with its one switch", async () => {
    const user = userEvent.setup()
    renderDialog(ON_GITHUB)

    expect(screen.getByTestId("forge-remote").textContent).toContain(
      "github.com/acme/widgets, via gh",
    )
    // The webhook is the table's to show, and the role pins are the CLI's.
    expect(screen.queryByText(/Webhook/)).toBeNull()
    expect(screen.queryByText(/runs on/)).toBeNull()
    await user.click(screen.getByRole("switch", { name: "GitHub integration" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.forge).toEqual({ enabled: true })
  })

  it("sends nothing of the forge where the switch did not move", async () => {
    const user = userEvent.setup()
    renderDialog({ ...REPOSITORY, forge: aForge({ enabled: true, login: "octocat" }) })

    expect(screen.getByTestId("forge-remote").textContent).toContain("signed in as octocat")
    await user.type(screen.getByLabelText("Description"), " Now on GitHub.")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).not.toHaveProperty("forge")
  })

  it("puts a refusal to enable on the switch, in the CLI's own words", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    stubDaemon({
      status: 409,
      code: "forge_unauthenticated",
      message: "`gh auth status --hostname github.com`: You are not logged into any GitHub hosts.",
    })
    renderDialog(ON_GITHUB, onOpenChange)

    await user.click(screen.getByRole("switch", { name: "GitHub integration" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    const message = await screen.findByText(/You are not logged into any GitHub hosts/)
    expect(message.closest("[data-slot=field-error]")).not.toBeNull()
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("offers no switch where no GitHub or GitLab remote was detected", () => {
    renderDialog(REPOSITORY)

    expect(screen.queryByRole("switch")).toBeNull()
  })

  it("shows no forge while registering: nothing is detected before the daemon opens the checkout", () => {
    renderDialog(null)

    expect(screen.queryByRole("switch")).toBeNull()
  })
})

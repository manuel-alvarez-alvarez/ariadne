// @vitest-environment jsdom

/**
 * The workflows screen as a combobox beside an editor, against a stubbed
 * daemon. The catalog sits behind the trigger rather than down the side, so
 * what is checked here is the picker's own behaviour: it groups by where a
 * workflow came from, it is filtered by name and by its step titles, and a
 * pick lands in the URL as `?workflow=`.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createBrowserRouter, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { WorkflowDto } from "@/api"
import { paths, WORKFLOW_PARAM } from "@/routes/paths"
import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen, startAt } from "@/test/harness"
import { WorkflowsPage } from "./workflows-page"

const SHIPPED: WorkflowDto = aWorkflow()

/**
 * A step title deliberately shares no word with the workflow's own name, so
 * a search that only the title answers to proves the detail line is read,
 * not only the name — and the two steps prove the join reads in full.
 */
const MINE: WorkflowDto = aWorkflow({
  name: "release",
  builtin: false,
  steps: [
    {
      id: "ship",
      title: "Ship it",
      description: "Release the work.",
      skills: [],
      rank: null,
      gate: null,
    },
    {
      id: "notify",
      title: "Notify team",
      description: "Tell everyone it shipped.",
      skills: [],
      rank: null,
      gate: null,
    },
  ],
})

let requests: { method: string; path: string }[] = []

function stubDaemon(initial: WorkflowDto[]) {
  const workflows = [...initial]
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    requests.push({ method: request.method, path: pathname })
    if (pathname === "/v1/workflows" && request.method === "GET") return jsonResponse(workflows)
    return new Response("not stubbed", { status: 404 })
  })
}

function renderPage(entry: string = paths.workflows()) {
  function Harness() {
    const navigate = useNavigate()
    const location = useLocation()
    return (
      <>
        {/* Stands in for a link elsewhere in the app, such as the command
            palette's own entry for a workflow (`entries.ts`): a mention,
            not a combobox pick. */}
        <button type="button" onClick={() => void navigate(paths.workflow(MINE.name))}>
          link to release
        </button>
        <button type="button" onClick={() => void navigate(-1)}>
          go back
        </button>
        <button type="button" onClick={() => void navigate(paths.agents())}>
          go to agents
        </button>
        <output data-testid="search">{location.search}</output>
      </>
    )
  }
  startAt(entry)
  const router = createBrowserRouter([
    {
      path: paths.workflows(),
      element: (
        <>
          <Harness />
          <WorkflowsPage />
        </>
      ),
    },
    { path: paths.agents(), element: <p>Agents screen</p> },
  ])
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

function selectedInUrl(): string | null {
  return new URLSearchParams(screen.getByTestId("search").textContent ?? "").get(WORKFLOW_PARAM)
}

/** The combobox trigger, which also reads back as the whole choice. */
function trigger(): HTMLElement {
  return screen.getByRole("button", { name: "Workflow" })
}

async function openCombobox(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Workflow" }))
  await screen.findByRole("listbox", { name: "Workflow" })
}

/** Opens the trigger by focusing it and pressing Enter, never a click. */
async function openComboboxWithKeyboard(user: ReturnType<typeof userEvent.setup>) {
  ;(await screen.findByRole("button", { name: "Workflow" })).focus()
  await user.keyboard("{Enter}")
  await screen.findByRole("listbox", { name: "Workflow" })
}

/** The group of one title, found by its own accessible name. */
function group(title: string): HTMLElement {
  return screen.getByRole("group", { name: title })
}

async function editorFor(name: string): Promise<HTMLElement> {
  return await screen.findByRole("heading", { level: 2, name })
}

beforeEach(() => {
  requests = []
  stubDaemon([SHIPPED, MINE])
})

describe("the combobox", () => {
  it("shows no left list panel", async () => {
    renderPage()
    await screen.findByRole("button", { name: "Workflow" })

    expect(screen.queryByRole("navigation")).toBeNull()
  })

  it("groups shipped workflows apart from user workflows and shows their columns", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    expect(
      within(group("Shipped with Ariadne")).getByRole("option", { name: /develop-review-merge/ })
        .textContent,
    ).toContain("Develop")
    // The full join, not a substring: both steps, in order, joined by " · ".
    expect(within(group("Yours")).getByRole("option", { name: /^release/ }).textContent).toContain(
      "Ship it · Notify team",
    )
  })

  it("narrows by a step's title even where the name shares none of its words", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    const filter = screen.getByRole("combobox", { name: "Workflow" })
    // "Ship" names no word of "release" or "develop-review-merge": only the
    // detail line can be what answers to it.
    await user.type(filter, "Ship")

    expect(screen.getByRole("option", { name: /^release/ })).toBeDefined()
    expect(screen.queryByRole("option", { name: /^develop-review-merge/ })).toBeNull()
    expect(screen.queryByRole("group", { name: "Shipped with Ariadne" })).toBeNull()
  })

  it("narrows by name, and says so when nothing is left", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    const filter = screen.getByRole("combobox", { name: "Workflow" })
    await user.type(filter, "develop-review-merge")

    expect(screen.getByRole("option", { name: /^develop-review-merge/ })).toBeDefined()
    expect(screen.queryByRole("option", { name: /^release/ })).toBeNull()

    await user.clear(filter)
    await user.type(filter, "nothing matches this")
    expect(screen.getByText(/No workflow matches/)).toBeDefined()
  })

  it("reads as a placeholder until a workflow is picked", async () => {
    renderPage()

    expect((await screen.findByRole("button", { name: "Workflow" })).textContent).toContain(
      "Select a workflow",
    )
  })
})

describe("the selection", () => {
  it("is nothing until a workflow is picked, and says so", async () => {
    renderPage()
    await screen.findByRole("button", { name: "Workflow" })

    expect(screen.getByText("Select a workflow, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("puts a picked workflow in the URL and opens its editor below the combobox", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    await user.click(screen.getByRole("option", { name: /^develop-review-merge/ }))

    expect(await editorFor("develop-review-merge")).toBeDefined()
    expect(selectedInUrl()).toBe("develop-review-merge")
    expect(trigger().textContent).toContain("develop-review-merge")
  })

  it("picks with the keyboard: open, filter, enter", async () => {
    const user = userEvent.setup()
    renderPage()
    await openComboboxWithKeyboard(user)

    await user.keyboard("release")
    await user.keyboard("{Enter}")

    expect(await editorFor("release")).toBeDefined()
    expect(selectedInUrl()).toBe("release")
  })

  it("picks with the keyboard: open, arrow, enter", async () => {
    const user = userEvent.setup()
    renderPage()
    await openComboboxWithKeyboard(user)

    // No filter typed, so the two groups keep the order they rendered in:
    // one arrow down moves off the lone shipped row onto the user's own.
    await user.keyboard("{ArrowDown}")
    await user.keyboard("{Enter}")

    expect(await editorFor("release")).toBeDefined()
    expect(selectedInUrl()).toBe("release")
  })

  it("closes on Escape, leaving the selection as it was", async () => {
    const user = userEvent.setup()
    renderPage()
    await openComboboxWithKeyboard(user)

    await user.keyboard("{Escape}")

    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Workflow" })).toBeNull()
    })
    expect(selectedInUrl()).toBeNull()
  })

  it("opens on the workflow a link named, so a mention of one leads here", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("button", { name: "Workflow" })

    await user.click(screen.getByRole("button", { name: "link to release" }))

    expect(await editorFor("release")).toBeDefined()
    expect(trigger().textContent).toContain("release")
  })

  it("comes back up on the workflow the URL names, on a reload", async () => {
    renderPage(paths.workflow(MINE.name))

    expect(await editorFor("release")).toBeDefined()
  })

  it("says so when the URL names a workflow the daemon does not have", async () => {
    const user = userEvent.setup()
    renderPage(paths.workflow("gone-since"))
    await screen.findByRole("button", { name: "Workflow" })

    expect(screen.getByText("No workflow by that name.")).toBeDefined()
    expect(screen.queryByRole("heading", { level: 2 })).toBeNull()

    await user.click(screen.getByRole("button", { name: "Clear selection" }))
    expect(await screen.findByText("Select a workflow, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })
})

describe("leaving a dirty workflow", () => {
  it("asks before picking a different workflow from the combobox", async () => {
    const user = userEvent.setup()
    renderPage(paths.workflow(SHIPPED.name))
    await editorFor(SHIPPED.name)

    const textarea = screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement
    await user.type(textarea, "x")
    await openCombobox(user)
    await user.click(screen.getByRole("option", { name: /^release/ }))

    expect(await screen.findByRole("dialog", { name: "Discard changes?" })).toBeDefined()
    // Blocked: the selection has not moved, and the box still has what was
    // typed, once the dialog in front of it is answered.
    expect(selectedInUrl()).toBe(SHIPPED.name)
    expect(textarea.value).toBe(`${SHIPPED.document}x`)
  })

  it("keeps the draft and stays, on Keep editing", async () => {
    const user = userEvent.setup()
    renderPage(paths.workflow(SHIPPED.name))
    await editorFor(SHIPPED.name)

    const textarea = screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement
    await user.type(textarea, "x")
    await openCombobox(user)
    await user.click(screen.getByRole("option", { name: /^release/ }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
    expect(await editorFor(SHIPPED.name)).toBeDefined()
    expect(selectedInUrl()).toBe(SHIPPED.name)
    expect(textarea.value).toBe(`${SHIPPED.document}x`)
  })

  it("discards the draft and switches, on Discard", async () => {
    const user = userEvent.setup()
    renderPage(paths.workflow(SHIPPED.name))
    await editorFor(SHIPPED.name)

    await user.type(screen.getByRole("textbox", { name: "Document" }), "x")
    await openCombobox(user)
    await user.click(screen.getByRole("option", { name: /^release/ }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await editorFor("release")).toBeDefined()
    expect(selectedInUrl()).toBe("release")
  })

  it("asks before a route to another screen leaves a dirty workflow, too", async () => {
    const user = userEvent.setup()
    renderPage(paths.workflow(SHIPPED.name))
    await editorFor(SHIPPED.name)

    await user.type(screen.getByRole("textbox", { name: "Document" }), "x")
    await user.click(screen.getByRole("button", { name: "go to agents" }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await screen.findByText("Agents screen")).toBeDefined()
  })
})

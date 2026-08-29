// @vitest-environment jsdom

/**
 * The palette's own half: the group that answers "what is waiting for me", and
 * the actions that only exist for what the screen underneath has open.
 *
 * `entries.test.ts` pins what a row says and where it goes; none of that says
 * whether the row is *offered*, which is what the conditions here decide — a
 * palette that lists "New task" over a screen with no goal open, or hides the
 * retry the moment it is the only thing worth clicking, is wrong in a way no
 * pure test can see.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { useStreamStore } from "@/stores/stream"
import { aGoal, aProfile, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CommandPalette } from "./command-palette"

const GOAL: GoalDto = aGoal()
const TASK: TaskDto = aTask({ goal_id: GOAL.id })
/** The one thing on the board that is stuck, and so the one attention row. */
const STUCK: TaskDto = aTask({
  id: "01JTASK0000000000000STUCK1",
  title: "Land the migration",
  goal_id: GOAL.id,
  status: "failed",
})
const SESSION: SessionDto = aSession({ goal_id: GOAL.id, task_id: TASK.id })

function stubDaemon(tasks: TaskDto[] = [TASK, STUCK]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const { pathname } = new URL(typeof input === "string" ? input : (input as Request).url)
    const body =
      pathname === "/v1/goals"
        ? [GOAL]
        : pathname === "/v1/tasks"
          ? tasks
          : pathname === "/v1/sessions"
            ? [SESSION]
            : pathname === "/v1/profiles"
              ? [aProfile()]
              : []
    return Promise.resolve(jsonResponse(body))
  })
}

const handlers = {
  onOpenChange: vi.fn(),
  onOpenSettings: vi.fn(),
  onNewGoal: vi.fn(),
  onOpenLogs: vi.fn(),
  onOpenShortcuts: vi.fn(),
  onToggleSidebar: vi.fn(),
}

function renderPalette(route = "/goals") {
  return renderScreen(<CommandPalette open {...handlers} />, { route }).location
}

/** The rows of one group, in the order they are listed. */
function group(heading: string): HTMLElement {
  const title = screen.getByText(heading)
  const found = title.closest("[cmdk-group]")
  if (!(found instanceof HTMLElement)) throw new Error(`no ${heading} group`)
  return found
}

beforeEach(() => {
  stubDaemon()
  useStreamStore.setState({ status: "open" })
  for (const handler of Object.values(handlers)) handler.mockClear()
})

it("leads with what is stuck, before anything has been typed", async () => {
  renderPalette()

  const attention = await screen.findByText("Needs attention")
  const rows = within(group("Needs attention")).getAllByRole("option")
  expect(rows).toHaveLength(1)
  expect(rows[0]?.textContent).toContain("Land the migration")
  expect(rows[0]?.textContent).toContain("Failed")

  // Above the actions: it is the answer to the question asked first.
  expect(attention.compareDocumentPosition(screen.getByText("Actions"))).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  )
})

it("says nothing about attention when nothing is stuck", async () => {
  stubDaemon([TASK])
  renderPalette()

  await screen.findByText("Actions")
  expect(screen.queryByText("Needs attention")).toBeNull()
})

it("drops the attention group as soon as the entities can answer instead", async () => {
  const user = userEvent.setup()
  renderPalette()
  await screen.findByText("Needs attention")

  await user.type(screen.getByRole("combobox"), "migration")

  expect(screen.queryByText("Needs attention")).toBeNull()
  expect(await screen.findByText("Tasks")).toBeTruthy()
})

it("offers a new task in the goal whose panel is open", async () => {
  renderPalette(`/goals?goal=${GOAL.id}`)

  expect(await screen.findByText("New task")).toBeTruthy()
})

it("has no task to offer over a screen with no goal open", async () => {
  renderPalette()

  await screen.findByText("Actions")
  expect(screen.queryByText("New task")).toBeNull()
})

it("offers the attach command for the session the screen has open", async () => {
  renderPalette(`/sessions?session=${SESSION.id}`)

  expect(await screen.findByText("Copy attach command")).toBeTruthy()
})

it("has nothing to attach to where neither a task nor a session is open", async () => {
  renderPalette()

  await screen.findByText("Actions")
  expect(screen.queryByText("Copy attach command")).toBeNull()
})

it("keeps the retry out of the way while the daemon is answering", async () => {
  renderPalette()

  await screen.findByText("Actions")
  expect(screen.queryByText("Retry connection")).toBeNull()
})

it("offers the banner's retry while the daemon is unreachable", async () => {
  useStreamStore.setState({ status: "reconnecting" })
  renderPalette()

  expect(await screen.findByText("Retry connection")).toBeTruthy()
})

it("opens the daemon logs and the cheat sheet, which the shell owns", async () => {
  const user = userEvent.setup()
  renderPalette()

  await user.click(await screen.findByText("Open daemon logs"))
  expect(handlers.onOpenLogs).toHaveBeenCalled()

  await user.click(screen.getByText("Keyboard shortcuts"))
  expect(handlers.onOpenShortcuts).toHaveBeenCalled()
})

it("leaves the popup's own height to what is inside it", async () => {
  renderPalette()
  await screen.findByText("Actions")

  // The popup is `height: fit-content` and out of flow, so a percentage height
  // on the box it hugs is a cycle — and WebKit resolves that one to zero, which
  // is how the whole palette came to be clipped away in the Tauri window while
  // its scrim showed. jsdom lays nothing out, so the class is what can be
  // pinned here; the geometry was measured in the WebView. See
  // `@/components/ui/command`.
  const command = document.querySelector('[data-slot="command"]')
  expect(command?.className).not.toMatch(/(^|\s)(h-full|size-full)(\s|$)/)
})

it("asks the daemon nothing until it is opened", async () => {
  renderScreen(<CommandPalette open={false} {...handlers} />, { route: "/goals" })

  // The attention group reads three lists with no `enabled` of their own; what
  // keeps them from being three requests on every screen is that the dialog
  // unmounts its whole subtree while it is closed.
  await Promise.resolve()
  expect(daemonFetch).not.toHaveBeenCalled()
})

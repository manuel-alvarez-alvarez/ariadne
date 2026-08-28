// @vitest-environment jsdom

/**
 * The sessions screen: the one place the list is mounted with no goal and no
 * task around it.
 *
 * What is worth pinning is exactly what that unscoped mounting buys, and none
 * of it shows without rendering the screen: the Context column saying which
 * work each row belongs to (a task's title, a planner session's goal), a pick
 * turning into `?session=` over the screen rather than a navigation away from
 * it, and the two filters — one of which the daemon answers (`?status=failed`)
 * and one of which it cannot (`live` is three statuses, so it is narrowed
 * here) — including what a second visit to the screen opens on, which is the
 * one thing about them that no single render shows.
 *
 * The tokens column is here because it is the row's own figure and the table
 * is where sessions are compared: a compact `in/out`, and a `0/0` — never a
 * blank — on a session whose agent has reported nothing.
 */

import { cleanup, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { formatAbsolute } from "@/lib/format"
import { useSettingsStore } from "@/stores/settings"
import { aGoal, aProfile, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SessionsPage } from "./sessions-page"

const GOAL: GoalDto = aGoal()

const TASK: TaskDto = aTask({
  goal_id: GOAL.id,
})

const PROFILE: ProfileDto = aProfile()

/** An engineer at work, and the planner that has no task of its own. */
const ENGINEER = aSession({
  id: "01JSESS0000000000000000ENG",
  usage: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
})
const PLANNER = aSession({
  id: "01JSESS0000000000000000PLA",
  task_id: null,
  role: "planner",
  status: "failed",
})

/** The daemon, answering the four lists the screen reads. */
function stubDaemon(sessions: SessionDto[] = [ENGINEER, PLANNER]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    const status = url.searchParams.get("status")
    const body =
      url.pathname === "/v1/sessions"
        ? sessions.filter((one) => !status || one.status === status)
        : url.pathname === "/v1/goals"
          ? [GOAL]
          : url.pathname === "/v1/tasks"
            ? [TASK]
            : url.pathname === "/v1/profiles"
              ? [PROFILE]
              : []
    return Promise.resolve(jsonResponse(body))
  })
}

/** What `GET /v1/sessions` was asked to filter by, oldest request first. */
function sessionRequests(): (string | null)[] {
  return daemonFetch.mock.calls
    .map(([input]) => new URL(typeof input === "string" ? input : (input as Request).url))
    .filter((url) => url.pathname === "/v1/sessions")
    .map((url) => url.searchParams.get("status"))
}

function renderPage(entry = "/sessions") {
  return renderScreen(<SessionsPage />, { route: entry }).location
}

/** The row a session is on, found by the role button that opens it. */
async function row(name: string): Promise<HTMLElement> {
  const button = await screen.findByRole("button", { name })
  const found = button.closest("tr")
  if (!found) throw new Error(`no row around ${name}`)
  return found
}

/** A row's tokens figure, which is also the element the hint hangs off. */
function tokens(row: HTMLElement): HTMLElement {
  const cell = within(row).getAllByRole("cell").at(-2)
  const figure = cell?.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")
  if (!figure) throw new Error("no tokens figure in the row")
  return figure
}

beforeEach(() => {
  stubDaemon()
  localStorage.clear()
  useSettingsStore.setState({ sessionStatusFilter: "", sessionRoleFilter: "" })
})

it("says which work each session belongs to, and links to it", async () => {
  renderPage()

  // A session on a task is named by the task; the planner, which has none, by
  // its goal — and the goal panel only opens on the board.
  const engineer = within(await row("Open Engineer session")).getByRole("link", {
    name: TASK.title,
  })
  expect(engineer.getAttribute("href")).toBe(`/sessions?task=${TASK.id}`)

  const planner = within(await row("Open Planner session")).getByRole("link", { name: GOAL.title })
  expect(planner.getAttribute("href")).toBe(`/goals?goal=${GOAL.id}`)
})

it("opens the picked session as a panel over the screen", async () => {
  const user = userEvent.setup()
  const seen = renderPage()

  await user.click(await screen.findByRole("button", { name: "Open Engineer session" }))

  await waitFor(() => expect(seen.url).toBe(`/sessions?session=${ENGINEER.id}`))
})

it("asks the daemon for one status and keeps it in the URL", async () => {
  const user = userEvent.setup()
  const seen = renderPage()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))
  expect(sessionRequests()).toEqual([null])

  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Failed" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed"))
  await waitFor(() => expect(sessionRequests()).toContain("failed"))
  expect(await screen.findByRole("button", { name: "Open Planner session" })).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Open Engineer session" })).toBeNull()
})

it("narrows live sessions itself, without asking for a status", async () => {
  const user = userEvent.setup()
  const seen = renderPage()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))

  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Live" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=live"))
  expect(await screen.findByRole("button", { name: "Open Engineer session" })).toBeTruthy()
  // The failed planner is gone, and the daemon was never asked for a status:
  // "live" is three of them, so the same unfiltered response was reused.
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Open Planner session" })).toBeNull(),
  )
  expect(sessionRequests().every((status) => status === null)).toBe(true)
})

it("filters by role, and blames the filters when nothing is left", async () => {
  const user = userEvent.setup()
  const seen = renderPage("/sessions?status=failed")
  await waitFor(() => expect(sessionRequests()).toContain("failed"))

  await user.click(screen.getByRole("button", { name: "Filter by role" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Engineer" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed&role=engineer"))
  expect(await screen.findByText("No sessions match these filters")).toBeTruthy()
})

it("calls an empty list empty when nothing is filtered", async () => {
  stubDaemon([])
  renderPage()

  expect(await screen.findByText("No sessions yet")).toBeTruthy()
})

/**
 * The table is the one surface that keeps the compact age as its text — the
 * heading says what it is the age of, and a column of "N minutes ago" is a
 * column of repeated words. Everything the column has no room for is the hint
 * behind it, and the hint opens on focus: both of these were a `title=`, which
 * a keyboard never reaches.
 */
it("puts the stamps behind the table's columns in reach of a keyboard", async () => {
  const user = userEvent.setup()
  renderPage()
  const engineer = await row("Open Engineer session")

  // The Context cell's link is the trigger, so the pair it names costs no
  // focus stop of its own.
  within(engineer).getByRole("link", { name: TASK.title }).focus()
  expect(await screen.findByText(`Goal: ${GOAL.title}`)).not.toBeNull()

  // The age column is further along the row; Tab walks to it.
  for (let stop = 0; stop < 8; stop++) {
    await user.tab()
    if (screen.queryByText(`last activity ${formatAbsolute(ENGINEER.last_activity_at)}`)) {
      // The two columns the table dropped ride along in the same hint.
      expect(screen.getByText(`started ${formatAbsolute(ENGINEER.created_at)}`)).not.toBeNull()
      return
    }
  }
  throw new Error("the last-activity stamp is not reachable by keyboard")
})

/** Leaving the screen for another one, and coming back to the sidebar's `/sessions`. */
function leaveAndComeBack() {
  cleanup()
  daemonFetch.mockClear()
  return renderPage()
}

/** The trigger of one filter, which doubles as the summary of what is selected. */
function trigger(name: "Filter by status" | "Filter by role") {
  return screen.getByRole("button", { name })
}

/** Pick one value out of one filter's menu. */
async function pick(
  user: ReturnType<typeof userEvent.setup>,
  filter: "Filter by status" | "Filter by role",
  value: string,
) {
  await user.click(trigger(filter))
  await user.click(await screen.findByRole("menuitemradio", { name: value }))
}

it("comes back to the filters the screen was left with", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))

  await pick(user, "Filter by status", "Failed")
  await pick(user, "Filter by role", "Planner")
  await waitFor(() => expect(sessionRequests()).toContain("failed"))

  const seen = leaveAndComeBack()

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed&role=planner"))
  // The screen is narrowed again, not just the URL: the daemon is asked for
  // the status, and the role is applied here.
  await waitFor(() => expect(sessionRequests()).toContain("failed"))
  expect(trigger("Filter by status").textContent).toContain("Failed")
  expect(trigger("Filter by role").textContent).toContain("Planner")
  expect(await screen.findByRole("button", { name: "Open Planner session" })).toBeTruthy()
})

it("keeps the filters where a restart can find them", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))

  await pick(user, "Filter by status", "Live")
  await pick(user, "Filter by role", "Engineer")

  await waitFor(() =>
    expect(JSON.parse(localStorage.getItem("ariadne.settings") ?? "{}")).toMatchObject({
      state: { sessionStatusFilter: "live", sessionRoleFilter: "engineer" },
    }),
  )
})

it("shows what an explicit filter asks for, not what is remembered", async () => {
  useSettingsStore.setState({ sessionStatusFilter: "live", sessionRoleFilter: "engineer" })
  const seen = renderPage("/sessions?status=failed&role=planner")

  await waitFor(() => expect(sessionRequests()).toContain("failed"))
  expect(seen.url).toBe("/sessions?status=failed&role=planner")
  expect(trigger("Filter by status").textContent).toContain("Failed")
  expect(trigger("Filter by role").textContent).toContain("Planner")

  // ...and that is what the next visit opens on: the screen remembers what it
  // is showing, however it was asked to show it.
  const back = leaveAndComeBack()
  await waitFor(() => expect(back.url).toBe("/sessions?status=failed&role=planner"))
})

it("restores the one filter a deep link says nothing about", async () => {
  useSettingsStore.setState({ sessionStatusFilter: "failed", sessionRoleFilter: "planner" })
  const seen = renderPage("/sessions?status=live")

  await waitFor(() => expect(seen.url).toBe("/sessions?status=live&role=planner"))
})

it("drops a remembered value the daemon no longer defines", async () => {
  useSettingsStore.setState({ sessionStatusFilter: "nonsense", sessionRoleFilter: "nobody" })
  const seen = renderPage()

  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))
  expect(seen.url).toBe("/sessions")
  expect(sessionRequests().every((status) => status === null)).toBe(true)
  expect(trigger("Filter by status").textContent).toContain("All statuses")
  expect(trigger("Filter by role").textContent).toContain("All roles")
})

it("leaves a cleared filter cleared", async () => {
  const user = userEvent.setup()
  renderPage("/sessions?status=failed&role=planner")
  await waitFor(() => expect(sessionRequests()).toContain("failed"))

  await pick(user, "Filter by status", "All statuses")
  await pick(user, "Filter by role", "All roles")
  await waitFor(() => expect(sessionRequests()).toContain(null))

  const seen = leaveAndComeBack()

  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))
  expect(seen.url).toBe("/sessions")
  expect(sessionRequests()).not.toContain("failed")
  expect(trigger("Filter by status").textContent).toContain("All statuses")
  expect(trigger("Filter by role").textContent).toContain("All roles")
})

it("restores the filters under a panel the entry opened", async () => {
  useSettingsStore.setState({ sessionStatusFilter: "failed", sessionRoleFilter: "planner" })
  const seen = renderPage(`/sessions?session=${ENGINEER.id}`)

  await waitFor(() =>
    expect(seen.url).toBe(`/sessions?session=${ENGINEER.id}&status=failed&role=planner`),
  )
})

it("carries each session's tokens, and zero for one that has reported none", async () => {
  renderPage()

  expect(tokens(await row("Open Engineer session")).textContent).toBe(
    "1.2M in, 89% cached, 45k out",
  )
  // The planner has spent nothing yet, which is a figure of its own — both
  // halves of it: a blank cell would read as a column the daemon has no
  // answer for.
  expect(tokens(await row("Open Planner session")).textContent).toBe("0 in, 0% cached, 0 out")
})

it("puts the exact counts behind the tokens column in reach of a keyboard", async () => {
  renderPage()

  // Every digit of the counts is what the column has no room for; the hint
  // opens on focus, like the table's other two.
  tokens(await row("Open Engineer session")).focus()
  const label = await screen.findByText("Input")
  const popup = label.closest<HTMLElement>("[data-slot='tooltip-content']")
  if (!popup) throw new Error("no hint around the exact counts")

  const exact = within(popup)
  expect(exact.getByText("Input").nextElementSibling?.textContent).toBe("1,234,567")
  // The cached line is under Input and part of it, carrying the same share
  // the figure itself shows rather than a total of its own.
  const cached = exact.getByText("cached")
  expect(cached.nextElementSibling?.textContent).toBe("1,100,000")
  expect(cached.nextElementSibling?.nextElementSibling?.textContent).toBe("89%")
  expect(exact.getByText("Output").nextElementSibling?.textContent).toBe("45,300")
})

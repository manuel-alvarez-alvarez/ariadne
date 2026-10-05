// @vitest-environment jsdom

/**
 * Focus across the one side panel, driven the way the app drives it: by the
 * URL.
 *
 * `DetailPanels` is the piece that decides which panel is open, and that
 * decision is exactly what focus depends on — so the real thing is mounted
 * rather than a stand-in for it. The daemon never answers, which is enough:
 * every panel has a pending state, and none of them needs data to be a pane
 * with focus in it.
 *
 * A task replaces its goal rather than stacking on it: the case worth
 * watching is that the goal's sheet is gone outright, not merely hidden
 * behind the task's, once that navigation lands. Focus belongs to whichever
 * sheet is mounted now; see `hooks/use-focus-return.ts`.
 *
 * Where that focus *lands* is the second thing here: the task panel opens on
 * its breadcrumb, which is therefore the one control in the app a deep link
 * puts a focus ring on before anything is clicked. It wears the app's own
 * ring rather than the browser's outline.
 */

import { act, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { type NavigateFunction, useNavigate } from "react-router-dom"
import { expect, it } from "vitest"
import { qk } from "@/api"
import { GoalSwimlanes } from "@/features/goals/goal-swimlanes"
import { paths } from "@/routes/paths"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { DetailPanels } from "./detail-panels"

daemonFetch.mockImplementation(() => new Promise(() => {}))

// `globals` is off, so nothing unmounts a screen between tests but this.

/** The router's `navigate`, for URL changes no rendered control stands in for. */
let go: NavigateFunction | undefined
function Navigator() {
  go = useNavigate()
  return null
}

function mountPanels(at: string) {
  renderScreen(
    <>
      <Navigator />
      <DetailPanels />
    </>,
    { route: at },
  )
}

it("unmounts the goal outright once a task replaces it, and focuses the task", async () => {
  mountPanels(`${paths.goals()}?goal=g1&tab=sessions&session=s1`)
  const goalSheet = screen
    .getByRole("heading", { name: "Session s1" })
    .closest('[data-slot="docked-pane"]')
  if (!goalSheet) throw new Error("no sheet holding the goal's session")
  // The pane takes focus on mount.
  await waitFor(() => expect(goalSheet.contains(document.activeElement)).toBe(true))

  // The task card's own navigation: `taskPanelTo` adds `?task=` and drops
  // `?goal=` in the one step, since the task replaces it rather than stacking
  // on it.
  await act(async () => {
    go?.({ pathname: paths.goals(), search: "?task=t1" })
  })

  // Gone outright, not merely hidden behind the task's sheet.
  expect(screen.queryByRole("heading", { name: "Session s1" })).toBeNull()
  expect(document.querySelectorAll('[data-slot="docked-pane"]')).toHaveLength(1)

  const taskSheet = screen.getByText("Loading task").closest('[data-slot="docked-pane"]')
  if (!taskSheet) throw new Error("no sheet holding the task")
  await waitFor(() => expect(taskSheet.contains(document.activeElement)).toBe(true))
})

it("gives the task panel's breadcrumb the app's own focus ring, once the task names its goal", async () => {
  const task = aTask({ id: "t1", goal_id: "g1" })
  renderScreen(<DetailPanels />, {
    route: `${paths.goals()}?task=t1`,
    seed: (client) => client.setQueryData(qk.tasks.detail(task.id), task),
  })

  // The goal is still loading, so the button wears the word rather than the
  // title — it is the first focusable thing in the sheet either way.
  const nav = await screen.findByLabelText("Breadcrumb")
  const breadcrumb = nav.querySelector("button")
  if (!breadcrumb) throw new Error("no way back to the goal in the breadcrumb")
  await waitFor(() => expect(document.activeElement).toBe(breadcrumb))
  expect(breadcrumb.className).toContain("focus-visible:ring-3")
  expect(breadcrumb.className).toContain("outline-none")
})

it("replaces a goal's own session view with the task it names, and Escape empties the pane rather than returning to it", async () => {
  const goal = aGoal({ id: "g1" })
  const task = aTask({ id: "t1", goal_id: "g1", title: "Wire the dispatcher" })
  const session = aSession({ id: "s1", goal_id: "g1", task_id: "t1" })
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    if (url.pathname === "/v1/goals/g1") return Promise.resolve(jsonResponse(goal))
    if (url.pathname === "/v1/sessions/s1") return Promise.resolve(jsonResponse(session))
    if (url.pathname === "/v1/tasks/t1") return Promise.resolve(jsonResponse(task))
    return new Promise(() => {})
  })
  mountPanels(`${paths.goals()}?goal=g1&tab=sessions&session=s1`)
  const user = userEvent.setup()
  await user.click(await screen.findByRole("link", { name: task.title }))

  // One panel in the DOM: the task's. The goal's session view is gone, not
  // stacked under it.
  expect(document.querySelectorAll('[data-slot="docked-pane"]')).toHaveLength(1)
  expect(await screen.findByRole("heading", { name: task.title })).toBeDefined()

  await user.keyboard("{Escape}")
  // The board, with no panel — not back onto the goal's session it was
  // followed from.
  await waitFor(() =>
    expect(document.querySelectorAll('[data-slot="docked-pane"]')).toHaveLength(0),
  )
})

it("floats a goal over the board, and a click on its scrim closes the goal", async () => {
  const first = aGoal({ id: "g1", title: "First lane" })
  const second = aGoal({ id: "g2", title: "Second lane" })
  const { location } = renderScreen(
    <>
      <main>
        <GoalSwimlanes goals={[first, second]} />
      </main>
      <DetailPanels />
    </>,
    {
      route: "/goals?goal=g1",
      seed: (client) => {
        client.setQueryData(qk.goals.detail(first.id), first)
        client.setQueryData(qk.tasks.list({}), [])
      },
    },
  )
  const pane = screen.getByRole("region", { name: first.title })
  // A floating pane, not a modal: the board stays in the tree behind it.
  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.getByRole("main").contains(pane)).toBe(false)
  const scrim = document.querySelector('[data-slot="docked-pane-scrim"]')
  if (!scrim) throw new Error("no scrim over the board")
  await userEvent.setup().click(scrim)
  expect(location.url).toBe("/goals")
  expect(document.querySelectorAll('[data-slot="docked-pane"]')).toHaveLength(0)
})

it("swaps the open goal for another goal the URL names", async () => {
  const first = aGoal({ id: "g1", title: "First goal" })
  const second = aGoal({ id: "g2", title: "Second goal" })
  renderScreen(
    <>
      <Navigator />
      <DetailPanels />
    </>,
    {
      route: `${paths.goals()}?goal=g1`,
      seed: (client) => {
        client.setQueryData(qk.goals.detail(first.id), first)
        client.setQueryData(qk.goals.detail(second.id), second)
        client.setQueryData(qk.tasks.list({}), [])
      },
    },
  )
  expect(screen.getByRole("region", { name: first.title })).toBeDefined()
  await act(async () => {
    go?.({ pathname: paths.goals(), search: "?goal=g2" })
  })
  expect(await screen.findByRole("region", { name: second.title })).toBeDefined()
  expect(document.querySelectorAll('[data-slot="docked-pane"]')).toHaveLength(1)
})

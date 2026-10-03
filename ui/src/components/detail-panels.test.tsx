// @vitest-environment jsdom

/**
 * Focus across the panel stack, driven the way the app drives it: by the URL.
 *
 * `DetailPanels` is the piece that decides which panels are open and which of
 * them owns `?session=`, and that decision is exactly what focus depends on —
 * so the real thing is mounted rather than a stand-in for it. The daemon never
 * answers, which is enough: every panel has a pending state, and none of them
 * needs data to be a pane with focus in it.
 *
 * The case is the one the goal panel's own params make easy to get wrong. A
 * goal drilled into a session, followed to the task that session ran, opens a
 * task view in the pane — and the goal panel's `?session=` goes away in the same
 * navigation, which is *not* the user coming back out of that session. Focus
 * belongs to the sheet on top; see `hooks/use-focus-return.ts`.
 *
 * Where that focus *lands* is the second thing here: a stacked task panel opens
 * on its breadcrumb, which is therefore the one control in the app a deep link
 * puts a focus ring on before anything is clicked. It wears the app's own ring
 * rather than the browser's outline.
 */

import { act, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { type NavigateFunction, useNavigate } from "react-router-dom"
import { expect, it } from "vitest"
import { qk } from "@/api"
import { GoalSwimlanes } from "@/features/goals/goal-swimlanes"
import { paths } from "@/routes/paths"
import { aGoal } from "@/test/fixtures"
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

/** The pane a given title belongs to; only its top level is visible. */
function sheetTitled(title: string): HTMLElement {
  const sheet = screen.getByText(title).closest('[data-slot="docked-pane"]')
  if (!sheet) throw new Error(`no sheet holding "${title}"`)
  return sheet as HTMLElement
}

it("leaves focus in the task sheet stacked over a goal's session", async () => {
  mountPanels(`${paths.goals()}?goal=g1&tab=sessions&session=s1`)
  const goalSheet = sheetTitled("Session s1")
  // The pane takes focus on mount.
  await waitFor(() => expect(goalSheet.contains(document.activeElement)).toBe(true))

  // The Task link of a session shown inside a goal: `taskPanelTo` adds `?task=`
  // and drops the panel's own `tab`/`session` in the one navigation.
  await act(async () => {
    go?.({ pathname: paths.goals(), search: "?goal=g1&task=t1" })
  })

  // On the spot, and about the panel underneath: the sheet on top asserts its
  // own focus a tick later, which would paper over the goal panel reading this
  // navigation as "came back out of the session" and grabbing focus first.
  expect(goalSheet.contains(document.activeElement)).toBe(false)

  const taskSheet = sheetTitled("Loading task")
  expect(taskSheet).not.toBe(goalSheet)
  await waitFor(() => expect(taskSheet.contains(document.activeElement)).toBe(true))
})

it("gives the stacked panel's breadcrumb the app's own focus ring", async () => {
  mountPanels(`${paths.goals()}?goal=g1&task=t1`)

  // The goal is still loading, so the button wears the word rather than the
  // title — it is the first focusable thing in the sheet either way.
  const nav = await screen.findByLabelText("Breadcrumb")
  const breadcrumb = nav.querySelector("button")
  if (!breadcrumb) throw new Error("no way back to the goal in the breadcrumb")
  await waitFor(() => expect(document.activeElement).toBe(breadcrumb))
  expect(breadcrumb.className).toContain("focus-visible:ring-3")
  expect(breadcrumb.className).toContain("outline-none")
})

it("keeps the board accessible and follows another lane title with a goal open", async () => {
  const first = aGoal({ id: "g1", title: "First lane" })
  const second = aGoal({ id: "g2", title: "Second lane" })
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname === "/v1/goals/g2") return Promise.resolve(jsonResponse(second))
    return new Promise(() => {})
  })
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
        client.setQueryData(qk.goals.detail(second.id), second)
        client.setQueryData(qk.tasks.list({}), [])
      },
    },
  )
  expect(screen.getByRole("main")).toBeDefined()
  await userEvent.setup().click(screen.getByRole("link", { name: second.title }))
  expect(location.url).toBe("/goals?goal=g2")
  expect(await screen.findByRole("region", { name: second.title })).toBeDefined()
  expect(screen.queryByRole("dialog")).toBeNull()
})

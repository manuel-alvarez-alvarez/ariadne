// @vitest-environment jsdom

/**
 * Where focus goes when a panel closes, and when one of them is drilled into
 * and back out of.
 *
 * Rendered rather than unit-tested because focus is not state anybody here
 * owns: it is the browser's, moved by Base UI's dialog on one side and by this
 * hook on the other, and the only way to know which of the two answered is to
 * mount the thing and press the keys.
 *
 * The sheet is the real one, and it is mounted the way the app mounts it — by
 * a URL, unmounted whole when that URL changes (see `detail-panels.tsx`) — so
 * the first case here is the regression test for the dialog keeping its end of
 * the bargain through an unmount, which is what lets the hook stay this small.
 */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { StrictMode, useRef } from "react"
import { MemoryRouter, useNavigate, useSearchParams } from "react-router-dom"
import { afterEach, expect, it } from "vitest"

import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"

import { useFocusReturn } from "./use-focus-return"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

/** A screen with two cards on it, and the panel one of them opens. */
function Screen({ withRow = true }: { withRow?: boolean }) {
  const [search, setSearch] = useSearchParams()
  const navigate = useNavigate()
  const task = search.get("task")
  const session = search.get("session")
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(session, panel)

  return (
    <>
      <button type="button" onClick={() => setSearch({ task: "t1" })}>
        card one
      </button>
      <button type="button" onClick={() => setSearch({ task: "t2" })}>
        card two
      </button>
      {task ? (
        <Sheet open onOpenChange={(open) => open || navigate(-1)}>
          <SheetContent ref={panel} aria-describedby={undefined}>
            <SheetTitle>Task {task}</SheetTitle>
            {session ? (
              <div>
                <button type="button" onClick={() => setSearch({ task }, { replace: true })}>
                  Back to the task
                </button>
              </div>
            ) : (
              // Shaped like the sessions table it stands in for: a different
              // tree from the view above, so going back really does rebuild
              // the row rather than reusing its node.
              <table>
                <tbody>
                  <tr>
                    <td>
                      {withRow ? (
                        <button
                          type="button"
                          data-focus-return="s1"
                          onClick={() => setSearch({ task, session: "s1" }, { replace: true })}
                        >
                          Open Engineer session
                        </button>
                      ) : null}
                    </td>
                  </tr>
                </tbody>
              </table>
            )}
          </SheetContent>
        </Sheet>
      ) : null}
    </>
  )
}

function mount({ at = "/", ...props }: { at?: string; withRow?: boolean } = {}) {
  render(
    <StrictMode>
      <MemoryRouter initialEntries={[at]}>
        <Screen {...props} />
      </MemoryRouter>
    </StrictMode>,
  )
  return userEvent.setup()
}

/** The panel's own drill-down, opened from the row that stands for a session. */
async function drillIn(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Open Engineer session" }))
  return screen.findByRole("button", { name: "Back to the task" })
}

it("hands focus back to the card that opened a panel when it closes", async () => {
  const user = mount()
  const card = screen.getByRole("button", { name: "card two" })
  card.focus()
  await user.keyboard("{Enter}")
  await screen.findByText("Task t2")

  await user.keyboard("{Escape}")
  await screen.findByRole("button", { name: "card two" })
  expect(document.activeElement).toBe(card)
})

it("hands focus back to the row a session was opened from", async () => {
  const user = mount()
  await user.click(screen.getByRole("button", { name: "card one" }))
  const back = await drillIn(user)

  await user.click(back)
  expect(document.activeElement).toBe(
    await screen.findByRole("button", { name: "Open Engineer session" }),
  )
})

it("falls back to the panel itself when there is no such row", async () => {
  // A link straight into a session: the panel was never on the tab that holds
  // the row, so going back has nothing to give focus to but itself — which is
  // still inside the dialog, and not `<body>` outside it.
  const user = mount({ at: "/?task=t1&session=s1", withRow: false })
  await user.click(await screen.findByRole("button", { name: "Back to the task" }))

  expect(document.activeElement).toBe(document.querySelector('[data-slot="sheet-content"]'))
})

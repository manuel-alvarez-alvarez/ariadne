// @vitest-environment jsdom

/**
 * The expanded terminal and the renderer behind it.
 *
 * A panel is a small window onto a pane, so the terminal can be lifted into a
 * dialog. What is worth pinning down about that move is not the size — that is
 * layout, and jsdom has none — but the three things it could quietly break:
 * the emulator has to *travel*, keeping the connection and the output it
 * already has rather than opening a new stream and fetching the pane again; it
 * has to still be the pane's keyboard once it is in there; and it is inside a
 * dialog that closes on Escape, which is also a keystroke the agent's TUI
 * expects to be handed.
 *
 * Sizing is the third thing, and the one with a request behind it: a live
 * pane is asked for the grid its frame has room for, once the frame has
 * stopped moving, and a session that is over is never asked at all. What the
 * frame measures to is layout, which jsdom does not have — so {@link paneFit}
 * is stubbed with the answer a laid-out frame would give, and what is pinned
 * down here is what the terminal does with it.
 *
 * The renderer is the other half. The GPU one is fetched only once the
 * emulator is open, and every way of not getting it — no WebGL2 context, an
 * addon that will not load, a context the driver takes back — has to leave the
 * terminal drawing through the DOM rather than not drawing at all.
 *
 * The panel is the third frame, and the same question in it: the sheets a
 * terminal appears in dismiss on Escape like every dialog in the app, and a
 * pane being typed into is what makes that press the agent's instead. That is
 * `PanelSheet`'s guard, driven here through a real emulator because what it
 * asks about — who holds the keyboard — is a fact of the real one.
 *
 * xterm needs a browser this environment only half is, so `matchMedia`,
 * `ResizeObserver` and the canvas are stubbed for it. Everything else is real:
 * a real emulator, keystrokes typed into it the way a user types them, and the
 * real `openapi-fetch` path behind a controllable `fetch`.
 */

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useLayoutEffect } from "react"
import { expect, it, vi } from "vitest"

import { PanelSheet } from "@/components/panel-sheet"
import { SheetContent, SheetTitle } from "@/components/ui/sheet"

import {
  connections,
  emulator,
  frameResizedTo,
  installTerminalEnvironment,
  keyboardOf,
  keystrokes,
  resizes,
  resizeTargets,
  settle,
  streams,
  stub,
} from "@/test/terminal-harness"

import { SessionTerminal } from "./session-terminal"

// The one thing in the fit that needs a laid-out frame. Its own arithmetic is
// covered in `pane-fit.test.ts`; here it stands in for a browser that measures.
vi.mock("./pane-fit", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./pane-fit")>()),
  paneFit: () => {
    stub.fits += 1
    return stub.roomForPane
  },
}))

// The real addon is a WebGL renderer, which is exactly the thing this
// environment does not have; what these tests are about is what the component
// does with it, so the addon is a stand-in that records being used.
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    constructor() {
      stub.webglRequests += 1
    }

    activate(): void {
      if (stub.webglRefuses) throw new Error("WebGL2 is not supported")
      stub.webglLoads += 1
    }

    dispose(): void {
      stub.webglDisposals += 1
    }

    onContextLoss(handler: () => void): { dispose: () => void } {
      stub.loseWebglContext = handler
      return { dispose: () => {} }
    }
  },
}))

const SESSION = "01ARZ3NDEKTSV4RRFFQ69G5FAV"

installTerminalEnvironment()

function expand(): Promise<void> {
  return userEvent.setup().click(screen.getByRole("button", { name: /expand the terminal/i }))
}

/**
 * Fires whatever timers are due from inside a *layout* effect. Rendered after
 * the terminal, that lands in the window a due timer would: the commit has
 * happened and every layout effect of it has run, and not one passive effect
 * — neither a cleanup nor a setup — has.
 */
function FireTimersOnCommit({ armed }: { armed: boolean }) {
  useLayoutEffect(() => {
    if (armed) vi.advanceTimersByTime(500)
  })
  return null
}

it("carries the same emulator and stream into the dialog and back", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  expect(connections).toHaveLength(1)
  const pane = emulator()

  await expand()

  // The panel says where the terminal went rather than going blank.
  expect(screen.getByText(/open in the expanded view/i)).toBeTruthy()
  const dialog = screen.getByRole("dialog")
  // Moved, not rebuilt: a second emulator would mean a second connection, and
  // a whole snapshot fetched for output that is already on screen.
  expect(emulator()).toBe(pane)
  expect(dialog.contains(pane)).toBe(true)
  expect(connections).toHaveLength(1)
  expect(connections[0]?.closed).toBe(false)

  // And it is still the pane's keyboard in there.
  keyboardOf(dialog).focus()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")

  // The dialog is modal, so the way back is its own collapse control; the
  // panel's "Back to the panel" is behind it, for once the dialog is gone.
  await user.click(screen.getByRole("button", { name: /collapse the terminal/i }))

  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.queryByText(/open in the expanded view/i)).toBeNull()
  expect(emulator()).toBe(pane)
  expect(connections).toHaveLength(1)
  expect(connections[0]?.closed).toBe(false)
})

it("hands Escape to the agent while the terminal has the keyboard", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await expand()

  keyboardOf(screen.getByRole("dialog")).focus()
  await user.keyboard("{Escape}")

  // The dialog closing on that press would take the pane away from under the
  // keystroke that was meant for it.
  expect(screen.getByRole("dialog")).toBeTruthy()

  // And the pane is still being typed into from in here.
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

it("still closes on Escape from outside the terminal", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await expand()

  screen.getByRole("dialog").focus()
  await user.keyboard("{Escape}")

  expect(screen.queryByRole("dialog")).toBeNull()
})

/** The terminal as a panel shows it: a sheet, with the pane inside. */
function inPanel(onClose: () => void) {
  render(
    <PanelSheet onClose={onClose}>
      <SheetContent aria-describedby={undefined}>
        <SheetTitle className="sr-only">Session</SheetTitle>
        <SessionTerminal sessionId={SESSION} status="running" />
      </SheetContent>
    </PanelSheet>,
  )
}

it("keeps the panel open when Escape was meant for the pane", async () => {
  const user = userEvent.setup()
  const closed = vi.fn()
  inPanel(closed)

  // The sheet takes the focus on the way in; the pane is clicked into after
  // that, which is what puts the keyboard in it.
  await settle()
  keyboardOf(emulator()).focus()
  await user.keyboard("{Escape}")

  // The panel closing would take the pane away from under a keystroke aimed
  // at the agent — and the agent would never get the keystroke either.
  expect(closed).not.toHaveBeenCalled()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

it("still closes the panel on Escape from outside the pane", async () => {
  const user = userEvent.setup()
  const closed = vi.fn()
  inPanel(closed)
  await settle()

  screen.getByRole("dialog").focus()
  await user.keyboard("{Escape}")

  expect(closed).toHaveBeenCalled()
})

it("draws through the GPU where there is one, and drops it with its context", async () => {
  render(<SessionTerminal sessionId={SESSION} status="running" />)

  // The addon is fetched only once the emulator is open, so it lands a turn
  // after the terminal does.
  await vi.waitFor(() => expect(stub.webglLoads).toBe(1))

  // A context the driver takes back leaves xterm drawing through the DOM
  // again, which is all disposing the addon means; asking for another one
  // would only lose it the same way.
  expect(stub.loseWebglContext).toBeTypeOf("function")
  stub.loseWebglContext?.()
  expect(stub.webglDisposals).toBe(1)
  expect(emulator()).toBeTruthy()
})

it("falls back to the DOM renderer when the WebGL addon will not load", async () => {
  const user = userEvent.setup()
  stub.webglRefuses = true
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await settle()

  expect(stub.webglRequests).toBe(1)
  expect(stub.webglLoads).toBe(0)
  // The renderer xterm opened with never went anywhere, so this is still a
  // terminal: it is on the page, and it is still typed into.
  keyboardOf(emulator()).focus()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

it("does not fetch the WebGL addon without a context to draw into", async () => {
  stub.webglContext = null
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await settle()

  // Not merely unused: a renderer this browser cannot run is a download it
  // should never have made.
  expect(stub.webglRequests).toBe(0)
  expect(stub.loseWebglContext).toBeNull()
  expect(emulator()).toBeTruthy()
})

it("asks a live pane for the grid its frame has room for", async () => {
  render(<SessionTerminal sessionId={SESSION} status="running" />)

  await vi.waitFor(() => expect(resizes).toEqual([{ cols: 137, rows: 41 }]))
  // The grid the emulator draws at is the daemon's to change: it comes back
  // through the stream, and nothing here anticipates it.
  expect(keystrokes).toEqual([])
})

it("asks once for a frame that moved several times", async () => {
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await vi.waitFor(() => expect(resizes).toHaveLength(1))

  // The first notification is the frame as it already is: a baseline.
  frameResizedTo(600, 300)
  stub.roomForPane = { cols: 100, rows: 30 }
  frameResizedTo(700, 300)
  // Still mid-drag: this is the size the pane should end up at, and the one
  // before it should never be asked for.
  stub.roomForPane = { cols: 120, rows: 30 }
  frameResizedTo(800, 300)

  await vi.waitFor(() => expect(resizes).toHaveLength(2))
  expect(resizes[1]).toEqual({ cols: 120, rows: 30 })
  // And a frame that stops where it already was is not a resize at all.
  frameResizedTo(900, 300)
  await settle()
  expect(resizes).toHaveLength(2)
})

/**
 * An agent can finish in the time a frame takes to settle. The fit measured a
 * moment ago was for a pane that is now gone, and asking for it would be a
 * 409 for a grid nobody will ever draw at.
 */
it("drops a fit in the queue when the session ends under it", async () => {
  const { rerender } = render(<SessionTerminal sessionId={SESSION} status="running" />)
  // Well inside the debounce: the fit is scheduled and has not gone out.
  expect(resizes).toEqual([])
  rerender(<SessionTerminal sessionId={SESSION} status="exited" />)

  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(resizes).toEqual([])
  // And the terminal that is left is the finished session's: display-only,
  // still on the page.
  expect(emulator()).toBeTruthy()
})

/**
 * The same thing, in the one moment a passive guard would miss it.
 *
 * React commits a render — layout effects and all — and *schedules* the
 * passive effects; a 200 ms timer that came due in between runs first, and a
 * guard kept in a `useEffect` would still be saying the session was live. The
 * window is reproduced by firing the timer from the layout effect of a sibling
 * rendered after the terminal: React runs effects in tree order, so at that
 * point every layout effect of the commit has run and no passive one has.
 */
it("drops a fit committed dead before the passive effects have run", async () => {
  vi.useFakeTimers()
  const { rerender } = render(
    <>
      <SessionTerminal sessionId={SESSION} status="running" />
      <FireTimersOnCommit armed={false} />
    </>,
  )
  expect(resizes).toEqual([])

  rerender(
    <>
      <SessionTerminal sessionId={SESSION} status="exited" />
      <FireTimersOnCommit armed />
    </>,
  )

  // Back to real time, and a turn of the loop: a request the fit did make
  // would be recorded by now.
  vi.useRealTimers()
  await settle()
  expect(resizes).toEqual([])
})

/**
 * A panel that moves to another session has a fit in the queue measured for
 * the one it was showing — and that pane is still live, so nothing but the
 * guard stops the request from going to it.
 */
it("drops a fit in the queue when the panel moves to another session", async () => {
  const other = "01ARZ3NDEKTSV4RRFFQ69G5FBW"
  const { rerender } = render(<SessionTerminal sessionId={SESSION} status="running" />)
  rerender(<SessionTerminal sessionId={other} status="running" />)

  // The second session asks for its own frame; the first one never does.
  await vi.waitFor(() => expect(resizes).toHaveLength(1))
  await settle()
  expect(resizes).toHaveLength(1)
  expect(resizeTargets).toEqual([other])
})

it("never asks a session that is over to resize", async () => {
  render(<SessionTerminal sessionId={SESSION} status="exited" />)
  frameResizedTo(600, 300)
  frameResizedTo(700, 300)
  await new Promise((resolve) => setTimeout(resolve, 300))

  // There is no pane behind a finished session; its replay is drawn at the
  // size the pane had when it was written, scaled to the frame.
  expect(resizes).toEqual([])
  expect(stub.fits).toBe(0)
})

it("keeps working when the daemon refuses to resize the pane", async () => {
  const user = userEvent.setup()
  stub.resizeFails = true
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await vi.waitFor(() => expect(stub.fits).toBeGreaterThan(0))
  await new Promise((resolve) => setTimeout(resolve, 300))

  // A pane that was not resized is a pane rendered smaller — not a terminal
  // that stopped drawing, and not one that stopped being typed into.
  expect(resizes).toEqual([])
  keyboardOf(emulator()).focus()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

/**
 * The pane can change shape without this frame having moved — another panel
 * fitting it to itself, a client attaching — and the emulator's own cells can
 * change size under a fit that was already made, which is what a webfont
 * landing after the terminal opened does. The first must not turn into two
 * viewers resizing the pane at each other; the second has to be noticed.
 */
it("measures again when the grid changes, and asks only when the answer does", async () => {
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await vi.waitFor(() => expect(resizes).toEqual([{ cols: 137, rows: 41 }]))

  // Somebody else's fit. This frame still has room for exactly what it asked
  // for, so it has nothing new to say.
  streams[0]?.emit("resize", { cols: 90, rows: 20 })
  await vi.waitFor(() => expect(stub.fits).toBeGreaterThan(1))
  await settle()
  expect(resizes).toHaveLength(1)

  // The same event, but this time the frame measures differently — cells that
  // settled on another size. That is a fit worth asking for.
  stub.roomForPane = { cols: 111, rows: 30 }
  streams[0]?.emit("resize", { cols: 90, rows: 21 })
  await vi.waitFor(() => expect(resizes).toHaveLength(2))
  expect(resizes[1]).toEqual({ cols: 111, rows: 30 })
})

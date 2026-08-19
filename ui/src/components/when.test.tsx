// @vitest-environment jsdom

/**
 * The two things a timestamp on this app has to do, and neither is formatting.
 *
 * It has to stay true — every screen holds one, and a board left open used to
 * go on saying "2 minutes ago" for an hour — so the tests advance fake timers
 * and read the text again rather than asserting the first render.
 *
 * And the exact stamp behind it has to be reachable by a keyboard, which is
 * the whole reason this is a `Tooltip` and not the `title=` it replaced: those
 * open on hover and nowhere else. So the test is Tab and read.
 */

import { act, cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { formatAbsolute } from "@/lib/time"

import { When } from "./when"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const NOW = new Date("2026-08-19T12:00:00Z")
const A_MINUTE_AGO = "2026-08-19T11:59:00Z"

function mount(ui: React.ReactNode) {
  render(<TooltipProvider delay={0}>{ui}</TooltipProvider>)
}

it("re-renders the relative text as the clock moves, without a reload", () => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
  mount(<When at={A_MINUTE_AGO} />)
  expect(screen.getByText("1 minute ago")).not.toBeNull()

  // Past the shared tick, which is what re-renders every timestamp on screen.
  act(() => void vi.advanceTimersByTime(4 * 60_000))
  expect(screen.getByText("5 minutes ago")).not.toBeNull()
})

it("moves the compact form on the same clock", () => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
  mount(<When at={A_MINUTE_AGO} format="age" />)
  expect(screen.getByText("1m")).not.toBeNull()

  act(() => void vi.advanceTimersByTime(4 * 60_000))
  expect(screen.getByText("5m")).not.toBeNull()
})

it("opens the absolute stamp on focus, not only on hover", async () => {
  const user = userEvent.setup()
  mount(<When at={A_MINUTE_AGO} label="updated" />)

  await user.tab()
  expect(screen.getByText(`updated ${formatAbsolute(A_MINUTE_AGO)}`)).not.toBeNull()
})

it("carries the sibling stamps a row has no room for", async () => {
  const user = userEvent.setup()
  mount(
    <When
      at={A_MINUTE_AGO}
      label="updated"
      detail={<span>created {formatAbsolute("2026-08-01T09:00:00Z")}</span>}
    />,
  )

  await user.tab()
  expect(screen.getByText(`created ${formatAbsolute("2026-08-01T09:00:00Z")}`)).not.toBeNull()
})

it("carries the instant a machine reads", () => {
  const { container } = render(
    <TooltipProvider delay={0}>
      <When at={A_MINUTE_AGO} />
    </TooltipProvider>,
  )
  expect(container.querySelector("time")?.getAttribute("datetime")).toBe(A_MINUTE_AGO)
})

/** A dash is not worth a focus stop: there is no stamp behind it to show. */
it("renders a nullish stamp as a dash with nothing to open", async () => {
  const user = userEvent.setup()
  mount(<When at={null} label="ended" />)

  expect(screen.getByText("—")).not.toBeNull()
  await user.tab()
  expect(screen.queryByText(/ended/)).toBeNull()
})

// @vitest-environment jsdom

/**
 * What the hook has to notice, and the half of it a scrollport cannot report.
 *
 * A port's own box is the width its parent gives it: it does not change when
 * the table inside it grows a column, so a `ResizeObserver` pointed at the port
 * alone stays silent through the one event that makes a table too wide — the
 * rows arriving. Every list in the app mounts loading skeletons first and gets
 * its real columns a request later, so that silence is exactly the moment the
 * fade is needed and exactly the moment it was missing.
 *
 * jsdom lays nothing out, so the two measurements and the observer are the
 * test's to supply: the widths are defined on the node, and the stub below
 * fires a callback only for the elements that observer was actually pointed at
 * — which is what makes "the content is observed" a behaviour rather than an
 * assertion about a call.
 */

import { act, render, screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

import { useHorizontalOverflow } from "./use-scroll-overflow"

/** One live `ResizeObserver`: what it watches, and how to make it report. */
interface Watch {
  targets: Element[]
  /** Reports a resize of `target`, if this observer is watching it at all. */
  fire: (target: Element) => void
}

/** Every observer the render made, in the order they were constructed. */
function watchResizes(): Watch[] {
  const watches: Watch[] = []
  vi.stubGlobal(
    "ResizeObserver",
    class {
      private readonly targets: Element[] = []
      constructor(callback: () => void) {
        watches.push({
          targets: this.targets,
          fire: (target) => {
            if (this.targets.includes(target)) callback()
          },
        })
      }
      observe(target: Element) {
        this.targets.push(target)
      }
      unobserve(target: Element) {
        const at = this.targets.indexOf(target)
        if (at >= 0) this.targets.splice(at, 1)
      }
      disconnect() {
        this.targets.length = 0
      }
    },
  )
  return watches
}

/** A scrollport with one element in it, the way both callers build theirs. */
function Port() {
  const scroll = useHorizontalOverflow<HTMLDivElement>()
  return (
    <>
      <div ref={scroll.ref} data-testid="port">
        <div data-testid="content" />
      </div>
      <output>{scroll.overflow.end ? "cut short" : "all of it"}</output>
    </>
  )
}

/** What jsdom will not measure: how wide the port is, and how wide its content. */
function widths(port: HTMLElement, { client, scroll }: { client: number; scroll: number }) {
  Object.defineProperty(port, "clientWidth", { configurable: true, value: client })
  Object.defineProperty(port, "scrollWidth", { configurable: true, value: scroll })
}

function mount(): { port: HTMLElement; content: HTMLElement; watches: Watch[] } {
  const watches = watchResizes()
  render(<Port />)
  return {
    port: screen.getByTestId("port"),
    content: screen.getByTestId("content"),
    watches,
  }
}

it("says so when the content grows past a port that has not moved", () => {
  const { port, content, watches } = mount()

  // The rows are still skeletons: everything fits, and the caller draws no fade.
  widths(port, { client: 600, scroll: 600 })
  act(() => {
    for (const watch of watches) watch.fire(port)
  })
  expect(screen.getByRole("status").textContent).toBe("all of it")

  // The daemon answers and the table lays out its real columns. The port is
  // the same width it was — its parent decides that — so a resize of the port
  // is never reported, and only the content's own is left to notice.
  widths(port, { client: 600, scroll: 900 })
  act(() => {
    for (const watch of watches) watch.fire(content)
  })
  expect(screen.getByRole("status").textContent).toBe("cut short")
})

it("still notices the window narrowing under a content that has not changed", () => {
  const { port, watches } = mount()

  widths(port, { client: 400, scroll: 900 })
  act(() => {
    for (const watch of watches) watch.fire(port)
  })

  expect(screen.getByRole("status").textContent).toBe("cut short")
})

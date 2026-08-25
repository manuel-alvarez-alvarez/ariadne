/**
 * Whether a scroll container is hiding content past its left or right edge.
 *
 * A scrollport gives nothing away on its own: macOS draws no scrollbar until
 * something moves, so a board whose fifth column falls off the viewport looks
 * like a board with four columns. The answer cannot be had in CSS — it is a
 * measurement — so it is taken here and turned into whatever the caller draws
 * with it (see the board's edge fades in `goal-swimlanes.tsx`).
 *
 * Measured on scroll and on every resize of the port itself, which is what a
 * window narrowing and a scrollbar appearing both come down to.
 */

import { useEffect, useState } from "react"

interface ScrollOverflow {
  /** Content is hidden to the left of the scrollport. */
  start: boolean
  /** Content is hidden to its right. */
  end: boolean
}

/**
 * Sub-pixel widths make `scrollLeft` land a fraction short of the end; a whole
 * pixel of hidden content is not an affordance worth drawing.
 */
const EPSILON = 1

export function useHorizontalOverflow<T extends HTMLElement>(): {
  /** Put this on the element that scrolls. */
  ref: (node: T | null) => void
  overflow: ScrollOverflow
} {
  // A callback ref rather than `useRef`, so the effect runs once the node is
  // actually there — the board is behind a loading state, and a ref object
  // does not re-render when it is filled in.
  const [node, setNode] = useState<T | null>(null)
  const [overflow, setOverflow] = useState<ScrollOverflow>({ start: false, end: false })

  useEffect(() => {
    if (!node) return

    function measure() {
      if (!node) return
      const hidden = node.scrollWidth - node.clientWidth - node.scrollLeft
      const next = { start: node.scrollLeft > EPSILON, end: hidden > EPSILON }
      // Every scroll event would otherwise re-render the whole board.
      setOverflow((current) =>
        current.start === next.start && current.end === next.end ? current : next,
      )
    }

    measure()
    node.addEventListener("scroll", measure, { passive: true })
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    return () => {
      node.removeEventListener("scroll", measure)
      observer.disconnect()
    }
  }, [node])

  return { ref: setNode, overflow }
}

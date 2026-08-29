/**
 * The one hint a scrollport gives that it goes on past its edge.
 *
 * macOS keeps its scrollbar hidden until something moves, so a board whose
 * fifth column falls off the window looks like a board with four, and a table
 * cut at "Las…" looks like a table with a typo in its heading. Content fading
 * out under the edge it is cut at is what says otherwise, on whichever side
 * has more of it.
 *
 * Two layers, because a fade alone says nothing where there is nothing to fade:
 * the wash, which is what makes a clipped card or a clipped column label trail
 * off, and a shade right at the edge, which is visible over an empty cell too.
 *
 * It was the goals board's alone; every table that can be cut sideways now
 * draws the same two edges through {@link ScrollableTable}, which is the whole
 * pattern — frame, scrollport, measurement and fades — for the surfaces that
 * are a table rather than a board.
 */

import type { ReactNode } from "react"

import { Table } from "@/components/ui/table"
import { useHorizontalOverflow } from "@/hooks/use-scroll-overflow"
import { cn } from "@/lib/format"

/**
 * A table in its own frame, saying so when it is wider than that frame.
 *
 * The scrollport is the table's own container, which is where the measurement
 * has to be taken: the frame around it never scrolls, and it is the frame the
 * fades are positioned against.
 */
export function ScrollableTable({
  className,
  children,
}: {
  /** The frame: its radius and border, which differ between the list screens and the panels. */
  className?: string
  children: ReactNode
}) {
  const scroll = useHorizontalOverflow<HTMLDivElement>()
  return (
    <div className={cn("relative overflow-hidden", className)}>
      <Table containerRef={scroll.ref}>{children}</Table>
      <ScrollEdge side="start" show={scroll.overflow.start} />
      <ScrollEdge side="end" show={scroll.overflow.end} />
    </div>
  )
}

/**
 * One faded edge, over whatever the scrollport holds.
 *
 * Drawn above any sticky header in the same stacking context (`z-30`, against
 * the board's `z-20` column row and `z-10` lane names), so the header a title
 * is cut in half by is exactly where it shows.
 */
export function ScrollEdge({ side, show }: { side: "start" | "end"; show: boolean }) {
  const start = side === "start"
  return (
    <div
      aria-hidden
      className={cn(
        // Inset by the border it sits inside, and pointer-transparent: this is
        // a picture of a state, never a target.
        "pointer-events-none absolute inset-y-px z-30 w-10 transition-opacity duration-150",
        start ? "left-px rounded-l-lg" : "right-px rounded-r-lg",
        show ? "opacity-100" : "opacity-0",
      )}
    >
      <div
        className={cn(
          "absolute inset-0 from-background to-transparent",
          start ? "bg-linear-to-r" : "bg-linear-to-l",
        )}
      />
      <div
        className={cn(
          // From the foreground colour rather than a fixed black, so it shows
          // against the page in either theme.
          "absolute inset-y-0 w-2.5 from-foreground/12 to-transparent",
          start ? "left-0 bg-linear-to-r" : "right-0 bg-linear-to-l",
        )}
      />
    </div>
  )
}

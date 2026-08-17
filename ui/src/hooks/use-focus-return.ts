/**
 * Focus, back where a drill-down started.
 *
 * A panel *closing* needs nothing from us: the sheet is a Base UI dialog, and
 * it remembers the element that had focus when it opened — the card or the row
 * that was clicked — and gives it back on close, unmount included. That covers
 * Escape, the close button and an outside press.
 *
 * A drill-down is the case it cannot cover, because nothing closes. `?session=`
 * swaps the panel's whole body for that session's view while the dialog stays
 * open, so the control that was activated (the row on the way in, "Back to the
 * task" on the way out) is unmounted from under the keyboard and focus drops to
 * `<body>` — outside the panel it is still inside.
 *
 * So the panel says what it is drilled into, and when that goes away focus
 * returns to whatever carries `data-focus-return="<id>"` — the row that opened
 * it, which the tab it is on renders again. There is not always one: a link can
 * open a panel straight into a session, and going back then lands on a tab that
 * never had that row. The panel itself is the fallback, which is the top of the
 * view the user just came back to and, crucially, still inside the dialog.
 */

import { type RefObject, useEffect, useRef } from "react"

export function useFocusReturn(
  /** What the panel is drilled into, or `null` when it is showing itself. */
  drilledInto: string | null,
  /** The panel, focused when the element that opened the drill-down is gone. */
  panel: RefObject<HTMLElement | null>,
) {
  const previous = useRef(drilledInto)

  useEffect(() => {
    const left = previous.current
    previous.current = drilledInto
    // Only the way back out, and only from somewhere: opening a drill-down
    // moves focus into it by itself, and a panel that was never in one has
    // nothing to return to.
    if (drilledInto !== null || left === null) return

    const opener = panel.current?.querySelector<HTMLElement>(
      `[data-focus-return="${CSS.escape(left)}"]`,
    )
    ;(opener ?? panel.current)?.focus()
  }, [drilledInto, panel])
}

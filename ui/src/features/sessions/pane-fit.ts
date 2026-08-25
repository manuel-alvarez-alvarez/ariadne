/**
 * The grid to ask a live pane for, worked out from the room its viewer has.
 *
 * A tmux pane is sized by whatever attached to it last, so a browser that
 * never attaches is shown 80×24 forever and can only make it *look* bigger by
 * growing the font. Asking for a size instead — `POST /v1/sessions/{id}/resize`
 * — needs the one number the frame does not state: how many cells fit in it.
 *
 * Columns come from xterm's own fit addon, which measures a cell against the
 * element the emulator draws in. Rows cannot: in the panel the frame grows
 * with the terminal, so the height the addon would measure is the height the
 * grid already has — the answer to the question, taken as the question. What
 * a row costs is measured instead (the screen's height over the rows drawn in
 * it), and the rows are however many of those fit the budget the frame allows.
 *
 * The arithmetic lives here, away from the DOM reads that feed it, because
 * every one of its edge cases is a frame that has not been laid out yet: a
 * zero-height screen, a font whose cells have not been measured, a frame with
 * no room at all. None of them is a grid, and a pane must not be asked for
 * one.
 *
 * The other half of fitting is the font — the one dimension that is ours to
 * choose while the pane is still the size somebody else made it, or is over and
 * cannot be asked at all. {@link nextFontSize} is its one step.
 */

import type { PaneSize } from "./log-stream"

/**
 * Largest grid a pane may be asked for, per side. The daemon refuses anything
 * over this (`MAX_PANE_SIDE` in `http/sessions.rs`); the same bound is applied
 * here so a mismeasured frame is never a request at all.
 */
const MAX_PANE_SIDE = 500
/**
 * Smallest grid worth asking for. Under it a TUI has nothing to draw in, and
 * a frame that narrow is better off scrolling a pane it cannot fit — which is
 * what a terminal too wide for its frame already does.
 */
const MIN_PANE_COLS = 20
const MIN_PANE_ROWS = 5

/** What one measurement of a frame yields, before it is a grid. */
interface PaneFitMeasurement {
  /**
   * Columns the frame has room for at the font the emulator is drawing at, as
   * `FitAddon.proposeDimensions()` reports it — `undefined` until the font's
   * cells have been measured.
   */
  cols: number | undefined
  /** Rows the emulator is drawing, and the height in px its screen takes. */
  rows: number
  screenHeight: number
  /** How tall the grid may get in this frame, in px. */
  heightBudget: number
}

/**
 * The grid that fills this frame, or `null` when the frame cannot yet say.
 */
export function paneFit({
  cols,
  rows,
  screenHeight,
  heightBudget,
}: PaneFitMeasurement): PaneSize | null {
  const cellHeight = rows > 0 ? screenHeight / rows : 0
  if (!cols || !Number.isFinite(cols) || cols <= 0) return null
  if (!Number.isFinite(cellHeight) || cellHeight <= 0) return null
  if (!Number.isFinite(heightBudget) || heightBudget <= 0) return null
  return {
    cols: clamp(Math.floor(cols), MIN_PANE_COLS, MAX_PANE_SIDE),
    rows: clamp(Math.floor(heightBudget / cellHeight), MIN_PANE_ROWS, MAX_PANE_SIDE),
  }
}

/** Whether two grids are the same one. */
export function sameSize(a: PaneSize | null, b: PaneSize | null): boolean {
  return a !== null && b !== null && a.cols === b.cols && a.rows === b.rows
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

/**
 * Font sizes the pane may be scaled to. Below the lower bound a monospace grid
 * stops being readable, so a pane too wide for the frame overflows it and
 * scrolls sideways instead of shrinking into illegibility.
 */
const MIN_FONT_SIZE = 8
export const MAX_FONT_SIZE = 15
/**
 * The ceiling in the expanded frame, where the pane is the whole screen and not
 * one card among others. It is high enough that the room — the dialog's height,
 * or its width for a wide pane — is what stops the font on any usual grid,
 * rather than the ceiling standing in for the panel's.
 */
export const EXPANDED_MAX_FONT_SIZE = 24
/** Where scaling starts, and what a pane is drawn at when it fits as it is. */
export const BASE_FONT_SIZE = 12
export const LINE_HEIGHT = 1.2
/** Tallest the grid may get, in the panel, before the font shrinks to fit (`28rem`). */
export const MAX_SCREEN_HEIGHT = 448

/**
 * The font size that would fit this grid into this frame, from one measurement
 * of it.
 *
 * Both factors are measured rather than derived: `proposedCols` is how many
 * columns the current font gets out of the frame, and `screenHeight` is what
 * the rows drawn in it cost, neither of which follows from the font size alone.
 * The ratio to the grid we want is the factor the font is off by — whichever of
 * width and height runs out first — and a pass or two settles it, since the
 * size is quantised and the first answer is rarely exact.
 *
 * Rounded *down* to the nearest half pixel: rounding up would overflow the
 * frame the size was measured against, and whole pixels alone would leave a
 * visible margin at the small sizes a wide pane needs.
 */
export function nextFontSize({
  current,
  proposedCols,
  gridCols,
  screenHeight,
  heightBudget,
  ceiling,
}: {
  current: number
  proposedCols: number
  /** Columns the emulator is actually drawing — the pane's own grid. */
  gridCols: number
  screenHeight: number
  heightBudget: number
  ceiling: number
}): number {
  const scale = Math.min(
    proposedCols / gridCols,
    screenHeight > 0 ? heightBudget / screenHeight : Number.POSITIVE_INFINITY,
  )
  return clamp(Math.floor(current * scale * 2) / 2, MIN_FONT_SIZE, ceiling)
}

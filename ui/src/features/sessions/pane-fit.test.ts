import { expect, it } from "vitest"

import { paneFit, sameSize } from "./pane-fit"

/** A frame that measures cleanly: 12px cells, 24 of them drawn, 480px to fill. */
const MEASURED = { cols: 137, rows: 24, screenHeight: 24 * 20, heightBudget: 480 }

it("fills the frame with whole cells", () => {
  // 480 / 20 = 24 rows exactly, and the columns are the addon's to propose.
  expect(paneFit(MEASURED)).toEqual({ cols: 137, rows: 24 })
})

it("never asks for a partial row", () => {
  // 449 / 20 is 22.45 rows: the pane gets the 22 that fit, not the one it
  // would have to draw half of.
  expect(paneFit({ ...MEASURED, heightBudget: 449 })).toEqual({ cols: 137, rows: 22 })
})

it("stays inside the sizes the daemon accepts", () => {
  const huge = paneFit({ cols: 4_000, rows: 24, screenHeight: 24, heightBudget: 100_000 })
  expect(huge).toEqual({ cols: 500, rows: 500 })
})

it("keeps a cramped frame at a size a terminal can still be", () => {
  const tiny = paneFit({ cols: 3, rows: 24, screenHeight: 24 * 20, heightBudget: 20 })
  expect(tiny).toEqual({ cols: 20, rows: 5 })
})

/**
 * Every one of these is a frame that has not been laid out yet — a font whose
 * cells are unmeasured, a screen with no height, a frame with no room. None of
 * them is a grid, and asking a pane for one would resize it to nonsense.
 */
it("says nothing about a frame that cannot be measured", () => {
  expect(paneFit({ ...MEASURED, cols: undefined })).toBeNull()
  expect(paneFit({ ...MEASURED, cols: 0 })).toBeNull()
  expect(paneFit({ ...MEASURED, cols: Number.NaN })).toBeNull()
  expect(paneFit({ ...MEASURED, screenHeight: 0 })).toBeNull()
  expect(paneFit({ ...MEASURED, rows: 0 })).toBeNull()
  expect(paneFit({ ...MEASURED, heightBudget: 0 })).toBeNull()
  expect(paneFit({ ...MEASURED, heightBudget: Number.NaN })).toBeNull()
})

it("tells one grid from another, and neither from nothing", () => {
  expect(sameSize({ cols: 80, rows: 24 }, { cols: 80, rows: 24 })).toBe(true)
  expect(sameSize({ cols: 80, rows: 24 }, { cols: 80, rows: 25 })).toBe(false)
  expect(sameSize(null, null)).toBe(false)
  expect(sameSize({ cols: 80, rows: 24 }, null)).toBe(false)
})

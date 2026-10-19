// A deep import, straight at the module the patch below touches: the
// package's own barrel (`@perses-dev/components`) pulls in its MUI theme,
// which `require()`s raw `.css` files that only Vite's own build (never a
// plain Vitest/Node module load) knows how to read.
import { formatValue } from "@perses-dev/components/dist/model/units"
import { describe, expect, it } from "vitest"

/**
 * Every panel kind (stat, gauge, pie, bar, table, time series) formats its
 * numbers through this one function, so this is the one seam that keeps a
 * NaN query result (036, an empty range over a 0/0 division or an
 * empty-histogram quantile) from reaching the screen as the literal string
 * "NaN" rather than Perses's own empty state. `patches/` carries the fix,
 * since the function itself ships in `@perses-dev/components`.
 */
describe("formatValue", () => {
  it("shows No data for a NaN value, formatted or not", () => {
    expect(formatValue(Number.NaN, { unit: "decimal" })).toBe("No data")
    expect(formatValue(Number.NaN, { unit: "percent-decimal", decimalPlaces: 0 })).toBe("No data")
    expect(formatValue(Number.NaN)).toBe("No data")
  })

  it("still formats a real value", () => {
    expect(formatValue(0.5, { unit: "percent-decimal", decimalPlaces: 0 })).toBe("50%")
  })
})

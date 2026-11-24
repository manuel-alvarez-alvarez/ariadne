/**
 * The dashboard's look is the app's look (spec 036). The expected values come
 * from `ui/src/index.css`, the app's own source of truth, converted here from
 * `oklch()` with a second, test-only converter — not from `theme.ts`.
 */

/// <reference types="node" />

import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { generateChartsTheme, getTheme } from "@perses-dev/components"
import { describe, expect, it } from "vitest"
import { buildChartsTheme, buildMuiTheme } from "./theme"
import { FONT_FAMILY, TOKENS } from "./theme-tokens"

type ModeTokens = typeof TOKENS.light

function read(relative: string): string {
  return readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8")
}

const css = read("../../src/index.css")

function block(selector: string): string {
  const start = css.indexOf(`\n${selector} {`)
  if (start < 0) throw new Error(`${selector} is missing from index.css`)
  return css.slice(start, css.indexOf("\n}", start))
}

function declared(body: string, name: string): string {
  const match = new RegExp(`^\\s*--${name}:\\s*([^;]+);`, "m").exec(body)
  if (!match) throw new Error(`--${name} is missing`)
  const value = (match[1] ?? "").trim()
  const ref = /^var\(--([\w-]+)\)$/.exec(value)
  return ref?.[1] ? declared(body, ref[1]) : value
}

type Rgba = [number, number, number, number]

function linearToOklab(L: number, C: number, h: number): number[] {
  const a = C * Math.cos((h * Math.PI) / 180)
  const b = C * Math.sin((h * Math.PI) / 180)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const inGamut = (v: number[]) => v.every((x) => x >= -1e-6 && x <= 1 + 1e-6)

function fromOklch(value: string): Rgba {
  const m = /^oklch\(([\d.]+) ([\d.]+) ([\d.]+)(?: \/ ([\d.]+)%)?\)$/.exec(value)
  if (!m) throw new Error(`not an oklch value: ${value}`)
  const [L, C, h] = [Number(m[1]), Number(m[2]), Number(m[3])] as [number, number, number]
  let lin = linearToOklab(L, C, h)
  if (!inGamut(lin)) {
    // CSS maps an out-of-gamut colour by lowering its chroma.
    let lo = 0
    let hi = C
    for (let i = 0; i < 30; i++) {
      const mid = (lo + hi) / 2
      if (inGamut(linearToOklab(L, mid, h))) lo = mid
      else hi = mid
    }
    lin = linearToOklab(L, lo, h)
  }
  const [r, g, b] = lin.map((x) => {
    const c = Math.min(1, Math.max(0, x))
    return Math.round((c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055) * 255)
  }) as [number, number, number]
  return [r, g, b, m[4] === undefined ? 1 : Number(m[4]) / 100]
}

function fromSrgb(value: string): Rgba {
  const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(value)
  if (hex)
    return [parseInt(hex[1] ?? "", 16), parseInt(hex[2] ?? "", 16), parseInt(hex[3] ?? "", 16), 1]
  const rgba = /^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/.exec(value)
  if (rgba) return [Number(rgba[1]), Number(rgba[2]), Number(rgba[3]), Number(rgba[4])]
  throw new Error(`not an sRGB value: ${value}`)
}

const NAMES: Record<keyof ModeTokens, string> = {
  background: "background",
  card: "card",
  popover: "popover",
  foreground: "foreground",
  muted: "muted",
  mutedForeground: "muted-foreground",
  border: "border",
  input: "input",
  primary: "primary",
  primaryForeground: "primary-foreground",
  destructive: "destructive",
}

describe("theme tokens", () => {
  for (const [mode, selector] of [
    ["light", ":root"],
    ["dark", ".dark"],
  ] as const) {
    it(`match the app's ${mode} tokens in ui/src/index.css`, () => {
      const body = block(selector)
      for (const [key, name] of Object.entries(NAMES)) {
        const want = fromOklch(declared(body, name))
        const got = fromSrgb(TOKENS[mode][key as keyof ModeTokens])
        expect(got.slice(0, 3), `${mode} ${name}`).toEqual(want.slice(0, 3))
        expect(got[3], `${mode} ${name} alpha`).toBeCloseTo(want[3], 5)
      }
    })
  }

  it("match the app's radius", () => {
    expect(TOKENS.radius).toBe(declared(block(":root"), "radius"))
  })

  it("keep the dark border and input alpha", () => {
    expect(fromSrgb(TOKENS.dark.border)[3]).toBe(0.1)
    expect(fromSrgb(TOKENS.dark.input)[3]).toBe(0.15)
  })
})

describe("MUI theme", () => {
  for (const mode of ["light", "dark"] as const) {
    it(`uses the ${mode} surfaces, text, borders, primary and radius`, () => {
      const t = TOKENS[mode]
      const theme = buildMuiTheme(mode)
      expect(theme.palette.mode).toBe(mode)
      expect(theme.palette.background.default).toBe(t.background)
      expect(theme.palette.background.paper).toBe(t.card)
      expect(theme.palette.text.primary).toBe(t.foreground)
      expect(theme.palette.text.secondary).toBe(t.mutedForeground)
      expect(theme.palette.divider).toBe(t.border)
      expect(theme.palette.primary.main).toBe(t.primary)
      expect(theme.palette.primary.contrastText).toBe(t.primaryForeground)
      expect(theme.palette.error.main).toBe(t.destructive)
      expect(theme.shape.borderRadius).toBe(8)
    })
  }

  it("styles panels, menus, dialogs and buttons from the tokens", () => {
    const t = TOKENS.dark
    const theme = buildMuiTheme("dark")
    const c = theme.components
    expect(c?.MuiPaper?.styleOverrides?.root).toMatchObject({ backgroundColor: t.card })
    for (const name of ["MuiPopover", "MuiMenu", "MuiDialog", "MuiDrawer"] as const) {
      expect(c?.[name]?.styleOverrides?.paper, name).toMatchObject({
        backgroundColor: t.popover,
        borderRadius: 8,
      })
    }
    expect(c?.MuiButton?.styleOverrides?.root).toMatchObject({ borderRadius: 8 })
    expect(c?.MuiOutlinedInput?.styleOverrides?.notchedOutline).toMatchObject({
      borderColor: t.input,
    })
  })

  it("extends Perses's theme: its alert override and small fields survive", () => {
    const base = getTheme("light", {}, true)
    const theme = buildMuiTheme("light")
    expect(theme.components?.MuiAlert).toEqual(base.components?.MuiAlert)
    expect(theme.components?.MuiTextField?.defaultProps).toEqual({ size: "small" })
  })

  it("sets the Geist font on the root and on the variants that carry a family", () => {
    const theme = buildMuiTheme("light")
    expect(theme.typography.fontFamily).toBe(FONT_FAMILY)
    for (const variant of [
      "h1",
      "h4",
      "subtitle1",
      "body1",
      "body2",
      "button",
      "caption",
    ] as const) {
      expect(theme.typography[variant].fontFamily, variant).toBe(FONT_FAMILY)
    }
  })
})

describe("Geist font", () => {
  it("comes from the package's own bundle, imported at the entry", () => {
    expect(read("./main.tsx")).toContain('import "@fontsource-variable/geist"')
    expect(read("../node_modules/@fontsource-variable/geist/index.css")).toContain(
      "font-family: 'Geist Variable'",
    )
    expect(FONT_FAMILY.startsWith('"Geist Variable"')).toBe(true)
  })
})

interface Axis {
  axisLabel?: { color?: string }
  axisLine?: { lineStyle?: { color?: string } }
  splitLine?: { lineStyle?: { color?: string; opacity?: number } }
}

interface Echarts {
  textStyle?: { color?: string }
  timeAxis?: Axis
  categoryAxis?: Axis
  valueAxis?: Axis
  tooltip?: { backgroundColor?: string; borderColor?: string; textStyle?: { color?: string } }
  legend?: { textStyle?: { color?: string } }
}

function echarts(mode: "light" | "dark"): Echarts {
  return buildChartsTheme(buildMuiTheme(mode)).echartsTheme as unknown as Echarts
}

describe("charts theme", () => {
  for (const mode of ["light", "dark"] as const) {
    it(`draws ${mode} axes, grid lines, tooltips and legends from the tokens`, () => {
      const t = TOKENS[mode]
      const e = echarts(mode)
      expect(e.textStyle?.color).toBe(t.foreground)
      for (const axis of [e.timeAxis, e.categoryAxis, e.valueAxis] as const) {
        expect(axis?.axisLabel?.color).toBe(t.mutedForeground)
        expect(axis?.splitLine?.lineStyle?.color).toBe(t.border)
        expect(axis?.splitLine?.lineStyle?.opacity).toBe(1)
      }
      expect(e.timeAxis?.axisLine?.lineStyle?.color).toBe(t.border)
      expect(e.tooltip?.backgroundColor).toBe(t.popover)
      expect(e.tooltip?.borderColor).toBe(t.border)
      expect(e.tooltip?.textStyle?.color).toBe(t.foreground)
      expect(e.legend?.textStyle?.color).toBe(t.foreground)
    })
  }

  it("keeps the default Perses series palette", () => {
    const base = getTheme("light", {}, true)
    const want = generateChartsTheme(base, {}).echartsTheme.color
    expect(want).toHaveLength(7)
    expect(buildChartsTheme(buildMuiTheme("light")).echartsTheme.color).toEqual(want)
    expect(buildChartsTheme(buildMuiTheme("dark")).echartsTheme.color).toEqual(want)
  })
})

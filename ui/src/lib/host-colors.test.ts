/** The host colour tokens of `index.css` stay readable in both themes. */

import { readFileSync } from "node:fs"
import { expect, it } from "vitest"

import { HOST_COLORS } from "./host-colors"

const CSS = readFileSync(new URL("../index.css", import.meta.url), "utf8")

type Rgb = [number, number, number]

/** The declarations of the `:root` or `.dark` block, by token name. */
function tokens(block: ":root" | ".dark"): Map<string, string> {
  const start = CSS.indexOf(`\n${block} {`)
  const body = CSS.slice(start, CSS.indexOf("\n}\n", start))
  return new Map(
    [...body.matchAll(/^\s*(--[\w-]+):\s*(.+);$/gm)].map((m) => [m[1] ?? "", m[2] ?? ""] as const),
  )
}

/** `oklch(L C H [/ A])` to gamma-encoded sRGB and alpha (Ottosson's matrices). */
function oklch(value: string): { rgb: Rgb; alpha: number } {
  const m = value.match(/oklch\(([\d.]+) ([\d.]+) ([\d.]+)(?: \/ ([\d.]+))?\)/)
  if (!m) throw new Error(`not an oklch colour: ${value}`)
  const [l, c, h] = [Number(m[1]), Number(m[2]), (Number(m[3]) * Math.PI) / 180]
  const a = c * Math.cos(h)
  const b = c * Math.sin(h)
  const l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3
  const linear: Rgb = [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
  ]
  const encode = (x: number) => {
    const v = Math.min(1, Math.max(0, x))
    return v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055
  }
  return { rgb: linear.map(encode) as Rgb, alpha: Number(m[4] ?? 1) }
}

/** WCAG relative luminance of an encoded sRGB colour. */
const luminance = (rgb: Rgb) => {
  const [r, g, b] = rgb.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0)
}
const ratio = (x: Rgb, y: Rgb) => {
  const [p, q] = [luminance(x), luminance(y)]
  return (Math.max(p, q) + 0.05) / (Math.min(p, q) + 0.05)
}
/** What a browser paints: alpha blended in encoded sRGB, as CSS does. */
const over = (top: { rgb: Rgb; alpha: number }, under: Rgb): Rgb =>
  top.rgb.map((x, i) => x * top.alpha + (under[i] as number) * (1 - top.alpha)) as Rgb

for (const [theme, block] of [
  ["light", ":root"],
  ["dark", ".dark"],
] as const) {
  it(`reads each host pill at WCAG AA, and each dot against the page, in the ${theme} theme`, () => {
    const set = tokens(block)
    const get = (name: string) => oklch(set.get(name) ?? "")
    const page = get("--background").rgb
    for (const name of [...HOST_COLORS.map((color) => color.value), "grey"]) {
      const soft = over(get(`--host-${name}-soft`), page)
      expect(ratio(get(`--host-${name}-fg`).rgb, soft), `${name} text`).toBeGreaterThanOrEqual(4.5)
      expect(ratio(get(`--host-${name}`).rgb, page), `${name} dot`).toBeGreaterThanOrEqual(2)
    }
  })
}

it("declares every host token in both themes and in the Tailwind theme", () => {
  for (const name of [...HOST_COLORS.map((color) => color.value), "grey"]) {
    for (const suffix of ["", "-fg", "-soft"]) {
      expect(tokens(":root").has(`--host-${name}${suffix}`)).toBe(true)
      expect(tokens(".dark").has(`--host-${name}${suffix}`)).toBe(true)
      expect(CSS).toContain(`--color-host-${name}${suffix}: var(--host-${name}${suffix})`)
    }
  }
})

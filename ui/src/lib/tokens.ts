/**
 * The app's tokens in `index.css`, read off the document for a surface that
 * cannot read a CSS variable where it draws: the session console (xterm.js).
 * It parses hex and not the `oklch()` the tokens are written in, so a colour
 * is converted here — a short, exact conversion rather than a dependency.
 */

/**
 * A token's computed value, in hex where it is a colour, or `undefined` where
 * the document has none — the test runner loads no stylesheet — so a caller
 * falls back to its own default rather than being handed an empty string.
 */
export function token(name: string): string | undefined {
  if (typeof document === "undefined") return undefined
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  if (value.length === 0) return undefined
  return oklchToHex(value) ?? value
}

export function withAlpha(hex: string | undefined, alpha: number): string | undefined {
  if (hex === undefined || !/^#[0-9a-f]{6}$/i.test(hex)) return undefined
  return `${hex}${channel(alpha)}`
}

const OKLCH =
  /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*([\d.]+)(%?)\s*)?\)$/i

/**
 * `oklch(L C h [/ a])` as `#rrggbb` or `#rrggbbaa`, or `undefined` for a value
 * that is not one. The matrices are the CSS Color 4 reference ones; the
 * result is clamped into sRGB, which is where every token already sits.
 */
function oklchToHex(value: string): string | undefined {
  const match = OKLCH.exec(value)
  if (!match) return undefined
  const [, lightness, lightnessUnit, chroma, hue, alpha, alphaUnit] = match
  const l = Number(lightness) / (lightnessUnit === "%" ? 100 : 1)
  const c = Number(chroma)
  const h = (Number(hue) * Math.PI) / 180

  const a = c * Math.cos(h)
  const b = c * Math.sin(h)

  const l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3

  const red = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_
  const green = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_
  const blue = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_

  const rgb = `#${channel(gamma(red))}${channel(gamma(green))}${channel(gamma(blue))}`
  if (alpha === undefined) return rgb
  return `${rgb}${channel(Number(alpha) / (alphaUnit === "%" ? 100 : 1))}`
}

/** Linear sRGB to the sRGB transfer curve, clamped to the gamut. */
function gamma(linear: number): number {
  const clamped = Math.min(1, Math.max(0, linear))
  return clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * clamped ** (1 / 2.4) - 0.055
}

/** A 0–1 channel as two hex digits. */
function channel(value: number): string {
  return Math.round(Math.min(1, Math.max(0, value)) * 255)
    .toString(16)
    .padStart(2, "0")
}

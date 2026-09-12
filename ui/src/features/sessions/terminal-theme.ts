/**
 * The terminal's colours and font, taken from the tokens the rest of the app
 * is drawn in (`index.css`).
 *
 * xterm.js paints on a canvas-like grid of its own, so it cannot read a CSS
 * variable where it is used: the theme is a set of colour values, read off
 * the document when the pane opens and again when the app's theme changes.
 * The tokens are `oklch()`, which xterm.js parses only through a canvas the
 * test runner does not have, so they are converted to hex here — a short,
 * exact conversion rather than a dependency.
 *
 * The console draws with the six named colours a terminal has — red, green,
 * yellow, blue, magenta, cyan — and each maps onto the step of the status
 * ramp that means the same thing, as the app's own badges do: the `-fg` step
 * for text, which is what a colour in a console is, and the solid for the
 * bright variant. Dim is xterm's own, and needs no token.
 */

import type { ITheme } from "@xterm/xterm"

/** The font size of the pane, in pixels: the app's `text-xs`. */
export const TERMINAL_FONT_SIZE = 12

/** The mono face the app uses, as xterm.js takes it. */
export function terminalFontFamily(): string {
  return token("--font-mono") ?? "ui-monospace, monospace"
}

/** The theme for the mode the app is in, read off the document's tokens. */
export function terminalTheme(dark: boolean): ITheme {
  const background = token("--background")
  const foreground = token("--foreground")
  const muted = token("--muted-foreground")
  return {
    background,
    foreground,
    cursor: foreground,
    cursorAccent: background,
    selectionBackground: withAlpha(token("--primary"), 0.3),
    black: dark ? token("--secondary") : foreground,
    brightBlack: muted,
    white: dark ? foreground : muted,
    brightWhite: foreground,
    red: token("--status-danger-fg"),
    brightRed: token("--status-danger"),
    green: token("--status-done-fg"),
    brightGreen: token("--status-done"),
    yellow: token("--status-warn-fg"),
    brightYellow: token("--status-warn"),
    blue: token("--status-active-fg"),
    brightBlue: token("--status-active"),
    magenta: token("--status-review-fg"),
    brightMagenta: token("--status-review"),
    cyan: token("--status-ready-fg"),
    brightCyan: token("--status-ready"),
  }
}

/**
 * A token's computed value, as xterm.js can take it, or `undefined` where the
 * document has none — the test runner loads no stylesheet — so xterm.js
 * falls back to its own default rather than being handed an empty string.
 */
function token(name: string): string | undefined {
  if (typeof document === "undefined") return undefined
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  if (value.length === 0) return undefined
  return oklchToHex(value) ?? value
}

function withAlpha(hex: string | undefined, alpha: number): string | undefined {
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

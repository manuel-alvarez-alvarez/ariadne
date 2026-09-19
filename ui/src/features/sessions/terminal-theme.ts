/**
 * The terminal's colours and font, taken from the tokens the rest of the app
 * is drawn in (`index.css`).
 *
 * xterm.js paints on a canvas-like grid of its own, so it cannot read a CSS
 * variable where it is used: the theme is a set of colour values, read off
 * the document when the pane opens and again when the app's theme changes.
 * The tokens are `oklch()`, which xterm.js parses only through a canvas the
 * test runner does not have, so they are converted to hex
 * (`@/lib/tokens.ts`, which the knowledge graph reads its colours through too).
 *
 * The console draws with the six named colours a terminal has — red, green,
 * yellow, blue, magenta, cyan — and each maps onto the step of the status
 * ramp that means the same thing, as the app's own badges do: the `-fg` step
 * for text, which is what a colour in a console is, and the solid for the
 * bright variant. Dim is xterm's own, and needs no token.
 */

import type { ITheme } from "@xterm/xterm"

import { token, withAlpha } from "@/lib/tokens"

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

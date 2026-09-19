/**
 * The colours a knowledge graph is drawn in, read off the app's tokens in
 * `index.css` for the theme on screen.
 *
 * sigma.js draws on WebGL and a canvas, so it cannot read a CSS variable
 * where it is used, and it parses hex and `rgb()` but not the `oklch()` the
 * tokens are written in: the palette is read off the document, converted, and
 * read again when the theme changes — the way the session console reads its
 * own (`@/features/sessions/terminal-theme.ts`).
 */

import { token, withAlpha } from "@/lib/tokens"

import type { GraphTone } from "./graph-model"

export interface GraphPalette {
  /** Each tone at its solid step: `--status-<tone>`. */
  tones: Record<GraphTone, string>
  /** Labels, and the counts on edges. */
  label: string
  /** Behind a hovered node's label. */
  background: string
  /** A node or an edge away from the hovered one. */
  faded: string
  font: string
}

const TONES: readonly GraphTone[] = [
  "pending",
  "ready",
  "active",
  "review",
  "approved",
  "done",
  "warn",
  "danger",
]

/** What a token the document does not have falls back to: the test runner loads no stylesheet. */
const FALLBACK = "#888888"

export function graphPalette(): GraphPalette {
  const muted = token("--muted-foreground") ?? FALLBACK
  return {
    tones: Object.fromEntries(
      TONES.map((tone) => [tone, token(`--status-${tone}`) ?? FALLBACK]),
    ) as Record<GraphTone, string>,
    label: token("--foreground") ?? "#000000",
    background: token("--popover") ?? "#ffffff",
    faded: withAlpha(muted, 0.25) ?? muted,
    font: token("--font-sans") ?? "sans-serif",
  }
}

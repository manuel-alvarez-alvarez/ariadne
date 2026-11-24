/**
 * The Ariadne app's design tokens, as sRGB.
 *
 * The values are those of `ui/src/index.css` (`:root` and `.dark`), because MUI
 * and ECharts both read sRGB and not `oklch()`. `theme.test.ts` fails when a
 * value here drifts from that file. The dark border and input tokens keep the
 * alpha the app gives them.
 */

interface ModeTokens {
  background: string
  card: string
  popover: string
  foreground: string
  muted: string
  mutedForeground: string
  border: string
  input: string
  primary: string
  primaryForeground: string
  destructive: string
}

interface Tokens {
  light: ModeTokens
  dark: ModeTokens
  /** The app's `--radius`, in rem, as it is written in `ui/src/index.css`. */
  radius: string
}

export const TOKENS: Tokens = {
  light: {
    background: "#ffffff",
    card: "#ffffff",
    popover: "#ffffff",
    foreground: "#090a0e",
    muted: "#f2f5fb",
    mutedForeground: "#717378",
    border: "#e2e5eb",
    input: "#e2e5eb",
    primary: "#3959a6",
    primaryForeground: "#fafafa",
    destructive: "#e40016",
  },
  dark: {
    background: "#090a0e",
    card: "#15171b",
    popover: "#15171b",
    foreground: "#f8faff",
    muted: "#24262a",
    mutedForeground: "#9ea1a6",
    border: "rgba(255, 255, 255, 0.1)",
    input: "rgba(255, 255, 255, 0.15)",
    primary: "#80a3f0",
    primaryForeground: "#171717",
    destructive: "#ff6568",
  },
  radius: "0.5rem",
}

/** The font the app ships; `main.tsx` bundles its files, so no request leaves the page. */
export const FONT_FAMILY = '"Geist Variable", sans-serif'

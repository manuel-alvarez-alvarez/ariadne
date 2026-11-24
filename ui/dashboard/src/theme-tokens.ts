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
  /**
   * Each token as `ui/src/index.css` writes it (`--brand` resolved), so the
   * drift test also catches a change too small to move a rounded sRGB value.
   */
  source: { light: ModeTokens; dark: ModeTokens }
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
  source: {
    light: {
      background: "oklch(1 0.008 265)",
      card: "oklch(1 0.008 265)",
      popover: "oklch(1 0.008 265)",
      foreground: "oklch(0.145 0.008 265)",
      muted: "oklch(0.97 0.008 265)",
      mutedForeground: "oklch(0.556 0.008 265)",
      border: "oklch(0.922 0.008 265)",
      input: "oklch(0.922 0.008 265)",
      primary: "oklch(0.48 0.13 265)",
      primaryForeground: "oklch(0.985 0 0)",
      destructive: "oklch(0.577 0.245 27.325)",
    },
    dark: {
      background: "oklch(0.145 0.008 265)",
      card: "oklch(0.205 0.008 265)",
      popover: "oklch(0.205 0.008 265)",
      foreground: "oklch(0.985 0.008 265)",
      muted: "oklch(0.269 0.008 265)",
      mutedForeground: "oklch(0.708 0.008 265)",
      border: "oklch(1 0.008 265 / 10%)",
      input: "oklch(1 0.008 265 / 15%)",
      primary: "oklch(0.72 0.12 265)",
      primaryForeground: "oklch(0.205 0 0)",
      destructive: "oklch(0.704 0.191 22.216)",
    },
  },
}

/** The font the app ships; `main.tsx` bundles its files, so no request leaves the page. */
export const FONT_FAMILY = '"Geist Variable", sans-serif'

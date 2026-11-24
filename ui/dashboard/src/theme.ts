/**
 * The Ariadne app look, for the Perses dashboard: the tokens of
 * `theme-tokens.ts`, applied to Perses's own themes.
 *
 * `buildMuiTheme` extends Perses's own `getTheme` — it does not replace it —
 * and `buildChartsTheme` extends `generateChartsTheme`. The chart series
 * colours (`echartsTheme.color`) are left to Perses's default palette.
 */

import { createTheme, type Theme } from "@mui/material/styles"
import { generateChartsTheme, getTheme, type PersesChartsTheme } from "@perses-dev/components"

import { FONT_FAMILY, TOKENS } from "./theme-tokens"

type ThemeMode = "light" | "dark"

const REM_PX = 16

function radiusPx(): number {
  return Number.parseFloat(TOKENS.radius) * REM_PX
}

export function buildMuiTheme(mode: ThemeMode): Theme {
  const t = TOKENS[mode]
  const radius = radiusPx()
  const base = getTheme(mode, {}, true)

  const surface = (color: string) => ({
    backgroundColor: color,
    backgroundImage: "none",
    color: t.foreground,
  })
  const popover = { paper: { ...surface(t.popover), borderRadius: radius } }

  // MUI keeps each variant's own `fontFamily` once the base theme is built,
  // so changing the root family alone would leave every variant on Perses's.
  const variants: Record<string, { fontFamily: string }> = {}
  for (const [key, value] of Object.entries(base.typography)) {
    if (value && typeof value === "object" && "fontFamily" in value) {
      variants[key] = { fontFamily: FONT_FAMILY }
    }
  }

  return createTheme(base, {
    palette: {
      mode,
      primary: { main: t.primary, contrastText: t.primaryForeground },
      error: { main: t.destructive },
      background: { default: t.background, paper: t.card },
      text: { primary: t.foreground, secondary: t.mutedForeground },
      divider: t.border,
    },
    shape: { borderRadius: radius },
    typography: { fontFamily: FONT_FAMILY, ...variants },
    components: {
      MuiCssBaseline: { styleOverrides: { body: { colorScheme: mode } } },
      MuiPaper: { styleOverrides: { root: surface(t.card) } },
      MuiDialog: { styleOverrides: popover },
      MuiDrawer: { styleOverrides: popover },
      MuiPopover: { styleOverrides: popover },
      MuiMenu: { styleOverrides: popover },
      MuiTooltip: {
        styleOverrides: {
          tooltip: {
            backgroundColor: t.popover,
            color: t.foreground,
            border: `1px solid ${t.border}`,
          },
        },
      },
      MuiButton: {
        styleOverrides: { root: { borderRadius: radius, textTransform: "none" } },
      },
      MuiOutlinedInput: {
        styleOverrides: {
          root: { borderRadius: radius },
          notchedOutline: { borderColor: t.input },
        },
      },
    },
  })
}

export function buildChartsTheme(muiTheme: Theme): PersesChartsTheme {
  const t = TOKENS[muiTheme.palette.mode]
  const axis = {
    axisLabel: { color: t.mutedForeground },
    axisTick: { lineStyle: { color: t.border } },
    axisLine: { lineStyle: { color: t.border } },
    splitLine: { lineStyle: { color: t.border, opacity: 1 } },
  }
  return generateChartsTheme(muiTheme, {
    echartsTheme: {
      textStyle: { color: t.foreground },
      categoryAxis: axis,
      timeAxis: axis,
      valueAxis: { axisLabel: axis.axisLabel, splitLine: axis.splitLine },
      legend: {
        textStyle: { color: t.foreground },
        pageTextStyle: { color: t.mutedForeground },
      },
      toolbox: { iconStyle: { borderColor: t.mutedForeground } },
      tooltip: {
        backgroundColor: t.popover,
        borderColor: t.border,
        textStyle: { color: t.foreground },
      },
      axisPointer: { lineStyle: { color: t.mutedForeground } },
      markLine: { itemStyle: { color: t.mutedForeground } },
    },
    noDataOption: { title: { textStyle: { color: t.mutedForeground } } },
  })
}

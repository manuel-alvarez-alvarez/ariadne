/**
 * What a fixed colour looks like in the app's source and in a drawn class:
 * Tailwind's own palette, which does not follow the theme, and a colour
 * written out. Tests that prove a screen uses the app's tokens alone match
 * them.
 */

/** A Tailwind palette class: a colour that does not change with the theme. */
export const PALETTE_CLASS =
  /\b(?:bg|text|border|ring|fill|stroke|outline|divide|from|via|to|shadow|decoration|accent|caret)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white)(?:-\d+)?\b/g

/** A colour written out: a hex value, or a colour function. */
export const COLOUR_LITERAL = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\(/g

/** Each fixed colour in `text`. */
export function fixedColours(text: string): string[] {
  return [...(text.match(PALETTE_CLASS) ?? []), ...(text.match(COLOUR_LITERAL) ?? [])]
}

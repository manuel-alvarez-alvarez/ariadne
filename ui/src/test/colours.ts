/**
 * What a fixed colour looks like in the app's source and in a drawn class:
 * Tailwind's own palette, which does not follow the theme, and a colour
 * written out. Tests that prove a screen uses the app's tokens alone match
 * them.
 *
 * A palette name after a `.` is a class selector, not a colour: an ancestor
 * that recolours a shared part, as `[&_.text-purple-500]:text-question-fg`
 * does, names the class it replaces.
 */

/** A Tailwind palette class: a colour that does not change with the theme. */
const PALETTE_CLASS =
  /(?<![.\w-])(?:bg|text|border|ring|fill|stroke|outline|divide|from|via|to|shadow|decoration|accent|caret)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white)(?:-\d+)?\b/g

/** A colour written out: a hex value, or a colour function. */
const COLOUR_LITERAL = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\(/g

/** Each fixed colour in `text`. */
export function fixedColours(text: string): string[] {
  return [...(text.match(PALETTE_CLASS) ?? []), ...(text.match(COLOUR_LITERAL) ?? [])]
}

/**
 * Each fixed colour drawn under `root`, in a class or an inline style. A
 * palette class that an ancestor recolours with a token class does not
 * count: its element shows the token.
 */
export function drawnColours(root: Element): string[] {
  return Array.from(root.querySelectorAll("[class], [style]")).flatMap((element) => {
    const classes = fixedColours(element.getAttribute("class") ?? "").filter(
      (name) => !recoloured(element, name),
    )
    return [...classes, ...fixedColours(element.getAttribute("style") ?? "")]
  })
}

/** Whether an ancestor of `element` gives its class `name` a token colour. */
function recoloured(element: Element, name: string): boolean {
  const override = `[&_.${name}]:`
  for (let at = element.parentElement; at; at = at.parentElement) {
    for (const one of (at.getAttribute("class") ?? "").split(/\s+/)) {
      if (one.startsWith(override) && fixedColours(one.slice(override.length)).length === 0)
        return true
    }
  }
  return false
}

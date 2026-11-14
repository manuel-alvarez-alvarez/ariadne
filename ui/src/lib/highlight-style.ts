/**
 * The one colour scheme of highlighted code: the light and dark highlight
 * styles the task diff viewer and the console's code blocks both use.
 */

import { defaultHighlightStyle, HighlightStyle } from "@codemirror/language"
import { oneDarkHighlightStyle } from "@codemirror/theme-one-dark"
import { tags } from "@lezer/highlight"

/**
 * CodeMirror's light style leaves property and attribute names the colour of
 * the text, which one-dark does not; this gives them one colour beside it.
 */
const lightNames = HighlightStyle.define([
  { tag: [tags.propertyName, tags.attributeName], color: "#00c" },
])

/** The highlight styles of each theme, applied together. */
export const HIGHLIGHT_STYLES: Record<"light" | "dark", readonly HighlightStyle[]> = {
  light: [lightNames, defaultHighlightStyle],
  dark: [oneDarkHighlightStyle],
}

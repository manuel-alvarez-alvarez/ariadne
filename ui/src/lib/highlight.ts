/**
 * The syntax highlighter for the code the console and the Markdown renderer
 * show: every language `@codemirror/language-data` knows, parsed by its own
 * CodeMirror grammar and coloured by the highlight styles it is given.
 * A language it does not know is plain text.
 *
 * A grammar is a chunk of its own, loaded the first time a block of its
 * language is shown; until then the block is plain text.
 */

import {
  type HighlightStyle,
  LanguageDescription,
  type LanguageSupport,
} from "@codemirror/language"
import { languages } from "@codemirror/language-data"
import { highlightTree } from "@lezer/highlight"
import { StyleModule } from "style-mod"

/** Fence names agents write that the data has under another name. */
const ALIASES = new Map([
  ["console", "shell"],
  ["golang", "go"],
  ["jsonc", "json"],
])

/** Fence names that mean plain text, which the extension match would misread. */
const PLAIN = new Set(["text", "txt", "plain", "plaintext"])

function describe(name: string): LanguageDescription | undefined {
  const lower = name.trim().toLowerCase()
  if (lower === "" || PLAIN.has(lower)) return undefined
  const named = ALIASES.get(lower) ?? lower
  return (
    LanguageDescription.matchLanguageName(languages, named, false) ??
    LanguageDescription.matchFilename(languages, `file.${named}`) ??
    undefined
  )
}

/** The language a fence or a file extension names, or undefined where it is not known. */
export function knownLanguage(name: string | undefined): string | undefined {
  return name === undefined ? undefined : describe(name)?.name.toLowerCase()
}

/** The label shown for a language `knownLanguage` returned. */
export function languageLabel(language: string): string {
  return describe(language)?.name ?? language
}

/** The language of a file path by its extension. */
export function languageForPath(path: string): string | undefined {
  const extension = /\.([^./\\]+)$/.exec(path.trim())?.[1]
  return knownLanguage(extension)
}

/** The grammar of a language `knownLanguage` returned, if it has loaded. */
export function loadedGrammar(language: string): LanguageSupport | undefined {
  return describe(language)?.support
}

/** Load the grammar of a language `knownLanguage` returned. */
export function loadGrammar(language: string): Promise<LanguageSupport> | undefined {
  return describe(language)?.load()
}

/** One run of text, with the highlight style's class where it is a token. */
type Token = { text: string; className?: string }

const mounted = new WeakSet<HighlightStyle>()

/**
 * Split `code` into tokens by `grammar`, classed by `styles`; the tokens
 * joined are always `code`. A style's rules are put in the document the first
 * time it is used, as an editor view would.
 */
export function highlight(
  code: string,
  grammar: LanguageSupport,
  styles: readonly HighlightStyle[],
): Token[] {
  if (typeof document !== "undefined") {
    for (const style of styles) {
      if (mounted.has(style)) continue
      if (style.module) StyleModule.mount(document, style.module)
      mounted.add(style)
    }
  }
  const tokens: Token[] = []
  let at = 0
  const push = (text: string, className?: string) => {
    if (text === "") return
    const last = tokens.at(-1)
    if (last && last.className === className) last.text += text
    else tokens.push(className ? { text, className } : { text })
  }
  highlightTree(grammar.language.parser.parse(code), styles, (from, to, className) => {
    // A grammar that styles a token line by line, as a heredoc is, leaves
    // the line break between two runs of one token unstyled; it joins them.
    const last = tokens.at(-1)
    const gap = code.slice(at, from)
    if (last?.className === className && /^\n+$/.test(gap)) last.text += gap
    else push(gap)
    push(code.slice(from, to), className)
    at = to
  })
  push(code.slice(at))
  return tokens
}

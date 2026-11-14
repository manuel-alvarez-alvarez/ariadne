/**
 * A block of code the way the console and the Markdown renderer show it: one
 * caption line with the language where it is known and an icon-only Copy
 * control, then the code itself, highlighted where its language is known and
 * plain where it is not. A fence has one light frame. A block with a
 * `caption` is a section of a block that frames it, so it has no frame of its
 * own, and its caption names the section first.
 *
 * The code is plain text until its language's grammar has loaded, then the
 * same text in CodeMirror's tokens, coloured by `HIGHLIGHT_STYLES`, the
 * scheme the task diff viewer uses too. The tokens of one text, language and theme are computed once.
 *
 * The block grows with its content and wraps a wide line, so it never adds a
 * scroll area of its own: the page around it is the only one that scrolls.
 * Copy puts the original text on the clipboard, never the highlighted markup.
 */

import { CopyIcon } from "lucide-react"
import { useTheme } from "next-themes"
import { type CSSProperties, useEffect, useMemo, useState } from "react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { copyText } from "@/lib/clipboard"
import { cn } from "@/lib/format"
import {
  highlight,
  knownLanguage,
  languageLabel,
  loadedGrammar,
  loadGrammar,
} from "@/lib/highlight"
import { HIGHLIGHT_STYLES } from "@/lib/highlight-style"

type Token = ReturnType<typeof highlight>[number]

/** The newest highlighted texts, so a block mounted again does not parse again. */
const CACHE_SIZE = 200
const cache = new Map<string, ReturnType<typeof highlight>>()

function cachedHighlight(
  language: string,
  code: string,
  grammar: Parameters<typeof highlight>[1],
  dark: boolean,
) {
  const key = `${language}\0${dark}\0${code}`
  let tokens = cache.get(key)
  if (!tokens) {
    tokens = highlight(code, grammar, HIGHLIGHT_STYLES[dark ? "dark" : "light"])
    if (cache.size >= CACHE_SIZE) cache.delete(cache.keys().next().value ?? "")
    cache.set(key, tokens)
  }
  return tokens
}

/** The grammar of `language` once it has loaded; a failed load stays plain. */
function useGrammar(language: string | undefined) {
  const [, setLoaded] = useState(0)
  const grammar = language === undefined ? undefined : loadedGrammar(language)
  useEffect(() => {
    if (language === undefined || grammar) return
    let live = true
    loadGrammar(language)?.then(
      () => live && setLoaded((count) => count + 1),
      () => undefined,
    )
    return () => {
      live = false
    }
  }, [language, grammar])
  return grammar
}

/** The runs of `code`: its highlight tokens once its grammar has loaded, else the text whole. */
function useTokens(code: string, language: string | undefined): Token[] {
  const dark = useTheme().resolvedTheme === "dark"
  const grammar = useGrammar(language)
  return useMemo(
    () => (language && grammar ? cachedHighlight(language, code, grammar, dark) : [{ text: code }]),
    [language, code, grammar, dark],
  )
}

function run(token: Token, key: number) {
  return token.className ? (
    <span key={key} className={token.className}>
      {token.text}
    </span>
  ) : (
    token.text
  )
}

/** The tokens split at every line break, each line's break kept at its end. */
function tokenLines(tokens: Token[]): Token[][] {
  const lines: Token[][] = [[]]
  for (const token of tokens) {
    const parts = token.text.split("\n")
    parts.forEach((text, index) => {
      const last = index === parts.length - 1
      const line = lines.at(-1) as Token[]
      const piece = last ? text : `${text}\n`
      if (piece !== "") line.push({ ...token, text: piece })
      if (!last) lines.push([])
    })
  }
  if (lines.length > 1 && lines.at(-1)?.length === 0) lines.pop()
  return lines
}

function HighlightedCode({
  code,
  language,
  firstLine,
}: {
  code: string
  language: string | undefined
  firstLine?: number
}) {
  const tokens = useTokens(code, language)
  if (firstLine === undefined) return tokens.map(run)
  const lines = tokenLines(tokens)
  const width = String(firstLine + lines.length - 1).length
  // The number is the line's generated content, so a selection and a copy skip it.
  return lines.map((line, index) => (
    <div
      // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one text, never reordered
      key={index}
      data-line={firstLine + index}
      style={{ "--gutter": `${width}ch` } as CSSProperties}
      className="flex min-h-lh before:w-(--gutter) before:shrink-0 before:pr-3 before:text-right before:text-muted-foreground/70 before:content-[attr(data-line)] before:select-none"
    >
      <span className="min-w-0 flex-1">{line.map(run)}</span>
    </div>
  ))
}

function lineTone(line: string): string | undefined {
  if (line.startsWith("+") && !line.startsWith("+++")) return "text-status-done-fg"
  if (line.startsWith("-") && !line.startsWith("---")) return "text-status-danger-fg"
  if (line.startsWith("@@")) return "text-muted-foreground"
  return undefined
}

export function CodeBlock({
  code,
  language,
  diff = false,
  caption,
  firstLine,
  className,
}: {
  code: string
  /** A fence's language name or one `knownLanguage` returned; unknown is plain text. */
  language?: string
  /** Color the lines as a unified diff. */
  diff?: boolean
  /**
   * The section this code is of a block that frames it, such as `input` or
   * `output`: the code is then bare, with no frame of its own, under a caption
   * that names the section and then the language.
   */
  caption?: string
  /** Number the lines from this one, in a gutter outside the text. */
  firstLine?: number
  className?: string
}) {
  const known = diff ? "diff" : knownLanguage(language)
  const label = diff ? "diff" : known ? languageLabel(known) : undefined
  const copyLabel = caption ? `Copy ${caption}` : label ? `Copy ${label} code` : "Copy code"
  return (
    <div
      data-code-block
      className={cn(
        "min-w-0 font-mono text-xs",
        caption === undefined && "rounded-md border border-border/60",
        className,
      )}
    >
      <div
        data-code-caption
        className={cn(
          "flex items-center gap-2 text-[11px] text-muted-foreground",
          caption === undefined && "px-2 pt-0.5",
        )}
      >
        <span className="min-w-0 flex-1 truncate">
          {caption}
          {caption && label && label !== caption ? <span aria-hidden> · </span> : null}
          <span data-code-language>{label === caption ? "" : (label ?? "")}</span>
        </span>
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="size-5"
          aria-label={copyLabel}
          title={copyLabel}
          onClick={() =>
            void copyText(code).then((copied) =>
              copied ? toast.success("Code copied") : toast.error("Could not copy the code"),
            )
          }
        >
          <CopyIcon />
        </Button>
      </div>
      <pre
        className={cn(
          "m-0 min-w-0 whitespace-pre-wrap break-words",
          caption === undefined ? "px-2 pt-0.5 pb-2" : "pt-0.5",
        )}
      >
        <code className="p-0">
          {diff ? (
            code.split("\n").map((line, index) => (
              <div
                // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff, never reordered
                key={index}
                className={lineTone(line)}
              >
                {line || " "}
              </div>
            ))
          ) : (
            <HighlightedCode code={code} language={known} firstLine={firstLine} />
          )}
        </code>
      </pre>
    </div>
  )
}

/**
 * A block of code the way the console and the Markdown renderer show it: the
 * language where it is known, a Copy control, and the code itself, highlighted
 * where its language is known and plain where it is not.
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
import { useEffect, useMemo, useState } from "react"
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

function HighlightedCode({ code, language }: { code: string; language: string | undefined }) {
  const dark = useTheme().resolvedTheme === "dark"
  const grammar = useGrammar(language)
  const tokens = useMemo(
    () => (language && grammar ? cachedHighlight(language, code, grammar, dark) : undefined),
    [language, code, grammar, dark],
  )
  if (!tokens) return code
  return tokens.map((token, index) =>
    token.className ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: the tokens of one text, never reordered
      <span key={index} className={token.className}>
        {token.text}
      </span>
    ) : (
      token.text
    ),
  )
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
  className,
}: {
  code: string
  /** A fence's language name or one `knownLanguage` returned; unknown is plain text. */
  language?: string
  /** Color the lines as a unified diff. */
  diff?: boolean
  className?: string
}) {
  const known = diff ? "diff" : knownLanguage(language)
  const label = diff ? "diff" : known ? languageLabel(known) : undefined
  return (
    <div
      data-code-block
      className={cn("rounded-md border bg-muted/50 font-mono text-xs", className)}
    >
      <div className="flex items-center justify-between gap-2 border-b px-2 py-0.5 text-[11px] text-muted-foreground">
        <span data-code-language>{label ?? ""}</span>
        <Button
          type="button"
          size="xs"
          variant="ghost"
          aria-label={label ? `Copy ${label} code` : "Copy code"}
          onClick={() =>
            void copyText(code).then((copied) =>
              copied ? toast.success("Code copied") : toast.error("Could not copy the code"),
            )
          }
        >
          <CopyIcon />
          Copy
        </Button>
      </div>
      <pre className="m-0 min-w-0 p-2 whitespace-pre-wrap break-words">
        <code className="bg-transparent p-0">
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
            <HighlightedCode code={code} language={known} />
          )}
        </code>
      </pre>
    </div>
  )
}

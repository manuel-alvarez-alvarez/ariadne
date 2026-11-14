/**
 * A block of code the way the console and the Markdown renderer show it: the
 * language where it is known, a Copy control, and the code itself, highlighted
 * where its language is known and plain where it is not.
 *
 * The block grows with its content and wraps a wide line, so it never adds a
 * scroll area of its own: the page around it is the only one that scrolls.
 * Copy puts the original text on the clipboard, never the highlighted markup.
 */

import { CopyIcon } from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { copyText } from "@/lib/clipboard"
import { cn } from "@/lib/format"
import { highlight, knownLanguage, languageLabel, type TokenKind } from "@/lib/highlight"

const TOKEN_TONE: Record<TokenKind, string> = {
  comment: "text-muted-foreground italic",
  string: "text-status-done-fg",
  number: "text-status-warn-fg",
  keyword: "text-status-active-fg",
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
          {diff
            ? code.split("\n").map((line, index) => (
                <div
                  // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff, never reordered
                  key={index}
                  className={lineTone(line)}
                >
                  {line || " "}
                </div>
              ))
            : highlight(code, known).map((token, index) =>
                token.kind ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: the tokens of one text, never reordered
                  <span key={index} className={TOKEN_TONE[token.kind]}>
                    {token.text}
                  </span>
                ) : (
                  token.text
                ),
              )}
        </code>
      </pre>
    </div>
  )
}

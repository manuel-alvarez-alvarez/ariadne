/**
 * The file index of the expanded diff: a filename search, every changed file
 * with its own counts, and previous / next buttons. Picking a file scrolls its
 * section into view and changes nothing else — folding and wrapping stay as
 * the reader left them.
 */

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { cn, plural } from "@/lib/format"
import type { DiffFile } from "./diff-parse"

/** The DOM id of a file's section, which the index scrolls to. */
export function diffFileDomId(fileId: string): string {
  return `diff-file-${fileId}`
}

function scrollToFile(fileId: string) {
  document.getElementById(diffFileDomId(fileId))?.scrollIntoView?.({ block: "start" })
}

export function DiffFileNav({ files }: { files: DiffFile[] }) {
  const [query, setQuery] = useState("")
  const [currentId, setCurrentId] = useState<string | null>(null)
  const needle = query.trim().toLowerCase()
  const shown = needle ? files.filter((file) => file.path.toLowerCase().includes(needle)) : files
  const at = shown.findIndex((file) => file.id === currentId)

  const go = (file: DiffFile | undefined) => {
    if (!file) return
    setCurrentId(file.id)
    scrollToFile(file.id)
  }

  return (
    <nav aria-label="Changed files" className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Filter files by name"
          aria-label="Filter files by name"
          autoComplete="off"
          className="h-7 max-w-xs flex-1"
        />
        <Button
          variant="ghost"
          size="sm"
          onClick={() => go(at <= 0 ? shown[shown.length - 1] : shown[at - 1])}
          disabled={shown.length === 0}
        >
          <ChevronUpIcon />
          Previous file
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => go(shown[at + 1] ?? shown[0])}
          disabled={shown.length === 0}
        >
          <ChevronDownIcon />
          Next file
        </Button>
      </div>
      {shown.length === 0 ? (
        <p className="text-sm text-muted-foreground">No changed file matches “{query.trim()}”.</p>
      ) : (
        <ul aria-label={plural(shown.length, "file")} className="max-h-32 overflow-y-auto">
          {shown.map((file) => (
            <li key={file.id}>
              <button
                type="button"
                onClick={() => go(file)}
                aria-current={file.id === currentId ? "true" : undefined}
                className={cn(
                  "flex w-full items-center gap-2 rounded px-2 py-0.5 text-left hover:bg-muted focus-visible:outline-2",
                  file.id === currentId && "bg-muted",
                )}
              >
                <span className="min-w-0 flex-1 truncate font-mono text-xs">{file.path}</span>
                {file.binary ? (
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">binary</span>
                ) : (
                  <span className="shrink-0 font-mono text-xs">
                    <span className="text-diff-add-fg">+{file.additions}</span>{" "}
                    <span className="text-diff-remove-fg">−{file.deletions}</span>
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </nav>
  )
}

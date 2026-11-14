/**
 * A block of the transcript that folds into one summary line. The whole
 * summary is the button, so a click or Enter anywhere on it toggles the fold;
 * the content sits outside the button, so selecting it, following a link in it
 * and its Copy controls never fold or unfold the block.
 */

import { ChevronRightIcon } from "lucide-react"
import { type ReactNode, useId } from "react"

import { cn } from "@/lib/format"

export function Foldable({
  folded,
  onToggle,
  summary,
  children,
  className,
}: {
  folded: boolean
  onToggle: () => void
  summary: ReactNode
  children: ReactNode
  className?: string
}) {
  const panel = useId()
  return (
    <div className={className}>
      <button
        type="button"
        aria-expanded={!folded}
        aria-controls={folded ? undefined : panel}
        onClick={onToggle}
        className="flex w-full min-w-0 items-start gap-1.5 rounded-md px-2 py-1 text-left font-mono text-xs text-muted-foreground outline-none hover:bg-muted/60 hover:text-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <ChevronRightIcon
          aria-hidden
          className={cn("mt-0.5 size-3.5 shrink-0 transition-transform", !folded && "rotate-90")}
        />
        <span className="min-w-0 flex-1 break-words">{summary}</span>
      </button>
      {folded ? null : (
        <div id={panel} className="px-2 pt-1 pb-2">
          {children}
        </div>
      )}
    </div>
  )
}

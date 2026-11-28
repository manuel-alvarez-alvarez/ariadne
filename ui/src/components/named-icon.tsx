/**
 * An icon that stands for a word where a row has no room for the word: the
 * kind of a step, the author of a turn. The word is the icon's accessible
 * name and its tooltip, so the icon is never the only carrier of it.
 */

import type { LucideIcon } from "lucide-react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

export function NamedIcon({
  icon: Icon,
  label,
  className,
  focusable = false,
}: {
  icon: LucideIcon
  /** The word the icon stands for: its accessible name and its tooltip. */
  label: string
  className?: string
  /** A tab stop, where no control around the icon takes the focus. */
  focusable?: boolean
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        tabIndex={focusable ? 0 : -1}
        render={
          <span
            role="img"
            aria-label={label}
            className={cn("inline-flex shrink-0 items-center", className)}
          />
        }
      >
        <Icon className="size-3.5" aria-hidden />
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

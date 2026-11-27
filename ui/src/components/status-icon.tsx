/**
 * The icon a status is shown as on the goals board, where a row has no room
 * for a pill.
 *
 * Meaning is carried twice, so colour is never the only carrier: each status
 * has its own icon shape, and the colour only tints it. The word is the
 * icon's accessible name and leads its tooltip. Like {@link StatusBadge}, the
 * tooltip sits on a real span rather than a `title=`, so it reaches a keyboard
 * where the caller gives the span a tab stop.
 */

import type { LucideIcon } from "lucide-react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

export function StatusIcon({
  icon: Icon,
  label,
  hint,
  tone,
  hintTabIndex,
  className,
}: {
  icon: LucideIcon
  /** What the status is called: the accessible name, and the tooltip's first word. */
  label: string
  /** What the status means, after the word in the tooltip. */
  hint?: string
  /** Text colour class from the status ramp; the icon draws in `currentColor`. */
  tone: string
  /** The tab stop, `-1` inside a link that is already focusable. */
  hintTabIndex?: number
  className?: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        tabIndex={hintTabIndex}
        render={
          <span
            role="img"
            aria-label={label}
            data-status-icon
            className={cn("inline-flex shrink-0 items-center", tone, className)}
          />
        }
      >
        <Icon className="size-3.5" aria-hidden />
      </TooltipTrigger>
      <TooltipContent>{hint ? `${label} — ${hint}` : label}</TooltipContent>
    </Tooltip>
  )
}

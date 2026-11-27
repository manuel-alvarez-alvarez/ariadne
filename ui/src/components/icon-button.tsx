import type { ComponentProps, ReactNode } from "react"

import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

/**
 * An action drawn as its icon alone — the one button every desktop action
 * that used to carry a word is built from.
 *
 * The `label` is both the accessible name and the tooltip, so a click target
 * with no text still says what it does to a screen reader, a keyboard (the
 * tooltip opens on focus) and a pointer. `tooltip` replaces what the tooltip
 * shows when it has more to say than the name does — a chord, or what a retry
 * keeps — and never changes the name.
 *
 * `mark` is a small dot on the corner for news the icon cannot carry, with its
 * own text for a screen reader. Everything else is a `Button`'s: `variant`,
 * `pending`, `disabled`, `aria-pressed`, `render` (to draw a link).
 */
export function IconButton({
  icon,
  label,
  tooltip,
  mark,
  size = "icon-sm",
  variant = "outline",
  children,
  ...props
}: Omit<ComponentProps<typeof Button>, "aria-label" | "children"> & {
  icon: ReactNode
  label: string
  tooltip?: ReactNode
  /** The text of the corner dot; no dot while it is empty. */
  mark?: string
  children?: never
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={<Button size={size} variant={variant} aria-label={label} {...props} />}
      >
        {icon}
        {mark ? (
          <span
            role="status"
            className="absolute top-0.5 right-0.5 size-2 rounded-full bg-primary ring-2 ring-background"
          >
            <span className="sr-only">{mark}</span>
          </span>
        ) : null}
      </TooltipTrigger>
      <TooltipContent>{tooltip ?? label}</TooltipContent>
    </Tooltip>
  )
}

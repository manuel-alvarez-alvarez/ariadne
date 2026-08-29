/**
 * The pill a goal, task or session status is shown as.
 *
 * Colour carries the meaning here, so the badge takes it rather than picking
 * it: each feature's status module owns the tone (and the dot) for its own
 * vocabulary, and this component owns the shape. That keeps a status the same
 * pill wherever it appears, and keeps the colours in one file per feature.
 *
 * `box` is the one thing the call sites still differ on, because they always
 * have: the goal and session pills are the bordered badge box (a fixed height,
 * so the border does not grow them), the board's are plain spans that size
 * themselves. Both are kept as they were rather than picked for them here.
 *
 * A pill with a `hint` is a real `Tooltip` on a focusable span, not a `title=`:
 * what a status *means* is the one thing on the pill that is not already
 * written on it, and a title attribute never reaches a keyboard.
 */

import { cva, type VariantProps } from "class-variance-authority"
import type { ReactNode } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

const statusBadgeVariants = cva(
  "inline-flex w-fit shrink-0 items-center gap-1.5 rounded-full py-0.5 text-xs font-medium whitespace-nowrap",
  {
    variants: {
      /** The compact pill sits in a dense row (a board card); the default in prose. */
      size: {
        sm: "px-1.5",
        md: "px-2",
      },
      box: {
        /** Sizes itself to its text. */
        plain: "",
        /** The badge box: fixed height, and a border that takes no colour. */
        badge: "h-5 overflow-hidden border border-transparent",
        /** The badge box with the border drawn, for a pill whose colour is its dot. */
        outlined: "h-5 overflow-hidden border border-border",
      },
    },
    defaultVariants: {
      size: "md",
      box: "plain",
    },
  },
)

export function StatusBadge({
  label,
  tone,
  dot,
  icon,
  pulse = false,
  size,
  box,
  hint,
  hintTabIndex,
  className,
}: VariantProps<typeof statusBadgeVariants> & {
  /** What the status is called, already capitalized. */
  label: string
  /** Background and text classes for this status. */
  tone?: string
  /** Dot colour classes; omitted, the badge has no dot. */
  dot?: string
  /**
   * A lucide icon leading the label, where the pill draws its meaning rather
   * than tinting it — the review verdicts. It takes the dot's place.
   */
  icon?: ReactNode
  /** Pulses the dot, for a status that is still moving. */
  pulse?: boolean
  /** What the status means; shown on hover and on focus. */
  hint?: string
  /**
   * The hint's tab stop. `-1` for a pill inside something that is already
   * focusable — a board card's link — where the hint reaches a keyboard
   * through that element's `aria-describedby` instead, and a stop of its own
   * would only be an interactive node nested in an interactive one.
   */
  hintTabIndex?: number
  className?: string
}) {
  const content = (
    <>
      {icon ??
        (dot ? (
          <span
            className={cn("size-1.5 shrink-0 rounded-full", dot, pulse && "animate-pulse")}
            aria-hidden
          />
        ) : null)}
      {label}
    </>
  )
  const classes = cn(statusBadgeVariants({ size, box }), tone, className)

  if (!hint) return <span className={classes}>{content}</span>

  return (
    <Tooltip>
      <TooltipTrigger tabIndex={hintTabIndex} render={<span className={classes} />}>
        {content}
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  )
}

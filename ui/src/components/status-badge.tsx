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
 */

import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

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
  pulse = false,
  size,
  box,
  title,
  className,
}: VariantProps<typeof statusBadgeVariants> & {
  /** What the status is called, already capitalized. */
  label: string
  /** Background and text classes for this status. */
  tone?: string
  /** Dot colour classes; omitted, the badge has no dot. */
  dot?: string
  /** Pulses the dot, for a status that is still moving. */
  pulse?: boolean
  /** What the status means, on hover. */
  title?: string
  className?: string
}) {
  return (
    <span className={cn(statusBadgeVariants({ size, box }), tone, className)} title={title}>
      {dot ? (
        <span
          className={cn("size-1.5 shrink-0 rounded-full", dot, pulse && "animate-pulse")}
          aria-hidden
        />
      ) : null}
      {label}
    </span>
  )
}

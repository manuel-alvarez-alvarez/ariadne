/**
 * The pill a goal, task or session status is shown as.
 *
 * Colour carries the meaning here, so the badge takes it rather than picking
 * it: each feature's status module owns the tone (and the dot) for its own
 * vocabulary, and this component owns the shape. That keeps a status the same
 * pill wherever it appears, and keeps the colours in one file per feature.
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
      /** An outlined pill takes its meaning from the dot rather than a fill. */
      outline: {
        true: "border border-border",
        false: "",
      },
    },
    defaultVariants: {
      size: "md",
      outline: false,
    },
  },
)

export function StatusBadge({
  label,
  tone,
  dot,
  pulse = false,
  size,
  outline,
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
    <span className={cn(statusBadgeVariants({ size, outline }), tone, className)} title={title}>
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

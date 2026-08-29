/**
 * The block of short facts every panel opens with — a goal's, a task's, a
 * session's, a profile's — as one component.
 *
 * All four were the same grid of `<dt>`/`<dd>` pairs written out four times,
 * and they had drifted where nothing forced them not to: three labelled their
 * facts in plain muted `text-xs` and the fourth in uppercase small caps, one
 * truncated every value and the others let them wrap, and the card frame was
 * spelled out with a different radius in each. A fact is the same thing in
 * every panel, so it reads the same in every panel, and the next thing that is
 * wrong with one of them is wrong in one place.
 *
 * What stays with the panel is what it is a panel *of*: which facts, in which
 * order, and what each value is made of.
 *
 * Values are not truncated here. A fact is as often a list — a task's
 * reviewers, a goal's repositories — as it is a single line, so the cell only
 * promises the width to work in (`min-w-0`), and a value that has to be cut
 * short says so itself. See {@link Fact.className} for the fact that takes the
 * whole row.
 */

import type { ReactNode } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

export function FactList({
  columns = 3,
  framed = true,
  className,
  children,
}: {
  /**
   * How many facts stand side by side where there is room. Three is the panel
   * default; two is for the lists that carry long values — a task's branch and
   * worktree paths — which a third column only cuts shorter.
   *
   * One column below `sm` either way: a 48rem panel on a narrow window is not
   * two of anything.
   */
  columns?: 2 | 3
  /** The card the facts sit in. Off where the surface around them is already one. */
  framed?: boolean
  className?: string
  children: ReactNode
}) {
  return (
    <dl
      className={cn(
        "grid gap-x-6 gap-y-3 text-sm",
        columns === 3 ? "sm:grid-cols-2 lg:grid-cols-3" : "sm:grid-cols-2",
        framed && "rounded-lg border bg-card p-3",
        className,
      )}
    >
      {children}
    </dl>
  )
}

export function Fact({
  label,
  hint,
  caption,
  className,
  children,
}: {
  label: string
  /**
   * What the value leaves unsaid, in a real hint rather than a `title=`: it
   * opens on focus, which is the only way a keyboard reaches it.
   */
  hint?: ReactNode
  /** A blurb about the value, on its own wrapping line under it. */
  caption?: ReactNode
  /**
   * How many of the grid's columns this fact takes; one, unless it says so —
   * `sm:col-span-2 lg:col-span-3` for the one that spans the row.
   */
  className?: string
  children: ReactNode
}) {
  return (
    <div className={cn("min-w-0 space-y-0.5", className)}>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      {hint ? (
        <Tooltip>
          <TooltipTrigger render={<dd className="min-w-0" />}>{children}</TooltipTrigger>
          <TooltipContent>{hint}</TooltipContent>
        </Tooltip>
      ) : (
        <dd className="min-w-0">{children}</dd>
      )}
      {caption ? <dd className="text-xs leading-snug text-muted-foreground">{caption}</dd> : null}
    </div>
  )
}

/**
 * "There is nothing here", everywhere it has to be said.
 *
 * An empty list is a normal state, not a failure, so it reads as a dashed box
 * with a sentence in it — and, where there is something the user can do about
 * it, the button that does it.
 *
 * How loudly it says so is the caller's, because it always was: a screen the
 * user is expected to fill announces itself and offers the way to fill it,
 * while a tab that is simply empty says one quiet sentence.
 */

import type { ReactNode } from "react"

import { cn } from "@/lib/format"

export function EmptyState({
  icon,
  title,
  description,
  action,
  emphasis = "prominent",
  className,
}: {
  /** A lucide icon, when one adds anything to the sentence. */
  icon?: ReactNode
  title: string
  /** Why it is empty, or what would fill it. */
  description?: ReactNode
  /** The way out of the empty state, when there is one. */
  action?: ReactNode
  /** `quiet` renders the title as the plain muted sentence it is. */
  emphasis?: "prominent" | "quiet"
  className?: string
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-2 rounded-lg border border-dashed px-4 py-8 text-center",
        className,
      )}
    >
      {/* The icon repeats the title in a picture; a screen reader gets it once. */}
      {icon ? (
        <span className="text-muted-foreground" aria-hidden>
          {icon}
        </span>
      ) : null}
      <p className={cn("text-sm", emphasis === "quiet" ? "text-muted-foreground" : "font-medium")}>
        {title}
      </p>
      {description ? <p className="max-w-sm text-sm text-muted-foreground">{description}</p> : null}
      {action}
    </div>
  )
}

/**
 * "There is nothing here", everywhere it has to be said.
 *
 * An empty list is a normal state, not a failure, so it reads as a dashed box
 * with a sentence in it — and, where there is something the user can do about
 * it, the button that does it.
 */

import type { ReactNode } from "react"

import { cn } from "@/lib/utils"

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  /** A lucide icon, when one adds anything to the sentence. */
  icon?: ReactNode
  title: string
  /** Why it is empty, or what would fill it. */
  description?: ReactNode
  /** The way out of the empty state, when there is one. */
  action?: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-2 rounded-lg border border-dashed px-4 py-8 text-center",
        className,
      )}
    >
      {icon ? <span className="text-muted-foreground">{icon}</span> : null}
      <p className="text-sm font-medium">{title}</p>
      {description ? <p className="max-w-sm text-sm text-muted-foreground">{description}</p> : null}
      {action}
    </div>
  )
}

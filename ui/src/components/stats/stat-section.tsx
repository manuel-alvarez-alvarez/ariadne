/**
 * One section of the Stats screen: the family's heading, the one sentence of
 * the question it answers, and then what the read came back with — the
 * failure or the skeleton `StatQueryState` draws, the one muted sentence of
 * an empty family, or the family's own figures.
 *
 * Every family draws through this, so the five read as one card style and
 * an empty or a failed family looks the same wherever it is. A card grows to
 * the height of its row in the screen's grid, and its body grows with it.
 */

import type { ReactNode } from "react"

import { EmptyState } from "@/components/empty-state"
import { cn } from "@/lib/format"
import { StatQueryState } from "./stat-query-state"

export function StatSection<T>({
  title,
  description,
  query,
  isEmpty,
  empty,
  children,
  className,
}: {
  /** "Work": the heading, and the section's accessible name. */
  title: string
  /** The question the family answers, in one sentence. */
  description: string
  query: {
    data: T | undefined
    isPending: boolean
    isError: boolean
    error: unknown
    refetch: () => unknown
  }
  /** Whether the answer holds nothing to show. */
  isEmpty: (data: T) => boolean
  /** The muted sentence an empty family says. */
  empty: string
  /** The family's figures, once the answer holds some. */
  children: (data: T) => ReactNode
  /** The card's place in the screen's grid, such as a full-width span. */
  className?: string
}) {
  const { data } = query
  return (
    <section
      aria-label={title}
      className={cn("flex min-w-0 flex-col gap-3 rounded-xl border bg-card p-4", className)}
    >
      <div className="flex flex-col gap-0.5">
        <h2 className="text-sm font-medium">{title}</h2>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <StatQueryState query={query} errorTitle={`Could not load the ${title.toLowerCase()} stats`}>
        {data === undefined || isEmpty(data) ? (
          <EmptyState emphasis="quiet" title={empty} className="flex-1 justify-center" />
        ) : (
          children(data)
        )}
      </StatQueryState>
    </section>
  )
}

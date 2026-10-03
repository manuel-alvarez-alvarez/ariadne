/**
 * One section of the Stats screen: the family's heading, the one sentence of
 * the question it answers, and then what the read came back with — the
 * failure or the skeleton `StatQueryState` draws, the one muted sentence of
 * an empty family, or the family's own figures.
 *
 * Every family draws through this, so the six read as one section style and
 * an empty or a failed family looks the same wherever it is.
 */

import type { ReactNode } from "react"

import { EmptyState } from "@/components/empty-state"
import { StatQueryState } from "./stat-query-state"

export function StatSection<T>({
  title,
  description,
  query,
  isEmpty,
  empty,
  children,
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
}) {
  const { data } = query
  return (
    <section aria-label={title} className="flex flex-col gap-3">
      <div className="flex flex-col gap-0.5">
        <h2 className="text-sm font-medium">{title}</h2>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <StatQueryState query={query} errorTitle={`Could not load the ${title.toLowerCase()} stats`}>
        {data === undefined || isEmpty(data) ? (
          <EmptyState emphasis="quiet" title={empty} />
        ) : (
          children(data)
        )}
      </StatQueryState>
    </section>
  )
}

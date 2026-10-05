/**
 * The headline figures of a section, one tile each: what a figure is, its
 * value, and a short hint under it where the value needs one. The tiles wrap
 * and grow, so each row of them fills its width and leaves no empty cell.
 */

import type { ReactNode } from "react"

import { StatExplain } from "./stat-explain"

export function StatTiles({ children }: { children: ReactNode }) {
  return <dl className="flex flex-wrap gap-3">{children}</dl>
}

export function StatTile({
  label,
  value,
  hint,
  explain,
}: {
  label: string
  value: ReactNode
  /** "of 12 tasks": what qualifies the value, in a few words. */
  hint?: ReactNode
  /** What this figure means, in one short plain sentence. */
  explain: string
}) {
  return (
    <div className="flex min-w-28 flex-1 flex-col gap-0.5 rounded-lg border bg-card px-3 py-2">
      <dt className="text-xs text-muted-foreground">
        <StatExplain explain={explain}>{label}</StatExplain>
      </dt>
      <dd className="text-xl font-semibold tabular-nums">{value}</dd>
      {hint ? <dd className="text-xs text-muted-foreground">{hint}</dd> : null}
    </div>
  )
}

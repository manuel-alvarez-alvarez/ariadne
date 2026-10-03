/**
 * The headline figures of a section, one tile each: what a figure is, its
 * value, and a short hint under it where the value needs one.
 */

import type { ReactNode } from "react"

export function StatTiles({ children }: { children: ReactNode }) {
  return <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">{children}</dl>
}

export function StatTile({
  label,
  value,
  hint,
}: {
  label: string
  value: ReactNode
  /** "of 12 tasks": what qualifies the value, in a few words. */
  hint?: ReactNode
}) {
  return (
    <div className="flex flex-col gap-0.5 rounded-lg border px-3 py-2">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-xl font-semibold tabular-nums">{value}</dd>
      {hint ? <dd className="text-xs text-muted-foreground">{hint}</dd> : null}
    </div>
  )
}

/**
 * The mount point of the dock above the composer. It holds the card of a
 * pending permission today. A later task docks the plan and the notices here.
 */

import type { ReactNode } from "react"

export function TrajectoryDock({ children }: { children?: ReactNode }) {
  if (children === undefined || children === null || children === false) return null
  return (
    <section aria-label="Console dock" className="space-y-2 px-3 pt-2">
      {children}
    </section>
  )
}

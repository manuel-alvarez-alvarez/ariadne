/**
 * The dock above the composer: the plan line, while a plan has an open
 * entry, then the card of a pending permission. With neither, it draws
 * nothing.
 */

import { Children, type ReactNode } from "react"

export function TrajectoryDock({ children }: { children?: ReactNode }) {
  if (Children.toArray(children).length === 0) return null
  return (
    <section aria-label="Console dock" className="space-y-2 px-3 pt-2">
      {children}
    </section>
  )
}

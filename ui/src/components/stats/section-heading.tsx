import type { ReactNode } from "react"

/**
 * The one heading style every panel of the Stats screen draws, so the
 * Reviews panel's heading no longer stands out at a different size from the
 * other four.
 */
export function StatSectionHeading({ children }: { children: ReactNode }) {
  return <h2 className="text-sm font-medium">{children}</h2>
}

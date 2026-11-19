/**
 * Which hosts a list over several hosts leaves out, and which are still loading.
 *
 * Two plain lines above the rows: one naming each listed host the screen does
 * not read, with the reason, and one naming each host that has not answered
 * while another has. A host that failed has its own warning
 * (`HostListErrors`), so it is in neither line.
 */

import type { Host, HostView } from "@/api"
import { HostLabel } from "@/components/host-label"

export function HostCoverage({
  absent,
  loading,
  hostCount,
}: {
  /** The listed hosts that are not read; shown only with more than one host listed. */
  absent: readonly HostView[]
  /** The hosts whose read has not answered yet. */
  loading: readonly Host[]
  /** How many hosts the screen reads: a line about loading needs another that answered. */
  hostCount: number
}) {
  const stillLoading = loading.length > 0 && loading.length < hostCount
  if (absent.length === 0 && !stillLoading) return null
  return (
    <div className="flex flex-col gap-1 text-sm text-muted-foreground">
      {absent.length > 0 ? (
        <p className="flex flex-wrap items-center gap-1.5">
          Not shown:
          {absent.map((host) => (
            <span key={host.name} className="inline-flex items-center gap-1">
              <HostLabel host={host.name} />({host.unreachable ?? "not reachable"})
            </span>
          ))}
        </p>
      ) : null}
      {stillLoading ? (
        <p className="flex flex-wrap items-center gap-1.5">
          Still loading:
          {loading.map((host) => (
            <HostLabel key={host} host={host} />
          ))}
        </p>
      ) : null}
    </div>
  )
}

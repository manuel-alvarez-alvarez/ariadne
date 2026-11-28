/**
 * The colour of a host as a dot, beside its name where a `HostLabel` pill does
 * not fit: a select, a menu row, a trigger.
 */

import { HostColorSwatch } from "@/components/host-color-swatch"
import { Skeleton } from "@/components/ui/skeleton"
import { useHostColor } from "@/hooks/use-host-color"

export function HostDot({ host, className }: { host: string; className?: string }) {
  const color = useHostColor(host)
  return color === null ? (
    <Skeleton className="size-2.5 rounded-full" />
  ) : (
    <HostColorSwatch color={color} className={className} />
  )
}

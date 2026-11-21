/**
 * The colour of a host as a dot, beside its name where a `HostLabel` pill does
 * not fit: a select, a menu row, a trigger.
 */

import { HostColorSwatch } from "@/components/host-color-swatch"
import { useHostColor } from "@/hooks/use-host-color"

export function HostDot({ host, className }: { host: string; className?: string }) {
  return <HostColorSwatch color={useHostColor(host)} className={className} />
}

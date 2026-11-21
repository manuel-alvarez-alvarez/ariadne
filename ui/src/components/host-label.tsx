/**
 * The host a row or a panel belongs to, drawn the one way the app draws it.
 *
 * Whether a row draws it at all is `useRowsShowHost()`, and whether a panel
 * does is `usePanelShowsHost()` (`@/hooks/use-host-label`): this component only
 * draws the name, in the host's colour (`hostColor`).
 */

import type { Host } from "@/api"
import { Badge } from "@/components/ui/badge"
import { useHostColor } from "@/hooks/use-host-color"
import { cn } from "@/lib/format"
import { HOST_HUE_CLASSES } from "@/lib/host-colors"

export function HostLabel({ host, className }: { host: Host; className?: string }) {
  const hue = useHostColor(host)
  return (
    <Badge
      variant="outline"
      data-host-color={hue}
      className={cn("shrink-0 border-transparent font-mono", HOST_HUE_CLASSES[hue].pill, className)}
      title="Host"
    >
      {host}
    </Badge>
  )
}

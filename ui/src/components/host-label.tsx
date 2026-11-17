/**
 * The host a row or a panel belongs to, drawn the one way the app draws it.
 *
 * Whether a row draws it at all is `useRowsShowHost()`, and whether a panel
 * does is `usePanelShowsHost()` (`@/hooks/use-host-label`): this component only
 * draws the name.
 */

import type { Host } from "@/api"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/format"

export function HostLabel({ host, className }: { host: Host; className?: string }) {
  return (
    <Badge variant="outline" className={cn("shrink-0 font-mono", className)} title="Host">
      {host}
    </Badge>
  )
}

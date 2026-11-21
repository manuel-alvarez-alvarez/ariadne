/**
 * One colour of `HOST_COLORS` as a dot: the swatch of a colour picker, and
 * what `HostDot` draws once it knows a host's colour.
 */

import { cn } from "@/lib/format"
import { HOST_HUE_CLASSES, type HostHue } from "@/lib/host-colors"

export function HostColorSwatch({ color, className }: { color: HostHue; className?: string }) {
  return (
    <span
      aria-hidden
      data-host-color={color}
      className={cn(
        "inline-block size-2.5 shrink-0 rounded-full",
        HOST_HUE_CLASSES[color].dot,
        className,
      )}
    />
  )
}

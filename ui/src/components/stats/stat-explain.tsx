/**
 * What a figure on the Stats screen means, in one short sentence: shown on
 * hover and on keyboard focus, through the app's own `Tooltip`, and wired as
 * the figure's accessible description rather than a sighted-only hint.
 *
 * Base UI's tooltip trigger and its popup share no `aria-describedby` of
 * their own — unlike the dialog and the popover it ships beside, which do —
 * so this is the one place that wires it: a stable id, set on the content and
 * named by the trigger, so a screen reader reads the same sentence a sighted
 * reader hovers or tabs to.
 */

import type { ReactNode } from "react"
import { useId } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

export function StatExplain({
  explain,
  className,
  children,
}: {
  /** What this figure means, in one short plain sentence. */
  explain: string
  className?: string
  children: ReactNode
}) {
  const id = useId()
  return (
    <Tooltip>
      <TooltipTrigger aria-describedby={id} render={<span className={className} />}>
        {children}
      </TooltipTrigger>
      <TooltipContent id={id}>{explain}</TooltipContent>
    </Tooltip>
  )
}

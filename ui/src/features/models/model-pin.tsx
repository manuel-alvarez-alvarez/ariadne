import { MiddleTruncated } from "@/components/copyable-id"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

import { pinLabel } from "./model-ref"

/** A model pin sized for either a fact cell that wraps or a table row that does not. */
export function ModelPin({
  model,
  effort,
  mode,
  className,
}: {
  model: string
  effort?: string | null
  mode: "wrap" | "row"
  className?: string
}) {
  const label = pinLabel(model, effort)

  if (mode === "wrap") {
    return <span className={cn("break-all font-mono", className)}>{label}</span>
  }

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span className={cn("flex max-w-full min-w-0 whitespace-nowrap font-mono", className)} />
        }
      >
        <MiddleTruncated value={model} title={model} />
        {effort ? <span className="shrink-0"> @ {effort}</span> : null}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{label}</TooltipContent>
    </Tooltip>
  )
}

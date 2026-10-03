import { MiddleTruncated } from "@/components/copyable-id"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

import { pinLabel } from "./model-ref"

/**
 * A model pin sized for wherever it sits: `wrap` breaks whole across as many
 * lines as a fact cell needs, and `row` and `line` instead hold it to the one
 * line a table row or a compact fact has room for — the model id cut in the
 * middle, its effort kept readable after it, and the whole pin in the
 * tooltip. The two differ only in name, for a call site to say which kind of
 * one-line place it is.
 */
export function ModelPin({
  model,
  effort,
  mode,
  className,
}: {
  model: string
  effort?: string | null
  mode: "wrap" | "row" | "line"
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

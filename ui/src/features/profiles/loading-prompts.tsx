/**
 * Standing in for the prompt tabs while the briefings are on their way.
 *
 * `GET /v1/profiles/{id}/prompts` is the one request on the profiles screen
 * whose *number of rows* the client cannot guess — a planner has one briefing,
 * an engineer five — so what stands in for them is the shape of what arrives:
 * a strip of tabs over the box the first of them is edited in.
 */

import { Skeleton } from "@/components/ui/skeleton"

export function LoadingPrompts() {
  return (
    <div className="flex flex-col gap-3" aria-busy>
      <Skeleton className="h-8 w-72" />
      <Skeleton className="h-8 w-full" />
      <Skeleton className="h-96 w-full" />
    </div>
  )
}

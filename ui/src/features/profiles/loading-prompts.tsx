/**
 * Standing in for prompts whose count is not known until they arrive.
 *
 * Both surfaces that show a profile's briefings — the details panel and the
 * form dialog — wait on the same request (`GET /v1/profiles/{id}/prompts`),
 * which is the one thing on either screen whose *number of rows* the client
 * cannot guess. They had a placeholder each, worded the same and shaped
 * differently by accident rather than by intent.
 *
 * The shape is the intent that is left: the form's sections arrive folded, so
 * what stands in for them is header rows; the panel's are open, so it is a
 * title over the block of text under it.
 */

import { Skeleton } from "@/components/ui/skeleton"

export function LoadingPrompts({ folded = false }: { folded?: boolean }) {
  if (folded) {
    return (
      <div className="flex flex-col gap-2">
        <Skeleton className="h-9 w-full" />
        <Skeleton className="h-9 w-full" />
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-4 w-40" />
      <Skeleton className="h-24 w-full" />
    </div>
  )
}

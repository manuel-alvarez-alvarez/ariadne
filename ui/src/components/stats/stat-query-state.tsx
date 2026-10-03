/**
 * What every panel of the Stats screen shows before it has rows to chart: a
 * failure as `ErrorState`, with the same retry and the same "daemon is not
 * answering" wording `DataTable` gives a list that fails the same way, or a
 * loading run as a plain skeleton the chart's own height.
 *
 * The heading stays above this in every panel, in every state — this is only
 * what stands where the chart or the empty sentence goes once there are rows
 * to say either of those things about.
 */

import type { ReactNode } from "react"

import { ApiError } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Skeleton } from "@/components/ui/skeleton"

export function StatQueryState({
  query,
  errorTitle,
  children,
}: {
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown }
  /** "Could not load the model stats" — the alert's heading when the read fails. */
  errorTitle: string
  children: ReactNode
}) {
  if (query.isError) {
    return (
      <ErrorState
        title={errorTitle}
        error={query.error}
        description={
          ApiError.is(query.error) && query.error.isNetworkError
            ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
            : undefined
        }
        onRetry={() => void query.refetch()}
      />
    )
  }
  if (query.isPending) {
    return <Skeleton className="h-24 w-full" />
  }
  return <>{children}</>
}

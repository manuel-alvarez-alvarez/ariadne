/**
 * The Permissions screen: the AI permission model's settings behind the `ai`
 * permission mode.
 *
 * One card because there is one settings row behind it — see `ai-card.tsx`.
 * A screen of its own, rather than a section of Repositories, because the
 * model is the daemon's: one install, shared by every repository that picks
 * `ai`, not a setting of any one of them.
 */

import { useQuery } from "@tanstack/react-query"

import { ApiError } from "@/api"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Skeleton } from "@/components/ui/skeleton"

import { AiCard } from "./ai-card"
import { aiPermissionsStatusQueryOptions } from "./queries"

export function PermissionsPage() {
  const status = useQuery(aiPermissionsStatusQueryOptions())

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Permissions"
        description="How the AI permission model is installed and configured."
      />

      {status.isPending ? <Skeleton className="h-72 rounded-xl" /> : null}

      {status.isError ? (
        <ErrorState
          title="Could not load the model's settings"
          error={status.error}
          description={networkHint(status.error)}
          onRetry={() => void status.refetch()}
        />
      ) : null}

      {status.data ? <AiCard status={status.data} /> : null}
    </div>
  )
}

/** A daemon that never answered has nothing to say about why. */
function networkHint(error: unknown): string | undefined {
  return ApiError.is(error) && error.isNetworkError
    ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
    : undefined
}

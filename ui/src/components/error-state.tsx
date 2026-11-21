/**
 * A failed call, everywhere one has to be shown.
 *
 * The daemon's own words are what goes on screen (see
 * {@link import("@/lib/format").describeError}), and where the caller can ask
 * again, the retry sits in the alert rather than somewhere below it.
 *
 * The icon is opt-in, because the surfaces disagree and did before: the ones
 * that lead a screen carry it, the ones inside a panel tab or a dialog do not.
 */

import { AlertCircleIcon, RefreshCwIcon, TriangleAlertIcon } from "lucide-react"
import type { ReactNode } from "react"
import { ApiError, type Host, LOCAL_HOST } from "@/api"
import { HostLabel } from "@/components/host-label"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/format"

export function ErrorState({
  title,
  error,
  description,
  onRetry,
  showIcon = false,
  className,
}: {
  title: ReactNode
  error: unknown
  /** Replaces the daemon's message, where a surface has something better to say. */
  description?: ReactNode
  /** Usually a query's `refetch`; without one, the alert has no retry. */
  onRetry?: () => void
  /** Draws the alert icon, for the surfaces that lead a screen with it. */
  showIcon?: boolean
  className?: string
}) {
  return (
    <Alert variant="destructive" className={className}>
      {showIcon ? <AlertCircleIcon /> : null}
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>{description ?? describeError(error)}</AlertDescription>
      {onRetry ? (
        <AlertAction>
          <Button variant="outline" size="sm" onClick={onRetry}>
            Retry
          </Button>
        </AlertAction>
      ) : null}
    </Alert>
  )
}

/**
 * A refresh that failed with content already cached.
 *
 * The cached rows stay on screen under this notice: they may be outdated, and
 * the retry asks again for just the read that failed. A read that never
 * succeeded is an {@link ErrorState} instead. The notice leaves once a refresh
 * succeeds, because the query's error clears with it.
 */
export function RefreshWarning({
  what,
  error,
  onRetry,
  className,
  cached = true,
}: {
  /** What is on screen, lower case: "goals", "tasks". */
  what: ReactNode
  error: unknown
  /** The failed query's `refetch`. */
  onRetry: () => void
  className?: string
  /** A last read is on screen. Without one the notice says "load" and not "outdated". */
  cached?: boolean
}) {
  return (
    <Alert className={className}>
      <RefreshCwIcon />
      <AlertTitle>
        Could not {cached ? "refresh" : "load"} {what}
      </AlertTitle>
      <AlertDescription>
        {cached ? `${OUTDATED} ` : null}
        {describeError(error)}
      </AlertDescription>
      <AlertAction>
        <Button variant="outline" size="sm" onClick={onRetry}>
          Retry
        </Button>
      </AlertAction>
    </Alert>
  )
}

const OUTDATED = "Showing what was last loaded, which may be outdated."

const NETWORK_HINT =
  "The daemon is not answering. Check the URL in settings and that it is listening on TCP."

/**
 * What to say beside a failed read of one host. A daemon that never answered
 * has nothing to say about why: for `local` that is the settings URL, and for a
 * named host the reason is the host's own message, which the error carries.
 */
export function hostErrorDescription(host: Host, error: unknown): string | undefined {
  if (host !== LOCAL_HOST) return describeError(error)
  return ApiError.is(error) && error.isNetworkError ? NETWORK_HINT : undefined
}

/** `title`, naming the host unless it is `local` and the only one read. */
export function hostTitle(title: string, host: Host, hostCount: number): ReactNode {
  if (host === LOCAL_HOST && hostCount <= 1) return title
  return (
    <>
      {title} on <HostLabel host={host} />
    </>
  )
}

/**
 * The failures of a list read over several hosts: a warning for each host that
 * failed while others answered, and an error state for each only when every
 * host failed and nothing is left to show. Each names its host and its reason.
 */
export function HostListErrors({
  what,
  errors,
  hostCount,
  hasRows,
  onRetry,
  showIcon,
}: {
  /** What is read, lower case: "goals", "sessions". */
  what: string
  errors: readonly { host: Host; error: Error; cached?: boolean }[]
  /** How many hosts are read. */
  hostCount: number
  /** Whether any row is on screen, from a host or from the cache. */
  hasRows: boolean
  onRetry: () => void
  showIcon?: boolean
}) {
  const everywhere =
    errors.length > 0 && errors.length >= hostCount && !hasRows && !errors.some((e) => e.cached)
  return (
    <>
      {errors.map(({ host, error, cached }) => {
        const title = hostTitle(`Could not load ${what}`, host, hostCount)
        return everywhere ? (
          <ErrorState
            key={host}
            title={title}
            error={error}
            description={hostErrorDescription(host, error)}
            onRetry={onRetry}
            showIcon={showIcon}
          />
        ) : (
          <Alert key={host}>
            <TriangleAlertIcon />
            <AlertTitle>{title}</AlertTitle>
            <AlertDescription>
              {cached ? `${OUTDATED} ` : null}
              {hostErrorDescription(host, error) ?? describeError(error)}
            </AlertDescription>
            <AlertAction>
              <Button variant="outline" size="sm" onClick={onRetry}>
                Retry
              </Button>
            </AlertAction>
          </Alert>
        )
      })}
    </>
  )
}

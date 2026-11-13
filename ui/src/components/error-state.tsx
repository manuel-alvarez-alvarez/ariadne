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

import { AlertCircleIcon, RefreshCwIcon } from "lucide-react"
import type { ReactNode } from "react"

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
  title: string
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
}: {
  /** What is on screen, lower case: "goals", "tasks". */
  what: string
  error: unknown
  /** The failed query's `refetch`. */
  onRetry: () => void
  className?: string
}) {
  return (
    <Alert className={className}>
      <RefreshCwIcon />
      <AlertTitle>Could not refresh {what}</AlertTitle>
      <AlertDescription>
        Showing what was last loaded, which may be outdated. {describeError(error)}
      </AlertDescription>
      <AlertAction>
        <Button variant="outline" size="sm" onClick={onRetry}>
          Retry
        </Button>
      </AlertAction>
    </Alert>
  )
}

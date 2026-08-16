/**
 * A failed call, everywhere one has to be shown.
 *
 * The daemon's own words are what goes on screen (see
 * {@link import("@/lib/errors").describeError}), and where the caller can ask
 * again, the retry sits in the alert rather than somewhere below it.
 */

import { AlertCircleIcon } from "lucide-react"
import type { ReactNode } from "react"

import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/errors"

export function ErrorState({
  title,
  error,
  description,
  onRetry,
  className,
}: {
  title: string
  error: unknown
  /** Replaces the daemon's message, where a surface has something better to say. */
  description?: ReactNode
  /** Usually a query's `refetch`; without one, the alert has no retry. */
  onRetry?: () => void
  className?: string
}) {
  return (
    <Alert variant="destructive" className={className}>
      <AlertCircleIcon />
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

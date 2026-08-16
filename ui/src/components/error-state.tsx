/**
 * A failed call, everywhere one has to be shown.
 *
 * The daemon's own words are what goes on screen (see
 * {@link import("@/lib/errors").describeError}), and where the caller can ask
 * again, the retry sits in the alert rather than somewhere below it.
 *
 * The icon is opt-in, because the surfaces disagree and did before: the ones
 * that lead a screen carry it, the ones inside a panel tab or a dialog do not.
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

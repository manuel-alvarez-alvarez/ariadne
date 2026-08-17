import { QueryClient } from "@tanstack/react-query"

import { ApiError } from "@/api"

/**
 * Whether a failed read is worth asking the daemon about again.
 *
 * Exported for the one screen-level property it decides: how long a screen
 * shimmers before it admits it has nothing. That has to be the *same* length
 * everywhere, because a daemon that went away takes every screen with it, and
 * a board that gave up at four seconds next to a table still shimmering at
 * fifteen reads as one of them being broken rather than the daemon being down.
 *
 * So the two hopeless cases are not retried at all:
 *
 * - a 4xx will not fix itself;
 * - a network failure means the request never reached a daemon, which the
 *   health probe is already polling for and the connection banner is already
 *   saying — retrying it only delays the same answer.
 *
 * A screen whose queries were warmed by something else (the sidebar's
 * attention count, another tab) would otherwise show its error first and a
 * cold one three backoffs later, which is exactly the drift this removes.
 * Everything else — a 5xx, a dropped response mid-flight — is transient and
 * still gets its two retries.
 */
export function shouldRetryQuery(failureCount: number, error: unknown): boolean {
  if (ApiError.is(error) && (error.isNetworkError || (error.status >= 400 && error.status < 500))) {
    return false
  }
  return failureCount < 2
}

/**
 * Defaults tuned for a client that also has a live event stream: the SSE
 * dispatcher patches and invalidates as things change, so polling and eager
 * refetching would only duplicate work.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // The stream is what keeps data fresh; time-based staleness is a
        // backstop for the window where the stream was down.
        staleTime: 30_000,
        retry: shouldRetryQuery,
      },
      mutations: {
        retry: false,
      },
    },
  })
}

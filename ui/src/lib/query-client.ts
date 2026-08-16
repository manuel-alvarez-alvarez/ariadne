import { QueryClient } from "@tanstack/react-query"

import { ApiError } from "@/api"

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
        retry: (failureCount, error) => {
          // A 4xx will not fix itself, and a daemon that is down is reported by
          // the connection indicator rather than retried into the ground.
          if (ApiError.is(error) && error.status >= 400 && error.status < 500) return false
          return failureCount < 2
        },
      },
      mutations: {
        retry: false,
      },
    },
  })
}

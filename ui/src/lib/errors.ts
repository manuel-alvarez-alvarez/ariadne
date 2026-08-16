/**
 * What to put in front of the user when a call fails.
 *
 * The daemon's error code is what its docs and the CLI talk about, so it is
 * shown alongside the message rather than swallowed — except for the two codes
 * that say nothing a reader could look up: a request that never reached the
 * daemon, and a non-2xx that carried no envelope.
 */

import { ApiError, HTTP_ERROR_CODE } from "@/api"

export function describeError(error: unknown): string {
  if (!ApiError.is(error)) return error instanceof Error ? error.message : String(error)
  return error.isNetworkError || error.code === HTTP_ERROR_CODE
    ? error.message
    : `${error.message} (${error.code})`
}

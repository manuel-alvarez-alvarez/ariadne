/**
 * The daemon answers every non-2xx with the same envelope:
 *
 *     {"error": {"code": "task_not_found", "message": "...", "details": {...}}}
 *
 * `ApiError` is that envelope as a throwable, so TanStack Query mutations and
 * queries can branch on `error.code` instead of parsing bodies.
 */

/** Wire shape of a daemon error response. */
interface ErrorEnvelope {
  error: {
    code: string
    message: string
    details?: unknown
  }
}

/** Code used when the request never reached the daemon. */
const NETWORK_ERROR_CODE = "network_error"
/** Code used when a non-2xx response carried no (or an unreadable) envelope. */
export const HTTP_ERROR_CODE = "http_error"

export class ApiError extends Error {
  /** HTTP status, or 0 when the request never got a response. */
  readonly status: number
  /** Stable machine-readable code, e.g. `task_not_found`. */
  readonly code: string
  /** Structured context from the envelope, when the daemon sent any. */
  readonly details: unknown

  constructor(options: {
    status: number
    code: string
    message: string
    details?: unknown
    cause?: unknown
  }) {
    super(options.message, { cause: options.cause })
    this.name = "ApiError"
    this.status = options.status
    this.code = options.code
    this.details = options.details
  }

  /** True when the request never reached the daemon (daemon down, bad URL, CORS). */
  get isNetworkError(): boolean {
    return this.code === NETWORK_ERROR_CODE
  }

  static is(value: unknown): value is ApiError {
    return value instanceof ApiError
  }

  static network(cause: unknown): ApiError {
    const message = cause instanceof Error ? cause.message : String(cause)
    return new ApiError({
      status: 0,
      code: NETWORK_ERROR_CODE,
      message: `cannot reach the daemon: ${message}`,
      cause,
    })
  }

  /** Build from a non-2xx response and whatever body was parsed out of it. */
  static fromResponse(response: Response, body: unknown): ApiError {
    if (isErrorEnvelope(body)) {
      return new ApiError({
        status: response.status,
        code: body.error.code,
        message: body.error.message,
        details: body.error.details,
      })
    }
    return new ApiError({
      status: response.status,
      code: HTTP_ERROR_CODE,
      message: `${response.status} ${response.statusText || "request failed"}`,
      details: body,
    })
  }
}

function isErrorEnvelope(value: unknown): value is ErrorEnvelope {
  if (typeof value !== "object" || value === null || !("error" in value)) return false
  const inner = (value as { error: unknown }).error
  return (
    typeof inner === "object" &&
    inner !== null &&
    typeof (inner as { code?: unknown }).code === "string" &&
    typeof (inner as { message?: unknown }).message === "string"
  )
}

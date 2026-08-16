/**
 * Typed HTTP client for the daemon.
 *
 * The UI is a pure REST/SSE client of `ariadned`'s TCP listener — exactly like
 * the CLI, never reaching into daemon internals. Every path, query parameter,
 * body and response below is typed from the committed OpenAPI types in
 * `schema.d.ts` (`npm run gen:api` regenerates them).
 *
 * Usage:
 *
 *     const goals = await unwrap(api().GET("/v1/goals", { params: { query: { limit: 50 } } }))
 *     const goal = await unwrap(api().GET("/v1/goals/{id}", { params: { path: { id } } }))
 *
 * `unwrap` returns the response body and throws [`ApiError`] on anything else,
 * which is the contract TanStack Query expects.
 */

import createClient, { type Client } from "openapi-fetch"

import { ApiError } from "./errors"
import type { paths } from "./schema"

export const DEFAULT_BASE_URL = "http://127.0.0.1:7676"

/** Strip trailing slashes so path concatenation stays predictable. */
export function normalizeBaseUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, "")
  return trimmed.length > 0 ? trimmed : DEFAULT_BASE_URL
}

let baseUrl = DEFAULT_BASE_URL
let client: Client<paths> = createClient<paths>({ baseUrl })

/**
 * Point the client at another daemon. Called by the settings store; the query
 * cache is cleared by the caller so nothing from the old daemon survives.
 */
export function setApiBaseUrl(next: string): void {
  const normalized = normalizeBaseUrl(next)
  if (normalized === baseUrl) return
  baseUrl = normalized
  client = createClient<paths>({ baseUrl: normalized })
}

export function getApiBaseUrl(): string {
  return baseUrl
}

/** The current typed client. Re-read it per call — it is swapped on URL changes. */
export function api(): Client<paths> {
  return client
}

/**
 * URL of the SSE endpoint, with the optional goal/task filters applied.
 *
 * `EventSource` is not routed through `openapi-fetch`, so the daemon it should
 * connect to is passed in explicitly rather than read off the module state.
 */
export function eventStreamUrl(
  daemonUrl: string,
  filters: { goal?: string; task?: string } = {},
): string {
  const url = new URL("/v1/events/stream", `${normalizeBaseUrl(daemonUrl)}/`)
  if (filters.goal) url.searchParams.set("goal", filters.goal)
  if (filters.task) url.searchParams.set("task", filters.task)
  return url.toString()
}

/** What `openapi-fetch` resolves to: exactly one of `data` / `error` is set. */
interface FetchResult<T> {
  data?: T
  error?: unknown
  response: Response
}

/**
 * Await an `openapi-fetch` call and return its body, turning transport failures
 * and error envelopes into [`ApiError`].
 */
export async function unwrap<T>(request: Promise<FetchResult<T>>): Promise<T> {
  let result: FetchResult<T>
  try {
    result = await request
  } catch (cause) {
    throw ApiError.network(cause)
  }
  const { data, error, response } = result
  if (!response.ok || error !== undefined) {
    throw ApiError.fromResponse(response, error ?? data)
  }
  return data as T
}

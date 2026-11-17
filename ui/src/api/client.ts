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
 *     const goals = await unwrap(api("local").GET("/v1/goals", { params: { query: { limit: 50 } } }))
 *     const goal = await unwrap(api(host).GET("/v1/goals/{id}", { params: { path: { id } } }))
 *
 * `unwrap` returns the response body and throws [`ApiError`] on anything else,
 * which is the contract TanStack Query expects.
 */

import createClient, { type Client } from "openapi-fetch"

import { recordApiRequest } from "@/telemetry/telemetry"

import { ApiError } from "./errors"
import type { paths } from "./schema"

export const DEFAULT_BASE_URL = "http://127.0.0.1:7676"

/** Strip trailing slashes so path concatenation stays predictable. */
export function normalizeBaseUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, "")
  return trimmed.length > 0 ? trimmed : DEFAULT_BASE_URL
}

/** A host's name, as `GET /v1/hosts` gives it. */
export type Host = string

/** The settings daemon: the one the base URL in the settings store points at. */
export const LOCAL_HOST: Host = "local"

/** Where a host is reached, or why it cannot be. */
type HostAddress = { url: string; error?: undefined } | { url: null; error: string }

let baseUrl = DEFAULT_BASE_URL
/** Every named host the settings daemon listed; `local` is never in here. */
let remotes = new Map<Host, HostAddress>()
/** One client per address, so a host whose address holds keeps its client. */
const clients = new Map<string, Client<paths>>()

/**
 * Point `local` at another daemon. Called by the settings store; the query
 * cache is cleared by the caller so nothing from the old daemon survives.
 */
export function setApiBaseUrl(next: string): void {
  baseUrl = normalizeBaseUrl(next)
}

/**
 * Remember where every named host is, from the settings daemon's
 * `GET /v1/hosts`. Called by the hosts query and by the dispatcher on
 * `host_updated`, and by nothing else.
 */
export function setHostAddresses(hosts: readonly HostAddressRow[]): void {
  remotes = new Map(
    hosts
      .filter((host) => host.name !== LOCAL_HOST)
      .map((host) => [
        host.name,
        host.url === null
          ? { url: null, error: host.error ?? "host is not reachable" }
          : { url: host.url },
      ]),
  )
}

/** What {@link setHostAddresses} takes: a host and the URL a client uses for it. */
interface HostAddressRow {
  name: Host
  /** Null for a host this client cannot reach. */
  url: string | null
  error?: string | null
}

/**
 * The base URL of a host: the settings base URL for `local`, the endpoint of a
 * reachable named host, and null for any other.
 */
export function hostBaseUrl(host: Host): string | null {
  if (host === LOCAL_HOST) return baseUrl
  return remotes.get(host)?.url ?? null
}

/**
 * The typed client of one host. Re-read it per call — it is swapped when the
 * host's address changes.
 *
 * A host this client cannot reach throws the same network error a daemon that
 * is down does, with the host's own reason, so a query over it fails like any
 * other unreachable read.
 */
export function api(host: Host): Client<paths> {
  const url = hostBaseUrl(host)
  if (url === null) {
    const reason = remotes.get(host)?.error ?? `unknown host ${host}`
    throw ApiError.network(new Error(reason))
  }
  let client = clients.get(url)
  if (!client) {
    client = createTelemetryClient(url)
    clients.set(url, client)
  }
  return client
}

function createTelemetryClient(url: string): Client<paths> {
  const next = createClient<paths>({ baseUrl: url })
  const started = new Map<string, number>()
  next.use({
    onRequest: ({ id }) => {
      started.set(id, performance.now())
    },
    onResponse: ({ id, request, response, schemaPath }) => {
      recordRequest(id, request.method, schemaPath, response.status)
    },
    onError: ({ id, request, schemaPath }) => {
      recordRequest(id, request.method, schemaPath, 0)
    },
  })
  function recordRequest(id: string, method: string, route: string, status: number): void {
    const began = started.get(id)
    started.delete(id)
    if (began !== undefined) recordApiRequest(method, route, status, performance.now() - began)
  }
  return next
}

/**
 * URL of one host's SSE endpoint, with the optional goal/task filters applied,
 * or null for a host this client cannot reach.
 */
export function eventStreamUrl(
  host: Host,
  filters: { goal?: string; task?: string } = {},
): string | null {
  const base = hostBaseUrl(host)
  if (base === null) return null
  const url = new URL("/v1/events/stream", `${base}/`)
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

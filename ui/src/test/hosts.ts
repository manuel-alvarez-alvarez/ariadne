/**
 * A settings daemon with one named host beside it, as the tests answer them.
 *
 * Every request reaches the one `fetch` stub (`./setup`), so the daemon a
 * request is for is its origin: `local` at the default base URL, and `box`
 * at its own endpoint. A test names what each daemon answers per path, and
 * `/v1/hosts` is the settings daemon's list of the two unless it says else.
 */

import type { HostDto } from "@/api"
import { DEFAULT_BASE_URL, rememberHosts } from "@/api"

import { jsonResponse } from "./harness"
import { daemonFetch } from "./setup"

/** The origin of the settings daemon in every test. */
export const LOCAL_ORIGIN = new URL(DEFAULT_BASE_URL).origin

/** A named host the app can reach over HTTP. */
export const BOX: HostDto = {
  name: "box",
  kind: "direct",
  status: "ready",
  endpoint: "http://box.test:7676",
  error: null,
  remote: null,
  sync: { state: "in_sync", error: null, missing_repositories: [], skipped_permissions: 0 },
}

/** `local` as the settings daemon lists it. */
export const LOCAL: HostDto = {
  name: "local",
  kind: "local",
  status: "ready",
  endpoint: DEFAULT_BASE_URL,
  error: null,
  remote: null,
  sync: { state: "in_sync", error: null, missing_repositories: [], skipped_permissions: 0 },
}

/** What one daemon answers: a body per path, or a handler for the rest. */
type Answers = Record<string, unknown | ((url: URL, request: Request) => Response | undefined)>

/** One request the stub saw: which daemon it went to, its method and its URL. */
export interface SeenRequest {
  origin: string
  method: string
  url: URL
  body: string
}

/**
 * Answers `local` and `box` from `answers`, keyed by origin, and keeps every
 * request it was sent. A path nobody answered is a request that never
 * settles, as `./setup`'s default is.
 */
export function stubHosts(
  answers: { local?: Answers; box?: Answers; at?: Record<string, Answers> },
  hosts: HostDto[] = [LOCAL, BOX],
): SeenRequest[] {
  const seen: SeenRequest[] = []
  const byOrigin: Record<string, Answers> = {
    [LOCAL_ORIGIN]: { "/v1/hosts": hosts, ...answers.local },
    [new URL(BOX.endpoint ?? "").origin]: answers.box ?? {},
    // Any other daemon, by its origin: a host that moved, say.
    ...answers.at,
  }
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const request = input instanceof Request ? input : new Request(input)
    const url = new URL(request.url)
    seen.push({
      origin: url.origin,
      method: request.method,
      url,
      body: request.method === "GET" ? "" : await request.clone().text(),
    })
    const answer = byOrigin[url.origin]?.[url.pathname]
    if (typeof answer === "function") {
      const response = answer(url, request)
      if (response) return response
    } else if (answer !== undefined) {
      return jsonResponse(answer)
    }
    return new Promise(() => {})
  })
  return seen
}

/** Forget every named host, so a test starts with `local` alone. */
export function forgetHosts(): void {
  rememberHosts([])
}

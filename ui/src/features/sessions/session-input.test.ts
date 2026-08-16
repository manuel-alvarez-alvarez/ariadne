/**
 * Tests for the one thing sending keystrokes has to get right: what reaches
 * the pane, and in what order.
 *
 * A terminal calls `sendSessionInput` once per keystroke and never waits, so
 * the ordering is entirely this module's problem — fired in parallel the
 * requests race down separate connections and `echo` arrives as `ceho`. And
 * when a send fails there is no retry, which has to include the input that
 * piled up behind it: replayed later, a Return or a Ctrl-C acts on whatever
 * the pane happens to be showing by then.
 *
 * The daemon is a controllable `fetch` rather than a mocked client, so the
 * real `openapi-fetch` path and the real request bodies are what is asserted.
 */

import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { DEFAULT_BASE_URL, setApiBaseUrl } from "@/api"

import { sendSessionInput } from "./queries"

const SESSION = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
/** Anywhere but a real daemon: these requests must never leave the process. */
const BASE_URL = "http://daemon.test"

/** One intercepted request, answerable whenever the test chooses. */
interface Call {
  url: string
  body: string
  resolve: (response: Response) => void
  reject: (error: Error) => void
}

let calls: Call[]
let originalFetch: typeof globalThis.fetch

/** What the daemon sends back on success: `204 No Content`. */
function noContent(): Response {
  return new Response(null, { status: 204 })
}

/** The daemon's `409` envelope, as a non-live session produces it. */
function conflict(): Response {
  return new Response(JSON.stringify({ error: { code: "conflict", message: "no live pane" } }), {
    status: 409,
    headers: { "content-type": "application/json" },
  })
}

/** The `data` of every request made so far, oldest first. */
function sent(): string[] {
  return calls.map((call) => JSON.parse(call.body).data)
}

/** The nth intercepted request, asserted to have been made. */
function call(index: number): Call {
  const found = calls[index]
  if (!found) throw new Error(`no request #${index}: only ${calls.length} were made`)
  return found
}

/** Wait for the request the caller expects to be in flight. */
async function callCount(n: number): Promise<void> {
  for (let i = 0; i < 100 && calls.length < n; i++) await Promise.resolve()
  expect(calls.length).toBe(n)
}

beforeEach(() => {
  calls = []
  originalFetch = globalThis.fetch
  globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    // `openapi-fetch` hands over a `Request`, body and all, so the payload is
    // read off that rather than off `init`.
    const request = input instanceof Request ? input : new Request(String(input), init)
    return new Promise<Response>((resolve, reject) => {
      void request
        .clone()
        .text()
        .then((body) => calls.push({ url: request.url, body, resolve, reject }))
    })
  }) as typeof globalThis.fetch
  // `openapi-fetch` captures `globalThis.fetch` when the client is built, so
  // the client has to be rebuilt after the stub is in place — otherwise these
  // requests go to whatever daemon is actually listening.
  setApiBaseUrl(BASE_URL)
})

afterEach(() => {
  globalThis.fetch = originalFetch
  setApiBaseUrl(DEFAULT_BASE_URL)
})

it("sends one keystroke straight through", async () => {
  const sending = sendSessionInput(SESSION, "a")
  await callCount(1)

  expect(call(0).url).toContain(`/v1/sessions/${SESSION}/input`)
  expect(sent()).toEqual(["a"])

  call(0).resolve(noContent())
  await expect(sending).resolves.toBeUndefined()
})

it("never has two requests in flight for one session", async () => {
  void sendSessionInput(SESSION, "e")
  await callCount(1)

  // Typed while the first request is still open.
  void sendSessionInput(SESSION, "c")
  void sendSessionInput(SESSION, "h")
  await callCount(1)

  call(0).resolve(noContent())
  await callCount(2)
  // What was typed meanwhile rides along in the next request, in order.
  expect(sent()).toEqual(["e", "ch"])

  call(1).resolve(noContent())
  await callCount(2)
})

it("drops what was typed behind a failed request", async () => {
  const failing = sendSessionInput(SESSION, "sleep 30")
  await callCount(1)

  // Typed while that request is in flight: a Return, which must not be
  // replayed once the session accepts input again.
  const queued = sendSessionInput(SESSION, "\r")
  call(0).resolve(conflict())
  await expect(failing).rejects.toThrow()
  await expect(queued).rejects.toThrow()

  // The next keystroke starts over: no stale Return in front of it.
  const next = sendSessionInput(SESSION, "x")
  await callCount(2)
  expect(sent()).toEqual(["sleep 30", "x"])

  call(1).resolve(noContent())
  await expect(next).resolves.toBeUndefined()
})

it("keeps working after a failure", async () => {
  const failing = sendSessionInput(SESSION, "a")
  await callCount(1)
  call(0).reject(new Error("daemon went away"))
  await expect(failing).rejects.toThrow()

  const recovered = sendSessionInput(SESSION, "b")
  await callCount(2)
  expect(sent()).toEqual(["a", "b"])
  call(1).resolve(noContent())
  await expect(recovered).resolves.toBeUndefined()
})

it("keeps sessions apart", async () => {
  const other = "01ARZ3NDEKTSV4RRFFQ69G5FAW"
  void sendSessionInput(SESSION, "a")
  void sendSessionInput(other, "b")
  // One in flight per session, not one overall.
  await callCount(2)

  expect(call(0).url).toContain(SESSION)
  expect(call(1).url).toContain(other)
  call(0).resolve(noContent())
  call(1).resolve(noContent())
})

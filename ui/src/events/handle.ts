/**
 * The app's one domain stream, reachable from outside React's tree.
 *
 * The connection banner's Retry button has to reach the stream that
 * `EventStreamProvider` owns, and that is the only imperative thing anything
 * outside the provider ever asks of it. A context for one call would mean
 * threading a provider through every consumer of `useConnection` — the footer,
 * the banner and their tests — so the provider registers its stream here on
 * mount and clears it on unmount instead.
 */

import type { DomainEventStream } from "./stream"

let current: DomainEventStream | null = null

/** Called by {@link import("./provider").EventStreamProvider}, and by nothing else. */
export function setDomainStream(stream: DomainEventStream | null): void {
  current = stream
}

/**
 * Drop the connection and open a new one now, instead of waiting out the
 * backoff. A no-op before the provider has mounted, which is what a click on a
 * banner that cannot be on screen yet deserves.
 */
export function retryDomainStream(reason: string): void {
  current?.forceReconnect(reason)
}

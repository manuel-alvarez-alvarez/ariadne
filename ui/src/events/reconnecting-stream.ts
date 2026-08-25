/**
 * An `EventSource` that reconnects on its own terms.
 *
 * Three streams in this app need one — the domain events, a session's pane, the
 * daemon's log — and all three had written the same machinery for themselves:
 * open, close the old source first, retry with capped exponential backoff and
 * jitter, cancel the pending retry on stop. Written three times they drifted in
 * the details that only show up when a daemon restarts.
 *
 * The browser's own reconnect is deliberately not used by any of them. It
 * cannot be told apart from a clean end, it gives the consumer no way to know a
 * gap opened, and it would silently splice a new snapshot onto old output.
 *
 * What a subclass supplies is what is actually its own: the URL, the events it
 * listens for, and the words it reports its state in.
 */

const INITIAL_BACKOFF_MS = 500

/** The three states a connection is in, in whatever words the consumer uses. */
interface StreamStates<S extends string> {
  /** Opening the first connection. */
  connecting: S
  /** Opening one after a gap. */
  reconnecting: S
  /** Connected and delivering. Absent where the consumer has no such state. */
  live?: S
}

interface StreamOptions<S extends string> {
  states: StreamStates<S>
  /** Longest the backoff may grow to. */
  maxBackoffMs: number
  onStatus: (status: S, error?: string | null) => void
  /**
   * Every successful open. `reconnected` is false only for a first connection
   * that succeeded straight away, where nothing can have been missed.
   */
  onOpen?: (info: { reconnected: boolean }) => void
  /** What a dropped connection is called in the status it reports. */
  dropped: string
}

export abstract class ReconnectingEventStream<S extends string> {
  #source: EventSource | null = null
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #backoff = INITIAL_BACKOFF_MS
  #stopped = true
  /** Set by {@link halt}: there is nothing left to reconnect to. */
  #halted = false
  #everOpened = false
  /**
   * Whether anything went wrong since the last successful open: a failed
   * attempt, or a drop forced from outside. This — not `#everOpened` — is what
   * makes an open count as a reconnect, because a first connection that only
   * came up after a few failures had a gap just the same.
   */
  #interrupted = false

  /** Re-read per attempt: the daemon it points at can change under the stream. */
  #url: () => string
  #options: StreamOptions<S>

  protected constructor(url: () => string, options: StreamOptions<S>) {
    this.#url = url
    this.#options = options
  }

  /** Register the subclass's own listeners on a freshly opened source. */
  protected abstract listen(source: EventSource): void

  /**
   * Connect, and keep reconnecting until {@link stop}. Idempotent.
   *
   * A start is a fresh run, so it forgets the gaps of the last one: the first
   * connection of a restarted stream reports {@link StreamStates.connecting},
   * not `reconnecting`. Only the backoff is carried over — a daemon that was
   * refusing connections a moment ago is unlikely to have changed its mind
   * because the view asked again.
   */
  start(): void {
    if (!this.#stopped) return
    this.#stopped = false
    this.#halted = false
    this.#everOpened = false
    this.#interrupted = false
    this.#connect()
  }

  /** Disconnect and cancel any pending retry. Idempotent. */
  stop(): void {
    this.#stopped = true
    this.#clearRetry()
    this.#closeSource()
  }

  /** Start over from a fresh connection, whatever state this one is in. */
  restart(): void {
    this.stop()
    this.start()
  }

  /** True while a source is open and delivering. */
  get isOpen(): boolean {
    return this.#source?.readyState === EventSource.OPEN
  }

  /**
   * Drop the current connection and reconnect straight away.
   *
   * For when something outside the stream knows it is dead — the health probe
   * losing the daemon — because a socket can sit in `OPEN` indefinitely without
   * `error` ever firing.
   */
  forceReconnect(reason: string): void {
    if (this.#stopped) return
    this.#clearRetry()
    this.#closeSource()
    this.#backoff = INITIAL_BACKOFF_MS
    // Whatever the old connection was doing, we are not sure we saw all of it.
    this.#interrupted = true
    this.#connect(reason)
  }

  /**
   * Retry now instead of waiting out the backoff — but only when nothing is in
   * flight, so this never interrupts a connection that is still being made.
   */
  reconnectIfClosed(reason: string): void {
    if (this.#stopped) return
    if (this.#source !== null && this.#source.readyState !== EventSource.CLOSED) return
    this.forceReconnect(reason)
  }

  /**
   * There is nothing left to connect to — the daemon said the session is over.
   * Closes as `stop` does, and refuses the retry the daemon's own hang-up would
   * otherwise look like.
   */
  protected halt(): void {
    this.#halted = true
    this.stop()
  }

  #connect(reason?: string): void {
    if (this.#stopped) return
    this.#closeSource()
    const { states, onStatus, onOpen } = this.#options
    const retrying = this.#everOpened || this.#interrupted
    onStatus(retrying ? states.reconnecting : states.connecting, reason)

    let source: EventSource
    try {
      source = new EventSource(this.#url())
    } catch (cause) {
      this.#scheduleRetry(cause instanceof Error ? cause.message : String(cause))
      return
    }
    this.#source = source

    source.onopen = () => {
      this.#backoff = INITIAL_BACKOFF_MS
      const reconnected = this.#everOpened || this.#interrupted
      this.#everOpened = true
      this.#interrupted = false
      if (states.live !== undefined) onStatus(states.live, null)
      onOpen?.({ reconnected })
    }

    // `EventSource` reports every failure as an opaque error event, and cannot
    // tell a dropped connection from a daemon that went away — both are retried.
    source.onerror = () => {
      this.#scheduleRetry(this.#options.dropped)
    }

    this.listen(source)
  }

  #scheduleRetry(error: string): void {
    if (this.#stopped || this.#halted || this.#retryTimer !== null) return
    this.#closeSource()
    // An attempt failed, so the next open is a reconnect even if no attempt has
    // ever succeeded: events during the gap are gone, there is no replay.
    this.#interrupted = true
    this.#options.onStatus(this.#options.states.reconnecting, error)
    // Jitter so a daemon restart does not get a thundering herd of windows.
    const delay = this.#backoff * (0.5 + Math.random() / 2)
    this.#backoff = Math.min(this.#backoff * 2, this.#options.maxBackoffMs)
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      this.#connect()
    }, delay)
  }

  #clearRetry(): void {
    if (this.#retryTimer === null) return
    clearTimeout(this.#retryTimer)
    this.#retryTimer = null
  }

  #closeSource(): void {
    if (this.#source === null) return
    this.#source.onopen = null
    this.#source.onerror = null
    this.#source.close()
    this.#source = null
  }
}

/**
 * An SSE payload as an object, or `undefined` when it is not one.
 *
 * A malformed payload is dropped and logged rather than thrown: one bad frame
 * must not take the connection — or the screen reading it — down with it.
 */
export function parsePayload(raw: string, label: string): object | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    console.error(`[${label}] dropping unparseable payload`, cause, raw)
    return undefined
  }
  if (typeof parsed !== "object" || parsed === null) return undefined
  return parsed
}

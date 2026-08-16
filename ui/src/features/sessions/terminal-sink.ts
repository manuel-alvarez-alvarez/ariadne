/**
 * How a session's log stream is written into the terminal.
 *
 * Two things here are subtle: what "start over" means when a reconnect
 * delivers a fresh snapshot instead of a continuation, and when a resize takes
 * effect. Both come down to the same property of `write()`.
 *
 * `Terminal.reset()` looks like the obvious way to drop the previous
 * connection's output first, but it is an **out-of-band** call. `write()` is
 * buffered and parsed asynchronously, so anything the dropped connection had
 * already queued is parsed *after* a reset that was issued later — the stale
 * output survives the reset that was meant to remove it. Worse, a chunk cut off
 * mid-escape-sequence leaves the parser mid-sequence, and `reset()` does not
 * touch the parser: the first bytes of the new snapshot are then swallowed as
 * the tail of a sequence the old connection never finished.
 *
 * So the reset travels **in the stream itself**, in the same `write` as the
 * snapshot it precedes. It is then ordered with everything queued before it,
 * and it goes through the parser rather than around it.
 */

/** CAN — aborts whatever escape sequence the parser is in the middle of. */
const CANCEL = "\x18"
/** RIS — reset to initial state: screen, scrollback, modes and parser. */
const RESET_TO_INITIAL_STATE = "\x1bc"

/**
 * Sent ahead of every snapshot. CAN first, because RIS starts with `ESC` and an
 * unfinished sequence is exactly the case this has to survive.
 */
export const SNAPSHOT_PREFIX = `${CANCEL}${RESET_TO_INITIAL_STATE}`

/** The part of xterm's `Terminal` this module needs — and all it may use. */
export interface TerminalWriter {
  /** `callback` runs once everything queued up to here has been parsed. */
  write(data: string, callback?: () => void): void
  resize(cols: number, rows: number): void
}

/**
 * Replace everything on the terminal with this connection's opening
 * scrollback. One `write` call: the reset must not be separable from the
 * snapshot it belongs to.
 */
export function writeSnapshot(terminal: TerminalWriter, chunk: string): void {
  terminal.write(SNAPSHOT_PREFIX + chunk)
}

/** Append output written since the last message. */
export function writeDelta(terminal: TerminalWriter, chunk: string): void {
  terminal.write(chunk)
}

/**
 * Draw at the grid the pane draws at, from here on.
 *
 * `resize()` is out of band for the same reason `reset()` is: it applies the
 * moment it is called, ahead of everything `write` has queued and not yet
 * parsed. Output produced *before* the pane was resized would then be laid
 * out at the size it was resized *to* — wrapped at the wrong column, with the
 * repaint that follows correcting a screen it never drew.
 *
 * So the resize travels in the stream too, as the callback of an empty write:
 * it runs once everything queued ahead of it has been parsed, and nothing
 * queued behind it can be parsed before it.
 */
export function writeResize(terminal: TerminalWriter, cols: number, rows: number): void {
  terminal.write("", () => terminal.resize(cols, rows))
}

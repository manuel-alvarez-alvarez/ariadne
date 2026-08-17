/**
 * Where to cut a value that does not fit its column.
 *
 * Cutting the end is the browser's default and the right one for most things.
 * It is the wrong one for values whose *tail* is what identifies them — a
 * branch is `ariadne/task-<26-char ULID>/<slug>`, so an ellipsis at the end
 * keeps the part no one can read and drops the part a person recognises.
 *
 * The split is by segment rather than by length: the last `/`-separated piece
 * is the name, everything before it is the prefix that may go. Values with no
 * separator fall back to keeping their last few characters, the way
 * {@link import("./ids").shortId} does.
 */

/** How much of a separator-less value is worth keeping, as in `shortId`. */
const TAIL_CHARS = 8

/**
 * Splits a value into the part that may be truncated and the part that must
 * survive. The two concatenated are always the value itself, so a renderer can
 * shrink the head and still show the whole thing when there is room.
 */
export function middleTruncate(value: string): { head: string; tail: string } {
  const cut = value.lastIndexOf("/")
  // A leading or trailing slash names nothing on its own; those fall through
  // to the character split below.
  if (cut > 0 && cut < value.length - 1) {
    return { head: value.slice(0, cut), tail: value.slice(cut) }
  }
  if (value.length <= TAIL_CHARS) return { head: value, tail: "" }
  return { head: value.slice(0, -TAIL_CHARS), tail: value.slice(-TAIL_CHARS) }
}

/**
 * How a row's secondary text is shaped so it never crowds out the row's name.
 *
 * The details are ids and branches: `ariadne/task-01m06j3y2rsx7j5t9b-command-
 * palette` is longer than the popup and it is the *end* of it — the slug, or
 * failing that the tail of the ulid — that says which branch it is. So the row
 * gives up the middle: {@link splitDetail} cuts the text in two, and the head
 * is what CSS truncates while the tail keeps its width.
 */

/**
 * How much of the end is kept, in characters. Roughly a third of the width a
 * detail is allowed: enough for a branch's trailing slug, small enough that the
 * title still leads the row when the head is gone.
 */
const TAIL_LENGTH = 16

/** Under this there is nothing to gain by splitting: the whole detail is short. */
const MIN_SPLIT_LENGTH = 24

/** Where a branch or a path can be cut without cutting a word in half. */
const SEPARATORS = new Set(["/", "-", "_", ".", ":"])

/**
 * A detail as two pieces: a `head` that may be truncated away, and the `tail`
 * that must survive. The tail starts at a separator when there is one near the
 * end — keeping the whole trailing slug rather than the last letters of it —
 * and is empty for a detail that is short enough to show whole.
 */
export function splitDetail(text: string): { head: string; tail: string } {
  if (text.length < MIN_SPLIT_LENGTH) return { head: text, tail: "" }

  const earliest = text.length - TAIL_LENGTH
  for (let at = earliest; at < text.length; at++) {
    if (SEPARATORS.has(text[at] ?? "")) return { head: text.slice(0, at), tail: text.slice(at) }
  }
  return { head: text.slice(0, earliest), tail: text.slice(earliest) }
}
